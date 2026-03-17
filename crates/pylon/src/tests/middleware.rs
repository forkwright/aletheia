use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::helpers::*;

// ── Security headers ────────────────────────────────────────────────────────

#[tokio::test]
async fn security_headers_present_on_response() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/api/health")
                .body(Body::empty())
                .expect("health request should build"),
        )
        .await
        .expect("health request should receive response");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "health endpoint should return 200"
    );
    assert_eq!(
        resp.headers()
            .get("x-frame-options")
            .expect("x-frame-options header should be present"),
        "DENY",
        "x-frame-options should be DENY"
    );
    assert_eq!(
        resp.headers()
            .get("x-content-type-options")
            .expect("x-content-type-options header should be present"),
        "nosniff",
        "x-content-type-options should be nosniff"
    );
    assert_eq!(
        resp.headers()
            .get("x-xss-protection")
            .expect("x-xss-protection header should be present"),
        "0",
        "x-xss-protection should be 0"
    );
    assert_eq!(
        resp.headers()
            .get("referrer-policy")
            .expect("referrer-policy header should be present"),
        "strict-origin-when-cross-origin",
        "referrer-policy should be strict-origin-when-cross-origin"
    );
    assert_eq!(
        resp.headers()
            .get("content-security-policy")
            .expect("content-security-policy header should be present"),
        "default-src 'self'",
        "content-security-policy should be default-src 'self'"
    );
    // HSTS should NOT be present when TLS is disabled
    assert!(
        resp.headers().get("strict-transport-security").is_none(),
        "HSTS header should not be present when TLS is disabled"
    );
}

#[tokio::test]
async fn hsts_header_present_when_tls_enabled() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        tls_enabled: true,
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let resp = router
        .oneshot(
            Request::get("/api/health")
                .body(Body::empty())
                .expect("health request should build"),
        )
        .await
        .expect("health request should receive response");

    assert_eq!(
        resp.headers()
            .get("strict-transport-security")
            .expect("HSTS header should be present when TLS is enabled"),
        "max-age=31536000; includeSubDomains",
        "HSTS header should have correct max-age and includeSubDomains"
    );
}

// ── Body limit ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn oversized_body_returns_413() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        body_limit_bytes: 100,
        csrf_enabled: false,
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let big_body = "x".repeat(200);
    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(big_body))
        .expect("oversized body request should build");

    let resp = router
        .oneshot(req)
        .await
        .expect("oversized body request should receive response");
    assert_eq!(
        resp.status(),
        StatusCode::PAYLOAD_TOO_LARGE,
        "oversized body should return 413"
    );
}

// ── CSRF ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn csrf_rejects_post_without_header() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        csrf_enabled: true,
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "csrf-test"
        })),
    );

    let resp = router
        .oneshot(req)
        .await
        .expect("CSRF-missing POST should receive response");
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "POST without CSRF header should be rejected with 403"
    );
}

#[tokio::test]
async fn csrf_allows_post_with_correct_header() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        csrf_enabled: true,
        ..SecurityConfig::default()
    };
    // WHY: The CSRF token is now a per-instance CSPRNG value. Read it from
    // the SecurityConfig so the test sends the correct token, not "aletheia".
    let csrf_token = security.csrf_header_value.clone();
    let router = build_router(state, &security);

    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("x-requested-with", csrf_token)
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "nous_id": "syn",
                "session_key": "csrf-test"
            }))
            .expect("session JSON should serialize"),
        ))
        .expect("CSRF POST request should build");

    let resp = router
        .oneshot(req)
        .await
        .expect("CSRF POST request should receive response");
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "POST with correct CSRF header should succeed with 201"
    );
}

#[tokio::test]
async fn csrf_allows_get_without_header() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        csrf_enabled: true,
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let resp = router
        .oneshot(authed_get("/api/v1/nous"))
        .await
        .expect("GET without CSRF header should receive response");

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "GET request should be allowed without CSRF header"
    );
}

#[tokio::test]
async fn csrf_rejects_wrong_header_value() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        csrf_enabled: true,
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("x-requested-with", "wrong-value")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "nous_id": "syn",
                "session_key": "csrf-wrong"
            }))
            .expect("session JSON should serialize"),
        ))
        .expect("CSRF wrong-value request should build");

    let resp = router
        .oneshot(req)
        .await
        .expect("CSRF wrong-value request should receive response");
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "POST with wrong CSRF header value should be rejected with 403"
    );
}

