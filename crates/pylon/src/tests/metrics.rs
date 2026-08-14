use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::helpers::*;
use crate::state::AppState;

/// Build a router with the requested metrics exposition mode.
async fn app_with_metrics_mode(
    mode: taxis::config::MetricsMode,
    detailed: bool,
) -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state().await;
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
