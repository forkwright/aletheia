#![expect(clippy::expect_used, reason = "test assertions use expect")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: JSON indices and byte-slice ranges are valid after asserting status or known protocol shape"
)]
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use koina::http::{API_HEALTH, API_V1};
use pylon::router::build_router;

mod common;
use common::{TestEnv, bearer, issue_test_token, permissive_security, read_body_json};

// Split: build_router construction + auth contracts.

// ── build_router: construction contracts ───────────────────────────────────

#[tokio::test]
async fn build_router_produces_router_with_health_endpoint() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(API_HEALTH)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    // WHY: health may legitimately report "unhealthy" (503) when
    // no providers are registered, so both 200 and 503 are contractually
    // valid. What matters is that the endpoint returns a response at all
    // and that the body parses as the documented HealthResponse shape.
    let status = response.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::SERVICE_UNAVAILABLE,
        "health must return 200 or 503, got {status}",
    );

    let body = read_body_json(response).await;
    assert!(body["status"].is_string(), "health body lacks status");
    assert!(body["version"].is_string(), "health body lacks version");
    assert!(
        body["uptime_seconds"].is_u64(),
        "uptime_seconds must be u64"
    );
    assert!(body["checks"].is_array(), "checks must be an array");
    assert!(
        !body["checks"].as_array().expect("checks array").is_empty(),
        "health must report at least one check"
    );
}

#[tokio::test]
async fn build_router_health_also_served_at_slash_health() {
    // WHY: The router exposes health at both `/api/health` and `/health`
    // for infrastructure compatibility (some load balancers default to
    // `/health`). Regression test: #2814 must not drop the bare path.
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get("/health")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    let status = response.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::SERVICE_UNAVAILABLE,
        "/health must return 200 or 503, got {status}",
    );
}

#[tokio::test]
async fn build_router_is_idempotent_for_shared_state() {
    let env = TestEnv::new().await;
    let router_one = build_router(Arc::clone(&env.state), &permissive_security());
    let router_two = build_router(Arc::clone(&env.state), &permissive_security());

    // WHY: AppState is shared behind Arc and build_router must not consume or
    // mutate it. Regression test: if build_router were to install a one-shot
    // layer that panics on re-entry, routing through the second router would
    // fail. Both should work.
    for router in [router_one, router_two] {
        let response = router
            .oneshot(
                Request::get(API_HEALTH)
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("router response");
        assert!(matches!(
            response.status(),
            StatusCode::OK | StatusCode::SERVICE_UNAVAILABLE
        ));
    }
}

#[tokio::test]
async fn build_router_unknown_path_returns_404_json_envelope() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get("/definitely/not/a/real/path")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = read_body_json(response).await;
    assert_eq!(body["error"]["code"], "not_found");
    assert!(
        body["error"]["request_id"].is_string(),
        "404 must carry a request_id for correlation"
    );
}

#[tokio::test]
async fn build_router_old_api_nous_path_returns_410_gone() {
    // WHY: The unversioned `/api/nous` path was moved to `/api/v1/nous`.
    // The fallback returns 410 Gone with a migration hint instead of 404
    // so older clients see an actionable error. Regression test: this
    // migration hint is a public contract.
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get("/api/nous")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::GONE);
    let body = read_body_json(response).await;
    assert_eq!(body["error"]["code"], "api_version_required");
    let message = body["error"]["message"]
        .as_str()
        .expect("message is a string");
    assert!(
        message.contains("/api/v1/nous"),
        "migration hint must name the new path, got {message}",
    );
}

// ── build_router: auth contracts ───────────────────────────────────────────

#[tokio::test]
async fn protected_endpoint_rejects_missing_bearer() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_endpoint_accepts_valid_bearer() {
    let env = TestEnv::new().await;
    let token = issue_test_token(&env.state);
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .header("authorization", bearer(&token))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn protected_endpoint_rejects_malformed_bearer() {
    let env = TestEnv::new().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .header("authorization", "Bearer not.a.valid.jwt")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_endpoint_rejects_bearer_without_prefix() {
    let env = TestEnv::new().await;
    let token = issue_test_token(&env.state);
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .header("authorization", token)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_mode_none_allows_access_without_bearer() {
    let env = TestEnv::builder().auth_mode("none").build().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/nous"))
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "auth_mode=none must not require a bearer on protected routes"
    );
}
