//! Integration tests for the pylon HTTP gateway.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use secrecy::SecretString;
use tower::ServiceExt;

use aletheia_hermeneus::provider::{LlmProvider, ProviderRegistry};
use aletheia_hermeneus::types::*;
use aletheia_mneme::store::SessionStore;
use aletheia_nous::config::{NousConfig, PipelineConfig};
use aletheia_nous::manager::NousManager;
use aletheia_organon::registry::ToolRegistry;
use aletheia_symbolon::jwt::{JwtConfig, JwtManager};
use aletheia_taxis::oikos::Oikos;

use crate::router::build_router;
use crate::state::AppState;

// --- Mock Provider ---

struct MockProvider {
    response: CompletionResponse,
}

impl MockProvider {
    fn new() -> Self {
        Self {
            response: CompletionResponse {
                id: "msg_test".to_owned(),
                model: "mock-model".to_owned(),
                stop_reason: StopReason::EndTurn,
                content: vec![ContentBlock::Text {
                    text: "Hello from mock!".to_owned(),
                }],
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                    ..Usage::default()
                },
            },
        }
    }
}

impl LlmProvider for MockProvider {
    fn complete(&self, _request: &CompletionRequest) -> aletheia_hermeneus::error::Result<CompletionResponse> {
        Ok(self.response.clone())
    }

    fn supported_models(&self) -> &[&str] {
        &["mock-model", "claude-opus-4-20250514"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "mock"
    }
}

// --- JWT Test Helpers ---

fn test_jwt_manager() -> Arc<JwtManager> {
    Arc::new(JwtManager::new(JwtConfig {
        signing_key: SecretString::from("test-secret-key-for-jwt".to_owned()),
        access_ttl: Duration::from_secs(3600),
        refresh_ttl: Duration::from_secs(86400),
        issuer: "aletheia-test".to_owned(),
    }))
}

fn default_token() -> String {
    test_jwt_manager()
        .issue_access("test-user", aletheia_symbolon::types::Role::Operator, None)
        .expect("test token")
}

// --- Test Helpers ---

async fn test_state() -> (Arc<AppState>, tempfile::TempDir) {
    test_state_with_provider(true).await
}

async fn test_state_with_provider(with_provider: bool) -> (Arc<AppState>, tempfile::TempDir) {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();

    // Create oikos directory structure required by the actor pipeline
    std::fs::create_dir_all(root.join("nous/syn")).expect("mkdir nous/syn");
    std::fs::create_dir_all(root.join("shared")).expect("mkdir shared");
    std::fs::create_dir_all(root.join("theke")).expect("mkdir theke");
    std::fs::write(root.join("nous/syn/SOUL.md"), "I am Syn, a test agent.").expect("write SOUL.md");

    let store = SessionStore::open_in_memory().expect("in-memory store");
    let oikos = Arc::new(Oikos::from_root(root));

    let mut provider_registry = ProviderRegistry::new();
    if with_provider {
        provider_registry.register(Box::new(MockProvider::new()));
    }
    let provider_registry = Arc::new(provider_registry);
    let tool_registry = Arc::new(ToolRegistry::new());

    let mut nous_manager = NousManager::new(
        Arc::clone(&provider_registry),
        Arc::clone(&tool_registry),
        Arc::clone(&oikos),
        None,
        None,
        None,
    );

    let nous_config = NousConfig {
        id: "syn".to_owned(),
        model: "mock-model".to_owned(),
        ..NousConfig::default()
    };
    nous_manager.spawn(nous_config, PipelineConfig::default()).await;

    let jwt_manager = test_jwt_manager();

    let state = Arc::new(AppState {
        session_store: Arc::new(Mutex::new(store)),
        nous_manager: Arc::new(nous_manager),
        provider_registry,
        tool_registry,
        oikos,
        jwt_manager,
        start_time: Instant::now(),
    });

    (state, dir)
}

async fn app() -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state().await;
    (build_router(state), dir)
}

async fn app_no_providers() -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state_with_provider(false).await;
    (build_router(state), dir)
}

