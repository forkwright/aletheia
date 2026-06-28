#![expect(clippy::expect_used, reason = "test assertions use expect")]
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use koina::http::API_V1;
use koina::secret::SecretString;
use pylon::router::build_router;
use pylon::security::{CorsConfig, CsrfConfig, RateLimitConfig, SecurityConfig, TlsConfig};
use symbolon::jwt::{JwtConfig, JwtManager};
use symbolon::types::{Claims, Role, TokenKind};

mod common;
use common::{TestEnv, bearer, issue_test_token, permissive_security, read_body_json};

// ── JWT round-trip via the public symbolon API wired into AppState ─────────

#[tokio::test]
async fn jwt_issue_then_validate_preserves_sub_and_role() {
    let env = TestEnv::new().await;
    let token = env
        .state
        .jwt_manager
        .issue_access("alice", Role::Admin, None)
        .expect("issue");

    let claims = env.state.jwt_manager.validate(&token).expect("validate");
    assert_eq!(claims.sub, "alice");
    assert_eq!(claims.role, Role::Admin);
    assert_eq!(claims.kind, TokenKind::Access);
    assert!(claims.nous_id.is_none());
}

#[tokio::test]
async fn jwt_agent_token_carries_nous_scope() {
    let env = TestEnv::new().await;
    let token = env
        .state
        .jwt_manager
        .issue_access("agent-syn", Role::Agent, Some("syn"))
        .expect("issue");

    let claims = env.state.jwt_manager.validate(&token).expect("validate");
    assert_eq!(claims.role, Role::Agent);
    assert_eq!(claims.nous_id.as_deref(), Some("syn"));
}

#[tokio::test]
async fn jwt_expired_token_is_rejected_by_router() {
    // WHY: The extractor and the manager must agree on expiry: a token the
    // manager rejects with ExpiredToken must yield 401 at the HTTP layer.
    let env = TestEnv::new().await;
    let claims = Claims {
        sub: "test-user".to_owned(),
        role: Role::Operator,
        nous_id: None,
        iss: "aletheia-test".to_owned(),
        iat: 1_000_000,
        exp: 1_000_001,
        jti: "expired-jti".to_owned(),
        kind: TokenKind::Access,
    };
    let token = env
        .state
        .jwt_manager
        .encode_claims(&claims)
        .expect("encode expired claims");

    assert!(
        env.state.jwt_manager.validate(&token).is_err(),
        "manager must reject expired token",
    );

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

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn jwt_wrong_issuer_is_rejected() {
    let env = TestEnv::new().await;
    let wrong_manager = JwtManager::new(JwtConfig {
        signing_key: SecretString::from("test-secret-key-for-jwt".to_owned()),
        access_ttl: Duration::from_hours(1),
        refresh_ttl: Duration::from_hours(24),
        issuer: "someone-else".to_owned(),
        ..JwtConfig::default()
    });
    let token = wrong_manager
        .issue_access("test-user", Role::Operator, None)
        .expect("issue");

    assert!(
        env.state.jwt_manager.validate(&token).is_err(),
        "token from a different issuer must be rejected"
    );
}

#[tokio::test]
async fn jwt_wrong_signing_key_is_rejected() {
    let env = TestEnv::new().await;
    let wrong_manager = JwtManager::new(JwtConfig {
        signing_key: SecretString::from("a-different-signing-key".to_owned()),
        access_ttl: Duration::from_hours(1),
        refresh_ttl: Duration::from_hours(24),
        issuer: "aletheia-test".to_owned(),
        ..JwtConfig::default()
    });
    let token = wrong_manager
        .issue_access("test-user", Role::Operator, None)
        .expect("issue");

    assert!(
        env.state.jwt_manager.validate(&token).is_err(),
        "token signed with wrong key must be rejected"
    );
}

// ── SecurityConfig and sub-configs: defaults are sensible ──────────────────

#[test]
fn security_config_default_has_1mib_body_limit() {
    let config = SecurityConfig::default();
    assert_eq!(
        config.body_limit_bytes, 1_048_576,
        "default body limit must be 1 MiB to match the documented contract"
    );
}

#[test]
fn security_config_default_enables_csrf() {
    let config = SecurityConfig::default();
    assert!(
        config.csrf.enabled,
        "CSRF defaults to enabled for safety: opt-out, not opt-in"
    );
    assert!(
        !config.csrf.disable_acknowledged,
        "CSRF disable acknowledgement defaults to false"
    );
    assert_eq!(config.csrf.header_name, "x-requested-with");
}

#[test]
fn csrf_config_default_uses_documented_bootstrap_header_value() {
    let csrf = CsrfConfig::default();
    assert_eq!(
        csrf.header_value.expose_secret(),
        "aletheia",
        "default CSRF header value must match the documented first-party client header"
    );
}

#[test]
fn csrf_config_debug_redacts_header_value() {
    let csrf = CsrfConfig {
        header_value: SecretString::from("synthetic-csrf-secret"),
        ..CsrfConfig::default()
    };
    let debug = format!("{csrf:?}");
    assert!(
        !debug.contains("synthetic-csrf-secret"),
        "debug output must not expose the CSRF header value"
    );
    assert!(debug.contains("[REDACTED]"));
}

#[test]
fn tls_config_default_is_disabled() {
    let tls = TlsConfig::default();
    assert!(!tls.enabled, "TLS must be opt-in");
    assert!(tls.cert_path.is_none());
    assert!(tls.key_path.is_none());
}

#[test]
fn rate_limit_config_default_is_disabled_but_sane() {
    let rl = RateLimitConfig::default();
    assert!(!rl.enabled, "rate limiting is opt-in");
    assert_eq!(rl.requests_per_minute, 60);
    assert!(
        !rl.trust_proxy,
        "trust_proxy must default to false: enabling it blindly is a spoofing vector"
    );
}

#[test]
fn cors_config_default_has_empty_allow_list_and_1h_max_age() {
    let cors = CorsConfig::default();
    assert!(
        cors.allowed_origins.is_empty(),
        "default must not pre-allow any origin"
    );
    assert_eq!(cors.max_age_secs, 3600);
}

// ── build_router: CSRF routing behaviour ───────────────────────────────────

#[tokio::test]
async fn csrf_enabled_blocks_post_without_header() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let security = SecurityConfig::default();
    let router = build_router(Arc::clone(&env.state), &security);

    let token = issue_test_token(&env.state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{API_V1}/sessions"))
                .header("content-type", "application/json")
                .header("authorization", bearer(&token))
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "nous_id": "syn",
                        "session_key": "csrf-missing",
                    }))
                    .expect("serialize"),
                ))
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

