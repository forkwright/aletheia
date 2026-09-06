use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, FromRef};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::helpers::*;
use crate::server::apply_reload;
use crate::state::{AppState, ConfigState};

/// Build a router with the requested metrics exposition mode.
async fn app_with_metrics_mode(
    mode: taxis::config::MetricsMode,
    detailed: bool,
) -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state().await;
    {
        // WHY(#5929): the `expose` handler now reads the live config rather
        // than the startup-cached `AppState::metrics_mode`/`metrics_detailed`
        // fields, so both must agree for these tests to exercise the mode
        // they name.
        let mut config = state.config.write().await;
        config.gateway.metrics.mode = mode;
        config.gateway.metrics.detailed = detailed;
    }
    let state = Arc::new(AppState {
        metrics_mode: mode,
        metrics_detailed: detailed,
        ..(*state).clone()
    });
    (build_router(state, &test_security_config()), dir)
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "current_thread executor; no deadlock risk — GAUGE_TESTS is never acquired inside the awaited request path"
)]
async fn metrics_local_only_allows_loopback() {
    let _guard = crate::metrics::gauge_lock();
    let (app, _dir) = app_with_metrics_mode(taxis::config::MetricsMode::LocalOnly, false).await;
    let mut req = Request::get("/metrics").body(Body::empty()).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1234))));
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_local_only_denies_remote() {
    let (app, _dir) = app_with_metrics_mode(taxis::config::MetricsMode::LocalOnly, false).await;
    let resp = app
        .oneshot(
            Request::get("/metrics")
                .header("x-forwarded-for", "203.0.113.1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // Without real TCP, ConnectInfo is absent, so the handler treats the peer
    // as non-loopback and denies the request.
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "current_thread executor; no deadlock risk — GAUGE_TESTS is never acquired inside the awaited request path"
)]
async fn metrics_public_allows_unauthenticated_remote() {
    let _guard = crate::metrics::gauge_lock();
    let (app, _dir) = app_with_metrics_mode(taxis::config::MetricsMode::Public, false).await;
    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_bearer_requires_authentication() {
    let (app, _dir) = app_with_metrics_mode(taxis::config::MetricsMode::Bearer, false).await;
    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "current_thread executor; no deadlock risk — GAUGE_TESTS is never acquired inside the awaited request path"
)]
async fn metrics_bearer_accepts_valid_token() {
    let _guard = crate::metrics::gauge_lock();
    let (app, _dir) = app_with_metrics_mode(taxis::config::MetricsMode::Bearer, false).await;
    let resp = app.oneshot(authed_get("/metrics")).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_disabled_returns_not_found() {
    let (app, _dir) = app_with_metrics_mode(taxis::config::MetricsMode::Disabled, false).await;
    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "current_thread executor; no deadlock risk — GAUGE_TESTS is never acquired inside the awaited request path"
)]
async fn metrics_redacts_sensitive_labels_by_default() {
    let _guard = crate::metrics::gauge_lock();
    let (app, _dir) = app_with_metrics_mode(taxis::config::MetricsMode::Public, false).await;

    // Record an HTTP request so the registry contains a `path` label.
    let _ = app
        .clone()
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains(r#"path="redacted""#),
        "default metrics did not redact path label: {body}"
    );
}

#[tokio::test]
#[expect(
    clippy::await_holding_lock,
    reason = "current_thread executor; no deadlock risk — GAUGE_TESTS is never acquired inside the awaited request path"
)]
async fn metrics_detailed_preserves_sensitive_labels() {
    let _guard = crate::metrics::gauge_lock();
    let (app, _dir) = app_with_metrics_mode(taxis::config::MetricsMode::Public, true).await;

    let _ = app
        .clone()
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        !body.contains(r#"path="redacted""#),
        "detailed metrics redacted path label: {body}"
    );
    assert!(
        body.contains(r#"path="/api/health""#),
        "detailed metrics did not preserve path label: {body}"
    );
}

/// #5929: `/metrics` must reflect a hot config reload of
/// `gateway.metrics.mode` without a process restart. Before the fix,
/// `expose()` read a startup-cached `AppState::metrics_mode` that
/// `apply_reload` never re-derived, so a reload from `local_only` to
/// `disabled` had no observable effect until the process restarted.
#[tokio::test]
async fn metrics_reload_from_local_only_to_disabled_takes_effect_live() {
    let (state, _dir) = test_state().await;
    {
        let mut config = state.config.write().await;
        config.gateway.metrics.mode = taxis::config::MetricsMode::LocalOnly;
    }
    let state = Arc::new(AppState {
        metrics_mode: taxis::config::MetricsMode::LocalOnly,
        metrics_detailed: false,
        ..(*state).clone()
    });
    // One router, built once, kept for both requests below — nothing about
    // the app or its state is reconstructed between them. If the reload
    // only took effect on a freshly built router, that would be a restart
    // in disguise.
    let app = build_router(Arc::clone(&state), &test_security_config());

    // Sanity: local_only initially serves loopback scrapes.
    let mut loopback_req = Request::get("/metrics").body(Body::empty()).unwrap();
    loopback_req
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1234))));
    let resp = app.clone().oneshot(loopback_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Reload `gateway.metrics.mode` -> `disabled` through the same
    // `apply_reload` path the real SIGHUP handler drives, against the exact
    // `state` backing the already-built `app` above.
    let old_config = state.config.read().await.clone();
    let mut new_config = old_config.clone();
    new_config.gateway.metrics.mode = taxis::config::MetricsMode::Disabled;
    let diff = taxis::reload::diff_configs(&old_config, &new_config).unwrap();
    assert!(
        diff.cold_changes().is_empty(),
        "gateway.metrics.mode must be hot-reloadable, not cold: {:?}",
        diff.cold_changes()
    );
    let outcome = taxis::reload::ReloadOutcome { new_config, diff };
    apply_reload(&ConfigState::from_ref(&state), outcome).await;

    // Same `app`, no restart: the next scrape must see `disabled`.
    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "reload to disabled must be visible on /metrics without restart"
    );
}