fn json_request(method: &str, uri: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");

    match body {
        Some(b) => builder
            .body(Body::from(serde_json::to_vec(&b).unwrap()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

fn authed_request(method: &str, uri: &str, body: Option<serde_json::Value>) -> Request<Body> {
    let token = default_token();
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"));

    match body {
        Some(b) => builder
            .body(Body::from(serde_json::to_vec(&b).unwrap()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

fn authed_get(uri: &str) -> Request<Body> {
    let token = default_token();
    Request::get(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

fn authed_delete(uri: &str) -> Request<Body> {
    let token = default_token();
    Request::delete(uri)
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .unwrap()
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn body_string(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn create_test_session(app: &axum::Router) -> serde_json::Value {
    let req = authed_request(
        "POST",
        "/api/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "test-session"
        })),
    );
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    body_json(resp).await
}

// --- Auth Tests ---

#[tokio::test]
async fn health_no_auth_required() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "healthy");
}

#[tokio::test]
async fn sessions_require_auth() {
    let (app, _dir) = app().await;
    let req = json_request(
        "POST",
        "/api/sessions",
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
    use jsonwebtoken::{Algorithm, EncodingKey, Header};

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
    let token = jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(b"test-secret-key-for-jwt"),
    )
    .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/api/sessions")
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
        .uri("/api/sessions")
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
        .uri("/api/sessions")
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

// --- Health Tests ---

#[tokio::test]
async fn health_returns_200() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "healthy");
    assert!(body["version"].is_string());
    assert!(body["uptime_seconds"].is_number());
    assert!(body["checks"].is_array());
}

#[tokio::test]
async fn health_degraded_without_providers() {
    let (app, _dir) = app_no_providers().await;
    let resp = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert_eq!(body["status"], "degraded");
}

// --- Session CRUD Tests ---

#[tokio::test]
async fn create_session_returns_201() {
    let (app, _dir) = app().await;
    let session = create_test_session(&app).await;
    assert!(session["id"].is_string());
    assert_eq!(session["nous_id"], "syn");
    assert_eq!(session["session_key"], "test-session");
    assert_eq!(session["status"], "active");
}

#[tokio::test]
async fn get_session_returns_created_session() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/sessions/{id}")))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["id"], id);
    assert_eq!(body["nous_id"], "syn");
}

#[tokio::test]
async fn get_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/sessions/nonexistent"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "session_not_found");
}

#[tokio::test]
async fn close_session_returns_204() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let resp = router
        .clone()
        .oneshot(authed_delete(&format!("/api/sessions/{id}")))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn get_closed_session_shows_archived() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    router
        .clone()
        .oneshot(authed_delete(&format!("/api/sessions/{id}")))
        .await
        .unwrap();

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/sessions/{id}")))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "archived");
}

#[tokio::test]
async fn close_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_delete("/api/sessions/nonexistent"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// --- History Tests ---

#[tokio::test]
async fn history_empty_for_new_session() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/sessions/{id}/history")))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body["messages"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn history_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/sessions/nonexistent/history"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn history_with_limit() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state));

    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    {
        let store = state.session_store.lock().unwrap();
        for i in 1..=5 {
            store
                .append_message(
                    id,
                    aletheia_mneme::types::Role::User,
                    &format!("message {i}"),
                    None,
                    None,
                    10,
                )
                .unwrap();
        }
    }

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/sessions/{id}/history?limit=3")))
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert_eq!(body["messages"].as_array().unwrap().len(), 3);
}

// --- SSE Message Tests ---

#[tokio::test]
async fn send_message_returns_sse_content_type() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state));
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Hello!" })),
    );

    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let content_type = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(content_type.contains("text/event-stream"));
}

#[tokio::test]
async fn send_message_stream_contains_events() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state));
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Hello!" })),
    );

    let resp = router.clone().oneshot(req).await.unwrap();
    let body = body_string(resp).await;

    assert!(body.contains("event: text_delta"), "should contain text_delta event");
    assert!(body.contains("Hello from mock!"), "should contain mock response text");
    assert!(body.contains("event: message_complete"), "should contain message_complete event");
}

#[tokio::test]
async fn send_message_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/sessions/nonexistent/messages",
        Some(serde_json::json!({ "content": "Hello!" })),
    );

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn send_empty_message_returns_400() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "" })),
    );

    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn send_message_stores_in_history() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state));
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Hello!" })),
    );
    let resp = router.clone().oneshot(req).await.unwrap();
    let _ = body_string(resp).await;

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/sessions/{id}/history")))
        .await
        .unwrap();

    let body = body_json(resp).await;
    let messages = body["messages"].as_array().unwrap();
    assert!(messages.len() >= 2, "should have user + assistant messages");

    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "Hello!");
}

// --- Error Format Tests ---