async fn discover_csrf_header(
    router: axum::Router,
    token: &str,
) -> (String, Option<String>, serde_json::Value) {
    let response = router
        .oneshot(
            Request::get(format!("{API_V1}/client/contract"))
                .header("authorization", bearer(token))
                .body(Body::empty())
                .expect("build contract request"),
        )
        .await
        .expect("contract response");

    assert_eq!(response.status(), StatusCode::OK);
    let body = read_body_json(response).await;
    let header_name = csrf_field(&body, "headerName")
        .as_str()
        .expect("csrf header name")
        .to_owned();
    let header_value = json_field(&body, "csrf")
        .get("headerValue")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    (header_name, header_value, body)
}

fn json_field<'a>(value: &'a serde_json::Value, field: &str) -> &'a serde_json::Value {
    value
        .get(field)
        .unwrap_or_else(|| panic!("missing JSON field {field}"))
}

fn csrf_field<'a>(body: &'a serde_json::Value, field: &str) -> &'a serde_json::Value {
    json_field(json_field(body, "csrf"), field)
}

#[tokio::test]
async fn client_contract_reports_default_example_csrf_header() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let security = SecurityConfig {
        csrf: CsrfConfig {
            enabled: true,
            ..CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let router = build_router(Arc::clone(&env.state), &security);
    let token = issue_test_token(&env.state);

    let (header_name, header_value, body) = discover_csrf_header(router, &token).await;

    assert_eq!(csrf_field(&body, "enabled").as_bool(), Some(true));
    assert_eq!(header_name, "x-requested-with");
    assert_eq!(header_value.as_deref(), Some("aletheia"));
}

#[tokio::test]
async fn client_contract_reports_custom_csrf_header() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let security = SecurityConfig {
        csrf: CsrfConfig {
            enabled: true,
            header_name: "x-custom-csrf".to_owned(),
            header_value: SecretString::from("custom-csrf-value"),
            ..CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let router = build_router(Arc::clone(&env.state), &security);
    let token = issue_test_token(&env.state);

    let (header_name, header_value, body) = discover_csrf_header(router, &token).await;

    assert_eq!(csrf_field(&body, "enabled").as_bool(), Some(true));
    assert_eq!(header_name, "x-custom-csrf");
    assert_eq!(header_value.as_deref(), Some("custom-csrf-value"));
}

#[tokio::test]
async fn client_contract_reports_disabled_csrf_without_value() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());
    let token = issue_test_token(&env.state);

    let (header_name, header_value, body) = discover_csrf_header(router, &token).await;

    assert_eq!(csrf_field(&body, "enabled").as_bool(), Some(false));
    assert_eq!(header_name, "x-requested-with");
    assert!(header_value.is_none());
}

