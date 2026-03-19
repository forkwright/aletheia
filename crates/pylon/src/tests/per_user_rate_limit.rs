#![expect(
    clippy::indexing_slicing,
    reason = "test: vec/JSON indices valid after asserting len or known structure"
)]
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use aletheia_taxis::config::PerUserRateLimitConfig;

use super::helpers::*;

/// Build a router with per-user rate limiting enabled with the given config.
async fn app_with_per_user_limits(
    config: PerUserRateLimitConfig,
) -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state().await;
    let security = SecurityConfig {
        csrf_enabled: false,
        per_user_rate_limit: config,
        ..SecurityConfig::default()
    };
    (build_router(state, &security), dir)
}

/// Build a router with very tight per-user rate limits for testing.
async fn app_tight_limits() -> (axum::Router, tempfile::TempDir) {
    app_with_per_user_limits(PerUserRateLimitConfig {
        enabled: true,
        default_rpm: 60,
        default_burst: 2,
        llm_rpm: 60,
        llm_burst: 1,
        tool_rpm: 60,
        tool_burst: 1,
        stale_after_secs: 600,
    })
    .await
}

#[tokio::test]
async fn requests_under_limit_succeed() {
    let (router, _dir) = app_tight_limits().await;

    let resp = router
        .clone()
        .oneshot(authed_get("/api/v1/nous"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = router
        .clone()
        .oneshot(authed_get("/api/v1/nous"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn requests_over_limit_return_429() {
    let (router, _dir) = app_tight_limits().await;

    router
        .clone()
        .oneshot(authed_get("/api/v1/nous"))
        .await
        .unwrap();
    router
        .clone()
        .oneshot(authed_get("/api/v1/nous"))
        .await
        .unwrap();

    let resp = router
        .clone()
        .oneshot(authed_get("/api/v1/nous"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn rate_limited_response_includes_retry_after() {
    let (router, _dir) = app_with_per_user_limits(PerUserRateLimitConfig {
        enabled: true,
        default_rpm: 60,
        default_burst: 1,
        llm_rpm: 60,
        llm_burst: 1,
        tool_rpm: 60,
        tool_burst: 1,
        stale_after_secs: 600,
    })
    .await;

    router
        .clone()
        .oneshot(authed_get("/api/v1/nous"))
        .await
        .unwrap();

    let resp = router
        .clone()
        .oneshot(authed_get("/api/v1/nous"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    let retry_after = resp
        .headers()
        .get("retry-after")
        .expect("must have Retry-After header");
    let secs: u64 = retry_after.to_str().unwrap().parse().unwrap();
    assert!(secs >= 1, "Retry-After must be at least 1 second");
}

#[tokio::test]
async fn rate_limited_body_contains_error_details() {
    let (router, _dir) = app_with_per_user_limits(PerUserRateLimitConfig {
        enabled: true,
        default_rpm: 60,
        default_burst: 1,
        llm_rpm: 60,
        llm_burst: 1,
        tool_rpm: 60,
        tool_burst: 1,
        stale_after_secs: 600,
    })
    .await;

    router
        .clone()
        .oneshot(authed_get("/api/v1/nous"))
        .await
        .unwrap();

    let resp = router
        .clone()
        .oneshot(authed_get("/api/v1/nous"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "rate_limited");
    assert!(body["error"]["details"]["retry_after_secs"].is_number());
    assert_eq!(body["error"]["details"]["category"], "general");
}

#[tokio::test]
async fn user_a_limit_does_not_affect_user_b() {
    use aletheia_symbolon::types::Role;

    let (router, _dir) = app_with_per_user_limits(PerUserRateLimitConfig {
        enabled: true,
        default_rpm: 60,
        default_burst: 1,
        llm_rpm: 60,
        llm_burst: 1,
        tool_rpm: 60,
        tool_burst: 1,
        stale_after_secs: 600,
    })
    .await;

    let jwt_manager = test_jwt_manager();
    let token_a = jwt_manager
        .issue_access("alice", Role::Operator, None)
        .expect("token for alice");
    let token_b = jwt_manager
        .issue_access("bob", Role::Operator, None)
        .expect("token for bob");

    let req = Request::get("/api/v1/nous")
        .header("authorization", format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    router.clone().oneshot(req).await.unwrap();

    let req = Request::get("/api/v1/nous")
        .header("authorization", format!("Bearer {token_a}"))
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "alice should be rate limited"
    );

    let req = Request::get("/api/v1/nous")
        .header("authorization", format!("Bearer {token_b}"))
        .body(Body::empty())
        .unwrap();
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "bob must not be affected by alice's rate limit"
    );
}

#[tokio::test]
async fn disabled_per_user_rate_limit_does_not_block() {
    let (router, _dir) = app_with_per_user_limits(PerUserRateLimitConfig {
        enabled: false,
        default_burst: 1,
        ..PerUserRateLimitConfig::default()
    })
    .await;

    for _ in 0..5 {
        let resp = router
            .clone()
            .oneshot(authed_get("/api/v1/nous"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