#[tokio::test]
async fn error_response_has_consistent_structure() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/sessions/nonexistent"))
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert!(body["error"].is_object());
    assert!(body["error"]["code"].is_string());
    assert!(body["error"]["message"].is_string());
}

#[tokio::test]
async fn malformed_create_body_returns_400() {
    let (app, _dir) = app().await;
    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri("/api/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(r#"{"invalid": true}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[tokio::test]
async fn malformed_send_body_returns_error() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/sessions/{id}/messages"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from(r#"{"wrong_field": "abc"}"#))
        .unwrap();

    let resp = router.clone().oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY
    );
}

// --- Nous Tests ---

#[tokio::test]
async fn list_nous_returns_agents() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/nous"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let agents = body["nous"].as_array().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["id"], "syn");
}

#[tokio::test]
async fn get_nous_status() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/nous/syn"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["id"], "syn");
    assert!(body["context_window"].is_number());
    assert!(body["max_output_tokens"].is_number());
}

#[tokio::test]
async fn get_unknown_nous_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/nous/nonexistent"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "nous_not_found");
}

#[tokio::test]
async fn get_nous_tools() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/nous/syn/tools"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body["tools"].is_array());
}

// --- Concurrent access ---

#[tokio::test]
async fn concurrent_session_creation() {
    let (state, _dir) = test_state().await;
    let mut handles = Vec::new();

    for i in 0..5 {
        let router = build_router(Arc::clone(&state));
        handles.push(tokio::spawn(async move {
            let req = authed_request(
                "POST",
                "/api/sessions",
                Some(serde_json::json!({
                    "nous_id": "syn",
                    "session_key": format!("concurrent-{i}")
                })),
            );
            let resp = router.oneshot(req).await.unwrap();
            resp.status()
        }));
    }

    for handle in handles {
        let status = handle.await.unwrap();
        assert_eq!(status, StatusCode::CREATED);
    }
}

// --- SSE with no provider ---

#[tokio::test]
async fn send_message_no_provider_returns_error() {
    let (state, _dir) = test_state_with_provider(false).await;
    let router = build_router(Arc::clone(&state));
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Hello!" })),
    );

    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// --- Actor-routed tests ---

#[tokio::test]
async fn send_message_routes_through_actor() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state));
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Test routing" })),
    );

    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;

    assert!(body.contains("event: text_delta"), "should have text_delta");
    assert!(body.contains("Hello from mock!"), "should contain mock response");
    assert!(body.contains("event: message_complete"), "should have message_complete");
    assert!(body.contains("end_turn"), "stop_reason should be end_turn");
}

#[tokio::test]
async fn nous_list_from_manager() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state));

    let resp = router
        .oneshot(authed_get("/api/nous"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let agents = body["nous"].as_array().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["id"], "syn");
    assert_eq!(agents[0]["model"], "mock-model");
    assert_eq!(agents[0]["status"], "active");
}

#[tokio::test]
async fn empty_json_body_send_message_returns_400() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().expect("session id");

    let req = authed_request(
        "POST",
        &format!("/api/sessions/{id}/messages"),
        Some(serde_json::json!({})),
    );

    let resp = router.clone().oneshot(req).await.expect("response");
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "expected 400 or 422, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn double_close_session_is_idempotent() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().expect("session id");

    let first = router
        .clone()
        .oneshot(authed_delete(&format!("/api/sessions/{id}")))
        .await
        .expect("first close");
    assert_eq!(first.status(), StatusCode::NO_CONTENT);

    let second = router
        .clone()
        .oneshot(authed_delete(&format!("/api/sessions/{id}")))
        .await
        .expect("second close");
    assert_eq!(second.status(), StatusCode::NO_CONTENT);

    // Session should still be accessible as archived after both closes
    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/sessions/{id}")))
        .await
        .expect("get after double close");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "archived");
}

#[tokio::test]
async fn get_session_after_create_reflects_state() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().expect("session id");

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/sessions/{id}")))
        .await
        .expect("response");

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["id"], id);
    assert_eq!(body["status"], "active");
    assert_eq!(body["nous_id"], "syn");
}

#[tokio::test]
async fn unknown_route_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/nonexistent"))
        .await
        .expect("response");

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn missing_auth_header_returns_401() {
    let (app, _dir) = app().await;
    let req = json_request(
        "POST",
        "/api/sessions",
        Some(serde_json::json!({
            "nous_id": "syn",
            "session_key": "no-auth-test"
        })),
    );

    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