#[tokio::test]
async fn discovered_csrf_header_allows_desktop_post_put_delete_actions() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let security = SecurityConfig {
        csrf: CsrfConfig {
            enabled: true,
            header_name: "x-desktop-csrf".to_owned(),
            header_value: SecretString::from("desktop-csrf-value"),
            ..CsrfConfig::default()
        },
        ..SecurityConfig::default()
    };
    let router = build_router(Arc::clone(&env.state), &security);
    let token = issue_test_token(&env.state);
    let (header_name, header_value, _) = discover_csrf_header(router.clone(), &token).await;
    let header_value = header_value.expect("enabled csrf value");

    let create_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{API_V1}/sessions"))
                .header("content-type", "application/json")
                .header("authorization", bearer(&token))
                .header(header_name.as_str(), header_value.as_str())
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "nous_id": "syn",
                        "session_key": "csrf-discovered",
                    }))
                    .expect("serialize create request"),
                ))
                .expect("build create request"),
        )
        .await
        .expect("create response");
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created = read_body_json(create_response).await;
    let session_id = json_field(&created, "id")
        .as_str()
        .expect("created session id");

    let write_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("{API_V1}/workspace/files/content"))
                .header("content-type", "application/json")
                .header("authorization", bearer(&token))
                .header(header_name.as_str(), header_value.as_str())
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "path": "csrf-discovered.md",
                        "content": "desktop write through discovered csrf",
                    }))
                    .expect("serialize write request"),
                ))
                .expect("build write request"),
        )
        .await
        .expect("write response");
    assert_eq!(write_response.status(), StatusCode::OK);

    let delete_response = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("{API_V1}/sessions/{session_id}"))
                .header("authorization", bearer(&token))
                .header(header_name.as_str(), header_value.as_str())
                .body(Body::empty())
                .expect("build delete request"),
        )
        .await
        .expect("delete response");
    assert_eq!(delete_response.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn csrf_disabled_allows_post_without_header() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let token = issue_test_token(&env.state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{API_V1}/sessions"))
                .header("content-type", "application/json")
                .header("authorization", bearer(&token))
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "nous_id": "syn",
                        "session_key": "csrf-disabled",
                    }))
                    .expect("serialize"),
                ))
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn csrf_disabled_rejects_cross_origin_post_with_origin_header() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let token = issue_test_token(&env.state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{API_V1}/sessions"))
                .header("host", "localhost:18789")
                .header("origin", "http://evil.example.com")
                .header("content-type", "application/json")
                .header("authorization", bearer(&token))
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "nous_id": "syn",
                        "session_key": "csrf-disabled-cross-origin",
                    }))
                    .expect("serialize"),
                ))
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn csrf_disabled_rejects_cross_origin_post_with_referer_header() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let token = issue_test_token(&env.state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{API_V1}/sessions"))
                .header("host", "localhost:18789")
                .header("referer", "http://evil.example.com/")
                .header("content-type", "application/json")
                .header("authorization", bearer(&token))
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "nous_id": "syn",
                        "session_key": "csrf-disabled-referer-cross-origin",
                    }))
                    .expect("serialize"),
                ))
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn csrf_disabled_allows_same_origin_post() {
    let env = TestEnv::builder().with_actor(true).build().await;
    let router = build_router(Arc::clone(&env.state), &permissive_security());

    let token = issue_test_token(&env.state);
    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("{API_V1}/sessions"))
                .header("host", "localhost:18789")
                .header("origin", "http://localhost:18789")
                .header("content-type", "application/json")
                .header("authorization", bearer(&token))
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "nous_id": "syn",
                        "session_key": "csrf-disabled-same-origin",
                    }))
                    .expect("serialize"),
                ))
                .expect("build request"),
        )
        .await
        .expect("router response");

    assert_eq!(response.status(), StatusCode::CREATED);
}