#[tokio::test]
async fn csrf_allows_delete_with_correct_header() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        csrf_enabled: true,
        ..SecurityConfig::default()
    };
    // WHY: The CSRF token is now a per-instance CSPRNG value. Read it from
    // the SecurityConfig so the test sends the correct token, not "aletheia".
    let csrf_token = security.csrf_header_value.clone();
    let router = build_router(Arc::clone(&state), &security);

    let token = default_token();

    let create_req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .header("x-requested-with", csrf_token.clone())
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "nous_id": "syn",
                "session_key": "csrf-delete"
            }))
            .expect("session JSON should serialize"),
        ))
        .expect("session create request should build");

    let resp = router
        .clone()
        .oneshot(create_req)
        .await
        .expect("session create request should receive response");
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "session creation should succeed with 201"
    );
    let session = body_json(resp).await;
    let id = session["id"]
        .as_str()
        .expect("session response should contain id field");

    let delete_req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/sessions/{id}"))
        .header("authorization", format!("Bearer {token}"))
        .header("x-requested-with", csrf_token)
        .body(Body::empty())
        .expect("session delete request should build");

    let resp = router
        .clone()
        .oneshot(delete_req)
        .await
        .expect("session delete request should receive response");
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "session deletion should succeed with 204"
    );
}

// ── CORS ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cors_permissive_when_no_origins_configured() {
    let (state, _dir) = test_state().await;
    let security = test_security_config(); // empty origins = permissive
    let router = build_router(state, &security);

    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/health")
        .header("origin", "http://evil.example.com")
        .header("access-control-request-method", "GET")
        .body(Body::empty())
        .expect("CORS OPTIONS request should build");

    let resp = router
        .oneshot(req)
        .await
        .expect("CORS OPTIONS request should receive response");
    // Permissive CORS should allow any origin
    assert!(
        resp.status().is_success() || resp.status() == StatusCode::NO_CONTENT,
        "permissive CORS should allow any origin"
    );
}

#[tokio::test]
async fn cors_rejects_unlisted_origin() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        allowed_origins: vec!["http://localhost:3000".to_owned()],
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let req = Request::builder()
        .method("OPTIONS")
        .uri("/api/health")
        .header("origin", "http://evil.example.com")
        .header("access-control-request-method", "GET")
        .body(Body::empty())
        .expect("CORS OPTIONS request should build");

    let resp = router
        .oneshot(req)
        .await
        .expect("CORS OPTIONS request should receive response");
    // Should not have the evil origin in access-control-allow-origin
    let allow_origin = resp.headers().get("access-control-allow-origin");
    assert!(
        allow_origin.is_none()
            || allow_origin.expect("allow-origin header to check value")
                != "http://evil.example.com",
        "unlisted origin should not be reflected in access-control-allow-origin"
    );
}

// ── Security config ─────────────────────────────────────────────────────────

#[test]
fn security_config_default_values() {
    let config = SecurityConfig::default();
    assert!(
        config.allowed_origins.is_empty(),
        "default allowed_origins should be empty"
    );
    assert_eq!(
        config.cors_max_age_secs, 3600,
        "default cors_max_age_secs should be 3600"
    );
    assert_eq!(
        config.body_limit_bytes, 1_048_576,
        "default body_limit_bytes should be 1MB"
    );
    assert!(config.csrf_enabled, "CSRF should be enabled by default");
    assert_eq!(
        config.csrf_header_name, "x-requested-with",
        "default CSRF header name should be x-requested-with"
    );
    // WHY: The default CSRF token is now a CSPRNG-generated 32-char hex string
    // rather than the static "aletheia" value, which was guessable.
    assert_eq!(
        config.csrf_header_value.len(),
        32,
        "default CSRF token should be 32 characters"
    );
    assert!(
        config
            .csrf_header_value
            .chars()
            .all(|c| c.is_ascii_hexdigit()),
        "default CSRF token should be a hex string"
    );
    assert_ne!(
        config.csrf_header_value, "aletheia",
        "default CSRF token should not be the static value"
    );
    assert!(!config.tls_enabled, "TLS should be disabled by default");
    assert!(
        config.tls_cert_path.is_none(),
        "tls_cert_path should be None by default"
    );
    assert!(
        config.tls_key_path.is_none(),
        "tls_key_path should be None by default"
    );
}

#[test]
fn security_config_from_gateway() {
    use aletheia_taxis::config::GatewayConfig;

    let gw = GatewayConfig::default();
    let config = SecurityConfig::from_gateway(&gw);
    assert!(
        !config.tls_enabled,
        "TLS should be disabled when gateway config has no TLS"
    );
    assert!(
        config.csrf_enabled,
        "CSRF should be enabled from gateway config"
    );
    assert_eq!(
        config.cors_max_age_secs, 3600,
        "cors_max_age_secs should be 3600 from gateway config"
    );
}

