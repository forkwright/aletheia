use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::helpers::*;

#[tokio::test]
async fn sessions_require_auth() {
    let (app, _dir) = app().await;
    let req = json_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "test"
        })),
    );

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn valid_token_passes() {
    let (app, _dir) = app().await;
    let session = create_test_session(&app).await;
    assert!(session["id"].is_string());
    assert_eq!(session["nous_id"], "syn");
}

#[tokio::test]
async fn expired_token_rejected() {
    use aletheia_symbolon::types::{Claims, Role, TokenKind};

    let (app, _dir) = app().await;

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
    let token = test_jwt_manager().encode_claims(&claims).unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "nous_id": "syn",
                "session_key": "test"
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn malformed_token_rejected() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", "Bearer not.a.valid.jwt")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "nous_id": "syn",
                "session_key": "test"
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn missing_bearer_prefix() {
    let (app, _dir) = app().await;
    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", token)
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "nous_id": "syn",
                "session_key": "test"
            }))
            .unwrap(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn missing_auth_header_returns_401() {
    let (app, _dir) = app().await;
    let req = json_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "no-auth-test"
        })),
    );

    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Auth mode ───────────────────────────────────────────────────────────────

async fn app_auth_disabled() -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state().await;
    let default_config = aletheia_taxis::config::AletheiaConfig::default();
    let (config_tx, _config_rx) = tokio::sync::watch::channel(default_config);
    let state = Arc::new(AppState {
        auth_mode: "none".to_owned(),
        session_store: Arc::clone(&state.session_store),
        nous_manager: Arc::clone(&state.nous_manager),
        provider_registry: Arc::clone(&state.provider_registry),
        tool_registry: Arc::clone(&state.tool_registry),
        oikos: Arc::clone(&state.oikos),
        jwt_manager: Arc::clone(&state.jwt_manager),
        start_time: state.start_time,
        config: Arc::clone(&state.config),
        config_tx,
        idempotency_cache: Arc::clone(&state.idempotency_cache),
        shutdown: state.shutdown.clone(),
        #[cfg(feature = "knowledge-store")]
        knowledge_store: None,
    });
    (build_router(state, &test_security_config()), dir)
}

#[tokio::test]
async fn auth_mode_none_allows_unauthenticated_access() {
    let (router, _dir) = app_auth_disabled().await;
    let req = json_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "no-auth-mode"
        })),
    );

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn auth_mode_none_injects_anonymous_identity() {
    let (router, _dir) = app_auth_disabled().await;
    let resp = router
        .oneshot(Request::get("/api/v1/nous").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

// ── Endpoint auth requirements ──────────────────────────────────────────────

#[tokio::test]
async fn nous_list_requires_auth() {
    let (app, _dir) = app().await;
    let req = Request::get("/api/v1/nous").body(Body::empty()).unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn nous_status_requires_auth() {
    let (app, _dir) = app().await;
    let req = Request::get("/api/v1/nous/syn")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn nous_tools_requires_auth() {
    let (app, _dir) = app().await;
    let req = Request::get("/api/v1/nous/syn/tools")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn events_endpoint_requires_auth() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/api/v1/events").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn config_get_requires_auth() {
    let (app, _dir) = app().await;
    let req = Request::get("/api/v1/config").body(Body::empty()).unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn config_section_requires_auth() {
    let (app, _dir) = app().await;
    let req = Request::get("/api/v1/config/gateway")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