// ── Request ID ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn request_id_present_in_error_responses() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions/nonexistent"))
        .await
        .expect("request to nonexistent session should receive response");

    let body = body_json(resp).await;
    let request_id = body["error"]["request_id"]
        .as_str()
        .expect("error response should contain request_id string");
    assert!(!request_id.is_empty(), "request_id should not be empty");
    assert!(request_id.len() >= 20, "request_id should be a ULID");
}

// ── Routing and error structure ─────────────────────────────────────────────

#[tokio::test]
async fn error_response_has_consistent_structure() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions/nonexistent"))
        .await
        .expect("request to nonexistent session should receive response");

    let body = body_json(resp).await;
    assert!(
        body["error"].is_object(),
        "error response body should have an error object"
    );
    assert!(
        body["error"]["code"].is_string(),
        "error object should have a code string"
    );
    assert!(
        body["error"]["message"].is_string(),
        "error object should have a message string"
    );
    assert!(
        body["error"]["request_id"].is_string(),
        "error response must include request_id"
    );
}

#[tokio::test]
async fn malformed_create_body_returns_400() {
    let (app, _dir) = app().await;
    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(r#"{"invalid": true}"#))
        .expect("malformed body request should build");

    let resp = app
        .oneshot(req)
        .await
        .expect("malformed body request should receive response");
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "malformed create body should return 400 or 422"
    );
}

#[tokio::test]
async fn malformed_send_body_returns_error() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"]
        .as_str()
        .expect("created session should have an id field");

    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/sessions/{id}/messages"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(r#"{"wrong_field": "abc"}"#))
        .expect("malformed send body request should build");

    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("malformed send body request should receive response");
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "malformed send body should return 400 or 422"
    );
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/nonexistent"))
        .await
        .expect("response");

    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "unknown route should return 404"
    );
    let body = body_json(resp).await;
    assert_eq!(
        body["error"]["code"], "not_found",
        "unknown route error should have not_found code"
    );
    assert!(
        body["error"]["request_id"].is_string(),
        "unknown route error should include request_id"
    );
}

#[tokio::test]
async fn old_api_nous_path_returns_gone() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/nous"))
        .await
        .expect("response");

    assert_eq!(
        resp.status(),
        StatusCode::GONE,
        "old /api/nous path should return 410 Gone"
    );
    let body = body_json(resp).await;
    assert_eq!(
        body["error"]["code"], "api_version_required",
        "old nous path error should have api_version_required code"
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string")
            .contains("/api/v1/nous"),
        "error message should reference the new versioned path"
    );
}

#[tokio::test]
async fn fallback_404_returns_json_error() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/totally/unknown/path")
                .body(Body::empty())
                .expect("unknown path request should build"),
        )
        .await
        .expect("response");

    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "unknown path should return 404"
    );
    let body = body_json(resp).await;
    assert_eq!(
        body["error"]["code"], "not_found",
        "fallback 404 error should have not_found code"
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string")
            .contains("/totally/unknown/path"),
        "error message should contain the requested path"
    );
    assert!(
        body["error"]["request_id"].is_string(),
        "fallback 404 error should include request_id"
    );
}

#[tokio::test]
async fn put_on_sessions_returns_405() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("PUT")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", default_token()))
        .body(Body::empty())
        .expect("PUT sessions request should build");

    let resp = app
        .oneshot(req)
        .await
        .expect("PUT sessions request should receive response");
    assert_eq!(
        resp.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "PUT on sessions should return 405"
    );
}

#[tokio::test]
async fn delete_on_nous_returns_405() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/nous")
        .header("authorization", format!("Bearer {}", default_token()))
        .body(Body::empty())
        .expect("DELETE nous request should build");

    let resp = app
        .oneshot(req)
        .await
        .expect("DELETE nous request should receive response");
    assert_eq!(
        resp.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "DELETE on nous should return 405"
    );
}

#[tokio::test]
async fn post_on_health_returns_405() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/api/health")
        .body(Body::empty())
        .expect("POST health request should build");

    let resp = app
        .oneshot(req)
        .await
        .expect("POST health request should receive response");
    assert_eq!(
        resp.status(),
        StatusCode::METHOD_NOT_ALLOWED,
        "POST on health should return 405"
    );
}
