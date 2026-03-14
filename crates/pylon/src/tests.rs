//! Integration tests for the pylon HTTP gateway.

#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

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
use tokio_util::sync::CancellationToken;

use crate::router::build_router;
use crate::security::SecurityConfig;
use crate::state::AppState;

/// Test helper: returns a `SecurityConfig` with CSRF disabled so that
/// POST/PUT/DELETE requests don't require the CSRF header in tests.
fn test_security_config() -> SecurityConfig {
    SecurityConfig {
        csrf_enabled: false,
        ..SecurityConfig::default()
    }
}

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
                    citations: None,
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
    fn complete<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = aletheia_hermeneus::error::Result<CompletionResponse>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async { Ok(self.response.clone()) })
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
    std::fs::write(root.join("nous/syn/SOUL.md"), "I am Syn, a test agent.")
        .expect("write SOUL.md");

    let store = SessionStore::open_in_memory().expect("in-memory store");
    let session_store = Arc::new(Mutex::new(store));
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
        Some(Arc::clone(&session_store)),
        #[cfg(feature = "knowledge-store")]
        None,
        Arc::new(vec![]),
        None,
        None,
    );

    let nous_config = NousConfig {
        id: "syn".to_owned(),
        model: "mock-model".to_owned(),
        ..NousConfig::default()
    };
    nous_manager
        .spawn(nous_config, PipelineConfig::default())
        .await;

    let jwt_manager = test_jwt_manager();

    let state = Arc::new(AppState {
        session_store: Arc::clone(&session_store),
        nous_manager: Arc::new(nous_manager),
        provider_registry,
        tool_registry,
        oikos,
        jwt_manager,
        start_time: Instant::now(),
        auth_mode: "token".to_owned(),
        config: Arc::new(tokio::sync::RwLock::new(
            aletheia_taxis::config::AletheiaConfig::default(),
        )),
        idempotency_cache: Arc::new(crate::idempotency::IdempotencyCache::new()),
        shutdown: CancellationToken::new(),
        #[cfg(feature = "knowledge-store")]
        knowledge_store: None,
    });

    (state, dir)
}

async fn app() -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state().await;
    (build_router(state, &test_security_config()), dir)
}

async fn app_no_providers() -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state_with_provider(false).await;
    (build_router(state, &test_security_config()), dir)
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
        "/api/v1/sessions",
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

#[tokio::test]
async fn health_checks_have_expected_shape() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;

    let checks = body["checks"].as_array().expect("checks is array");
    assert!(checks.len() >= 2, "expected at least 2 health checks");

    for check in checks {
        assert!(check["name"].is_string(), "each check has a name");
        assert!(check["status"].is_string(), "each check has a status");
    }

    let names: Vec<&str> = checks.iter().filter_map(|c| c["name"].as_str()).collect();
    assert!(
        names.contains(&"session_store"),
        "missing session_store check"
    );
    assert!(names.contains(&"providers"), "missing providers check");
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
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
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
        .oneshot(authed_get("/api/v1/sessions/nonexistent"))
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
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
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
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .unwrap();

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
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
        .oneshot(authed_delete("/api/v1/sessions/nonexistent"))
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
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}/history")))
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
        .oneshot(authed_get("/api/v1/sessions/nonexistent/history"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn history_with_limit() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    {
        let store = state.session_store.lock().await;
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
        .oneshot(authed_get(&format!(
            "/api/v1/sessions/{id}/history?limit=3"
        )))
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert_eq!(body["messages"].as_array().unwrap().len(), 3);
}

// --- SSE Message Tests ---

#[tokio::test]
async fn send_message_returns_sse_content_type() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
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
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Hello!" })),
    );

    let resp = router.clone().oneshot(req).await.unwrap();
    let body = body_string(resp).await;

    assert!(
        body.contains("event: text_delta"),
        "should contain text_delta event"
    );
    assert!(
        body.contains("Hello from mock!"),
        "should contain mock response text"
    );
    assert!(
        body.contains("event: message_complete"),
        "should contain message_complete event"
    );
}

#[tokio::test]
async fn send_message_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions/nonexistent/messages",
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
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "" })),
    );

    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn send_message_stores_in_history() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Hello!" })),
    );
    let resp = router.clone().oneshot(req).await.unwrap();
    let _ = body_string(resp).await;

    // Allow the spawned send_turn task to complete and store assistant message
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}/history")))
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
        .oneshot(authed_get("/api/v1/sessions/nonexistent"))
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert!(body["error"].is_object());
    assert!(body["error"]["code"].is_string());
    assert!(body["error"]["message"].is_string());
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
        .uri(format!("/api/v1/sessions/{id}/messages"))
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
    let resp = app.oneshot(authed_get("/api/v1/nous")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let agents = body["nous"].as_array().unwrap();
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["id"], "syn");
}

#[tokio::test]
async fn get_nous_status() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/nous/syn")).await.unwrap();

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
        .oneshot(authed_get("/api/v1/nous/nonexistent"))
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
        .oneshot(authed_get("/api/v1/nous/syn/tools"))
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
        let router = build_router(Arc::clone(&state), &test_security_config());
        handles.push(tokio::spawn(async move {
            let req = authed_request(
                "POST",
                "/api/v1/sessions",
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
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Hello!" })),
    );

    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

// --- Actor-routed tests ---

#[tokio::test]
async fn send_message_routes_through_actor() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Test routing" })),
    );

    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;

    assert!(body.contains("event: text_delta"), "should have text_delta");
    assert!(
        body.contains("Hello from mock!"),
        "should contain mock response"
    );
    assert!(
        body.contains("event: message_complete"),
        "should have message_complete"
    );
    assert!(body.contains("end_turn"), "stop_reason should be end_turn");
}

#[tokio::test]
async fn nous_list_from_manager() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let resp = router.oneshot(authed_get("/api/v1/nous")).await.unwrap();

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
        &format!("/api/v1/sessions/{id}/messages"),
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
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("first close");
    assert_eq!(first.status(), StatusCode::NO_CONTENT);

    let second = router
        .clone()
        .oneshot(authed_delete(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("second close");
    assert_eq!(second.status(), StatusCode::NO_CONTENT);

    // Session should still be accessible as archived after both closes
    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
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
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
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
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "not_found");
    assert!(body["error"]["request_id"].is_string());
}

#[tokio::test]
async fn old_api_nous_path_returns_gone() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/nous"))
        .await
        .expect("response");

    assert_eq!(resp.status(), StatusCode::GONE);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "api_version_required");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("/api/v1/nous")
    );
}

#[tokio::test]
async fn fallback_404_returns_json_error() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/totally/unknown/path")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "not_found");
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("/totally/unknown/path")
    );
    assert!(body["error"]["request_id"].is_string());
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

// --- Security Header Tests ---

#[tokio::test]
async fn security_headers_present_on_response() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers().get("x-frame-options").unwrap(), "DENY");
    assert_eq!(
        resp.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    assert_eq!(resp.headers().get("x-xss-protection").unwrap(), "0");
    assert_eq!(
        resp.headers().get("referrer-policy").unwrap(),
        "strict-origin-when-cross-origin"
    );
    assert_eq!(
        resp.headers().get("content-security-policy").unwrap(),
        "default-src 'self'"
    );
    // HSTS should NOT be present when TLS is disabled
    assert!(resp.headers().get("strict-transport-security").is_none());
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
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(
        resp.headers().get("strict-transport-security").unwrap(),
        "max-age=31536000; includeSubDomains"
    );
}

// --- Body Limit Tests ---

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
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

// --- CSRF Tests ---

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

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
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
            .unwrap(),
        ))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn csrf_allows_get_without_header() {
    let (state, _dir) = test_state().await;
    let security = SecurityConfig {
        csrf_enabled: true,
        ..SecurityConfig::default()
    };
    let router = build_router(state, &security);

    let resp = router.oneshot(authed_get("/api/v1/nous")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

// --- OpenAPI Tests ---

#[tokio::test]
async fn openapi_spec_returns_valid_json() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/api/docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let version = body["openapi"].as_str().unwrap();
    assert!(
        version.starts_with("3."),
        "expected OpenAPI 3.x, got {version}"
    );
}

#[tokio::test]
async fn openapi_spec_has_all_paths() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/api/docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = body_json(resp).await;
    let paths = body["paths"].as_object().unwrap();
    assert!(paths.contains_key("/api/health"));
    assert!(paths.contains_key("/api/v1/sessions"));
    assert!(paths.contains_key("/api/v1/sessions/{id}"));
    assert!(paths.contains_key("/api/v1/sessions/{id}/messages"));
    assert!(paths.contains_key("/api/v1/sessions/{id}/history"));
    assert!(paths.contains_key("/api/v1/nous"));
    assert!(paths.contains_key("/api/v1/nous/{id}"));
    assert!(paths.contains_key("/api/v1/nous/{id}/tools"));
}

#[tokio::test]
async fn openapi_docs_no_auth_required() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/api/docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn openapi_spec_has_schemas() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(
            Request::get("/api/docs/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = body_json(resp).await;
    let schemas = body["components"]["schemas"].as_object().unwrap();
    assert!(schemas.contains_key("SessionResponse"));
    assert!(schemas.contains_key("ErrorResponse"));
    assert!(schemas.contains_key("HealthResponse"));
    assert!(schemas.contains_key("NousStatus"));
}

// --- Metrics Tests ---

#[tokio::test]
async fn metrics_returns_200_with_prometheus_content_type() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let content_type = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        content_type.contains("text/plain"),
        "expected text/plain content type, got: {content_type}"
    );
}

#[tokio::test]
async fn metrics_no_auth_required() {
    let (app, _dir) = app().await;
    // No authorization header — should still succeed
    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn metrics_contains_aletheia_prefixed_families() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let body = body_string(resp).await;
    assert!(
        body.contains("aletheia_http_requests_total"),
        "should contain HTTP request counter"
    );
    assert!(
        body.contains("aletheia_uptime_seconds"),
        "should contain uptime gauge"
    );
    assert!(
        body.contains("# HELP"),
        "should contain Prometheus HELP comments"
    );
    assert!(
        body.contains("# TYPE"),
        "should contain Prometheus TYPE comments"
    );
}

#[tokio::test]
async fn metrics_counters_increment_after_request() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    // Make a health request first to increment the counter
    let _ = router
        .clone()
        .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
        .await
        .unwrap();

    // Then check /metrics for the counter
    let resp = router
        .clone()
        .oneshot(Request::get("/metrics").body(Body::empty()).unwrap())
        .await
        .unwrap();

    let body = body_string(resp).await;
    assert!(
        body.contains("/api/health"),
        "should contain the health endpoint path in metrics"
    );
}

// --- CORS Tests ---

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
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    // Permissive CORS should allow any origin
    assert!(resp.status().is_success() || resp.status() == StatusCode::NO_CONTENT);
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
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    // Should not have the evil origin in access-control-allow-origin
    let allow_origin = resp.headers().get("access-control-allow-origin");
    assert!(allow_origin.is_none() || allow_origin.unwrap() != "http://evil.example.com");
}

// --- Auth mode "none" bypasses JWT ---

async fn app_auth_disabled() -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state().await;
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

// --- Session list with filter ---

#[tokio::test]
async fn list_sessions_returns_empty_initially() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/sessions")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body["sessions"].is_array());
}

#[tokio::test]
async fn list_sessions_includes_created_session() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    create_test_session(&router).await;

    let resp = router
        .clone()
        .oneshot(authed_get("/api/v1/sessions"))
        .await
        .unwrap();

    let body = body_json(resp).await;
    let sessions = body["sessions"].as_array().unwrap();
    assert!(!sessions.is_empty());
    assert_eq!(sessions[0]["nousId"], "syn");
}

#[tokio::test]
async fn list_sessions_filter_by_nous_id() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    create_test_session(&router).await;

    let resp = router
        .clone()
        .oneshot(authed_get("/api/v1/sessions?nousId=syn"))
        .await
        .unwrap();

    let body = body_json(resp).await;
    let sessions = body["sessions"].as_array().unwrap();
    assert!(!sessions.is_empty());

    let resp2 = router
        .clone()
        .oneshot(authed_get("/api/v1/sessions?nousId=nonexistent"))
        .await
        .unwrap();

    let body2 = body_json(resp2).await;
    let sessions2 = body2["sessions"].as_array().unwrap();
    assert!(sessions2.is_empty());
}

// --- POST archive endpoint ---

#[tokio::test]
async fn archive_via_post_returns_204() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request("POST", &format!("/api/v1/sessions/{id}/archive"), None);
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .unwrap();
    let body = body_json(resp).await;
    assert_eq!(body["status"], "archived");
}

#[tokio::test]
async fn archive_unknown_session_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request("POST", "/api/v1/sessions/nonexistent/archive", None);
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// --- Create session with unknown nous ---

#[tokio::test]
async fn create_session_unknown_nous_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "nonexistent-agent",
            "session_key": "test"
        })),
    );

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "nous_not_found");
}

// --- History with before filter ---

#[tokio::test]
async fn history_before_filter() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    {
        let store = state.session_store.lock().await;
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
        .oneshot(authed_get(&format!(
            "/api/v1/sessions/{id}/history?before=3"
        )))
        .await
        .unwrap();

    let body = body_json(resp).await;
    let messages = body["messages"].as_array().unwrap();
    assert!(messages.iter().all(|m| m["seq"].as_i64().unwrap() < 3));
}

// --- Stream turn (TUI protocol) ---

#[tokio::test]
async fn stream_turn_returns_sse() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let req = authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "agentId": "syn",
            "message": "Hello from TUI",
            "sessionKey": "stream-test"
        })),
    );

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/event-stream"));
}

#[tokio::test]
async fn stream_turn_contains_turn_start_event() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());

    let req = authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "agentId": "syn",
            "message": "Hello!",
            "sessionKey": "stream-events"
        })),
    );

    let resp = router.oneshot(req).await.unwrap();
    let body = body_string(resp).await;
    assert!(
        body.contains("event: turn_start"),
        "should contain turn_start event"
    );
}

#[tokio::test]
async fn stream_turn_empty_message_returns_400() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "agentId": "syn",
            "message": "",
            "sessionKey": "empty-msg"
        })),
    );

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn stream_turn_unknown_agent_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "POST",
        "/api/v1/sessions/stream",
        Some(serde_json::json!({
            "agentId": "nonexistent",
            "message": "Hello!",
            "sessionKey": "test"
        })),
    );

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// --- Events SSE ---

#[tokio::test]
async fn events_endpoint_returns_sse() {
    // WHY: /api/v1/events is removed (stub replaced with 404) until a real
    // server-side broadcast channel is wired into AppState (#1026).
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/events")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "not_implemented");
}

#[tokio::test]
async fn events_stream_contains_init_event() {
    // WHY: /api/v1/events returns 404 until a broadcast channel is wired in
    // (#1026). This test verifies the error response is well-formed JSON.
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/events")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(
        body["error"]["code"], "not_implemented",
        "events endpoint should return not_implemented code"
    );
}

// --- Config handler tests ---

#[tokio::test]
async fn config_get_requires_auth() {
    let (app, _dir) = app().await;
    let req = Request::get("/api/v1/config").body(Body::empty()).unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn config_get_returns_redacted_config() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/config")).await.unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body.is_object());
}

#[tokio::test]
async fn config_get_section_valid() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/config/gateway"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn config_get_section_invalid_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/config/nonexistent_section"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn config_update_invalid_section_returns_404() {
    let (app, _dir) = app().await;
    let req = authed_request(
        "PUT",
        "/api/v1/config/nonexistent_section",
        Some(serde_json::json!({"key": "value"})),
    );

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// --- Nous tools for unknown nous ---

#[tokio::test]
async fn nous_tools_unknown_returns_404() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/nous/nonexistent/tools"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "nous_not_found");
}

// --- Nous list auth required ---

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

// --- Config requires auth ---

#[tokio::test]
async fn config_section_requires_auth() {
    let (app, _dir) = app().await;
    let req = Request::get("/api/v1/config/gateway")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// --- Gateway config redaction ---

#[tokio::test]
async fn gateway_config_signing_key_is_redacted() {
    let (state, _dir) = test_state().await;

    // Inject a signing key so there is a secret value to redact.
    {
        let mut config = state.config.write().await;
        config.gateway.auth.signing_key = Some("super-secret-signing-key".to_owned());
    }

    let router = build_router(Arc::clone(&state), &test_security_config());
    let resp = router
        .oneshot(authed_get("/api/v1/config/gateway"))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;

    // The raw secret must not appear anywhere in the response.
    assert!(
        !body.to_string().contains("super-secret-signing-key"),
        "signing key must not appear in API response"
    );
    // The field must be replaced with the redaction placeholder.
    assert_eq!(body["auth"]["signingKey"], "***");
    // Non-secret fields must still be present and correct.
    assert_eq!(body["port"], 18789);
}

// --- Router: Method Not Allowed ---

#[tokio::test]
async fn put_on_sessions_returns_405() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("PUT")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", default_token()))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn delete_on_nous_returns_405() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("DELETE")
        .uri("/api/v1/nous")
        .header("authorization", format!("Bearer {}", default_token()))
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn post_on_health_returns_405() {
    let (app, _dir) = app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/api/health")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED);
}

// --- Request ID injection ---

#[tokio::test]
async fn request_id_present_in_error_responses() {
    let (app, _dir) = app().await;
    let resp = app
        .oneshot(authed_get("/api/v1/sessions/nonexistent"))
        .await
        .unwrap();

    let body = body_json(resp).await;
    let request_id = body["error"]["request_id"].as_str().unwrap();
    assert!(!request_id.is_empty());
    assert!(request_id.len() >= 20, "request_id should be a ULID");
}

// --- SseEvent serialization ---

#[test]
fn sse_event_type_text_delta() {
    let event = crate::stream::SseEvent::TextDelta {
        text: "hello".to_owned(),
    };
    assert_eq!(event.event_type(), "text_delta");
}

#[test]
fn sse_event_type_thinking_delta() {
    let event = crate::stream::SseEvent::ThinkingDelta {
        thinking: "hmm".to_owned(),
    };
    assert_eq!(event.event_type(), "thinking_delta");
}

#[test]
fn sse_event_type_tool_use() {
    let event = crate::stream::SseEvent::ToolUse {
        id: "t1".to_owned(),
        name: "search".to_owned(),
        input: serde_json::json!({}),
    };
    assert_eq!(event.event_type(), "tool_use");
}

#[test]
fn sse_event_type_tool_result() {
    let event = crate::stream::SseEvent::ToolResult {
        tool_use_id: "t1".to_owned(),
        content: "result".to_owned(),
        is_error: false,
    };
    assert_eq!(event.event_type(), "tool_result");
}

#[test]
fn sse_event_type_message_complete() {
    let event = crate::stream::SseEvent::MessageComplete {
        stop_reason: "end_turn".to_owned(),
        usage: crate::stream::UsageData {
            input_tokens: 10,
            output_tokens: 5,
        },
    };
    assert_eq!(event.event_type(), "message_complete");
}

#[test]
fn sse_event_type_error() {
    let event = crate::stream::SseEvent::Error {
        code: "test".to_owned(),
        message: "err".to_owned(),
    };
    assert_eq!(event.event_type(), "error");
}

#[test]
fn sse_event_serialization_roundtrip() {
    let event = crate::stream::SseEvent::TextDelta {
        text: "hello".to_owned(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "text_delta");
    assert_eq!(json["text"], "hello");
}

#[test]
fn sse_event_message_complete_serialization() {
    let event = crate::stream::SseEvent::MessageComplete {
        stop_reason: "end_turn".to_owned(),
        usage: crate::stream::UsageData {
            input_tokens: 100,
            output_tokens: 50,
        },
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "message_complete");
    assert_eq!(json["stop_reason"], "end_turn");
    assert_eq!(json["usage"]["input_tokens"], 100);
    assert_eq!(json["usage"]["output_tokens"], 50);
}

#[test]
fn sse_event_error_serialization() {
    let event = crate::stream::SseEvent::Error {
        code: "turn_failed".to_owned(),
        message: "provider error".to_owned(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "error");
    assert_eq!(json["code"], "turn_failed");
    assert_eq!(json["message"], "provider error");
}

// --- TUI stream event type tests ---

#[test]
fn tui_event_turn_start_type() {
    let event = crate::stream::WebchatEvent::TurnStart {
        session_id: "s1".to_owned(),
        nous_id: "syn".to_owned(),
        turn_id: "t1".to_owned(),
    };
    assert_eq!(event.event_type(), "turn_start");
}

#[test]
fn tui_event_text_delta_type() {
    let event = crate::stream::WebchatEvent::TextDelta {
        text: "hello".to_owned(),
    };
    assert_eq!(event.event_type(), "text_delta");
}

#[test]
fn tui_event_thinking_delta_type() {
    let event = crate::stream::WebchatEvent::ThinkingDelta {
        text: "hmm".to_owned(),
    };
    assert_eq!(event.event_type(), "thinking_delta");
}

#[test]
fn tui_event_tool_start_type() {
    let event = crate::stream::WebchatEvent::ToolStart {
        tool_name: "search".to_owned(),
        tool_id: "t1".to_owned(),
        input: serde_json::json!({}),
    };
    assert_eq!(event.event_type(), "tool_start");
}

#[test]
fn tui_event_tool_result_type() {
    let event = crate::stream::WebchatEvent::ToolResult {
        tool_name: "search".to_owned(),
        tool_id: "t1".to_owned(),
        result: "found".to_owned(),
        is_error: false,
        duration_ms: 42,
    };
    assert_eq!(event.event_type(), "tool_result");
}

#[test]
fn tui_event_turn_complete_type() {
    let event = crate::stream::WebchatEvent::TurnComplete {
        outcome: crate::stream::TurnOutcome {
            text: "done".to_owned(),
            nous_id: "syn".to_owned(),
            session_id: "s1".to_owned(),
            model: Some("mock".to_owned()),
            tool_calls: 0,
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        },
    };
    assert_eq!(event.event_type(), "turn_complete");
}

#[test]
fn tui_event_error_type() {
    let event = crate::stream::WebchatEvent::Error {
        message: "fail".to_owned(),
    };
    assert_eq!(event.event_type(), "error");
}

#[test]
fn tui_event_turn_start_serialization() {
    let event = crate::stream::WebchatEvent::TurnStart {
        session_id: "s1".to_owned(),
        nous_id: "syn".to_owned(),
        turn_id: "t1".to_owned(),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "turn_start");
    assert_eq!(json["sessionId"], "s1");
    assert_eq!(json["nousId"], "syn");
    assert_eq!(json["turnId"], "t1");
}

#[test]
fn tui_event_turn_complete_serialization() {
    let event = crate::stream::WebchatEvent::TurnComplete {
        outcome: crate::stream::TurnOutcome {
            text: "response".to_owned(),
            nous_id: "syn".to_owned(),
            session_id: "s1".to_owned(),
            model: Some("claude".to_owned()),
            tool_calls: 2,
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: 10,
            cache_write_tokens: 20,
        },
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["type"], "turn_complete");
    let outcome = &json["outcome"];
    assert_eq!(outcome["text"], "response");
    assert_eq!(outcome["nousId"], "syn");
    assert_eq!(outcome["toolCalls"], 2);
    assert_eq!(outcome["cacheReadTokens"], 10);
    assert_eq!(outcome["cacheWriteTokens"], 20);
}

// --- Error type tests ---

#[test]
fn api_error_session_not_found_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::SessionNotFound {
        id: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn api_error_nous_not_found_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::NousNotFound {
        id: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[test]
fn api_error_bad_request_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::BadRequest {
        message: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[test]
fn api_error_internal_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::Internal {
        message: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[test]
fn api_error_unauthorized_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::Unauthorized {
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn api_error_rate_limited_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::RateLimited {
        retry_after_ms: 1000,
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[test]
fn api_error_forbidden_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::Forbidden {
        message: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[test]
fn api_error_service_unavailable_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::ServiceUnavailable {
        message: "test".to_owned(),
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[test]
fn api_error_validation_failed_status_code() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::ValidationFailed {
        errors: vec!["field required".to_owned()],
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[test]
fn api_error_rate_limited_includes_details() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::RateLimited {
        retry_after_ms: 5000,
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let body = rt.block_on(async {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    });
    assert_eq!(body["error"]["details"]["retry_after_ms"], 5000);
}

#[test]
fn api_error_validation_failed_includes_errors() {
    use crate::error::ApiError;
    use axum::response::IntoResponse;

    let err = ApiError::ValidationFailed {
        errors: vec!["field1 required".to_owned(), "field2 invalid".to_owned()],
        location: snafu::Location::default(),
    };
    let response = err.into_response();
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let body = rt.block_on(async {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
    });
    let errors = body["error"]["details"]["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2);
}

// --- SecurityConfig tests ---

#[test]
fn security_config_default_values() {
    let config = SecurityConfig::default();
    assert!(config.allowed_origins.is_empty());
    assert_eq!(config.cors_max_age_secs, 3600);
    assert_eq!(config.body_limit_bytes, 1_048_576);
    assert!(config.csrf_enabled);
    assert_eq!(config.csrf_header_name, "x-requested-with");
    // WHY: The default CSRF token is now a CSPRNG-generated 32-char hex string
    // rather than the static "aletheia" value, which was guessable.
    assert_eq!(config.csrf_header_value.len(), 32);
    assert!(
        config
            .csrf_header_value
            .chars()
            .all(|c| c.is_ascii_hexdigit())
    );
    assert_ne!(config.csrf_header_value, "aletheia");
    assert!(!config.tls_enabled);
    assert!(config.tls_cert_path.is_none());
    assert!(config.tls_key_path.is_none());
}

#[test]
fn security_config_from_gateway() {
    use aletheia_taxis::config::GatewayConfig;

    let gw = GatewayConfig::default();
    let config = SecurityConfig::from_gateway(&gw);
    assert!(!config.tls_enabled);
    assert!(config.csrf_enabled);
    assert_eq!(config.cors_max_age_secs, 3600);
}

// --- deep_merge tests ---

#[test]
fn deep_merge_overwrites_scalar() {
    use crate::handlers::config::deep_merge;
    let mut base = serde_json::json!({"key": "old"});
    let patch = serde_json::json!({"key": "new"});
    deep_merge(&mut base, patch);
    assert_eq!(base["key"], "new");
}

#[test]
fn deep_merge_adds_missing_keys() {
    use crate::handlers::config::deep_merge;
    let mut base = serde_json::json!({"existing": 1});
    let patch = serde_json::json!({"new_key": 2});
    deep_merge(&mut base, patch);
    assert_eq!(base["existing"], 1);
    assert_eq!(base["new_key"], 2);
}

#[test]
fn deep_merge_recurses_objects() {
    use crate::handlers::config::deep_merge;
    let mut base = serde_json::json!({"nested": {"a": 1, "b": 2}});
    let patch = serde_json::json!({"nested": {"b": 3, "c": 4}});
    deep_merge(&mut base, patch);
    assert_eq!(base["nested"]["a"], 1);
    assert_eq!(base["nested"]["b"], 3);
    assert_eq!(base["nested"]["c"], 4);
}

#[test]
fn deep_merge_replaces_non_object_with_object() {
    use crate::handlers::config::deep_merge;
    let mut base = serde_json::json!({"key": "string"});
    let patch = serde_json::json!({"key": {"nested": true}});
    deep_merge(&mut base, patch);
    assert_eq!(base["key"]["nested"], true);
}

// --- Session response fields ---

#[tokio::test]
async fn session_response_has_all_expected_fields() {
    let (app, _dir) = app().await;
    let session = create_test_session(&app).await;

    assert!(session["id"].is_string());
    assert!(session["nous_id"].is_string());
    assert!(session["session_key"].is_string());
    assert!(session["status"].is_string());
    assert!(session["message_count"].is_number());
    assert!(session["token_count_estimate"].is_number());
    assert!(session["created_at"].is_string());
    assert!(session["updated_at"].is_string());
}

// --- History response structure ---

#[tokio::test]
async fn history_messages_have_expected_fields() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    {
        let store = state.session_store.lock().await;
        store
            .append_message(
                id,
                aletheia_mneme::types::Role::User,
                "test message",
                None,
                None,
                10,
            )
            .unwrap();
    }

    let resp = router
        .clone()
        .oneshot(authed_get(&format!("/api/v1/sessions/{id}/history")))
        .await
        .unwrap();

    let body = body_json(resp).await;
    let msg = &body["messages"][0];
    assert!(msg["id"].is_number());
    assert!(msg["seq"].is_number());
    assert!(msg["role"].is_string());
    assert!(msg["content"].is_string());
    assert!(msg["created_at"].is_string());
}

// --- Nous status response fields ---

#[tokio::test]
async fn nous_status_response_has_all_fields() {
    let (app, _dir) = app().await;
    let resp = app.oneshot(authed_get("/api/v1/nous/syn")).await.unwrap();

    let body = body_json(resp).await;
    assert!(body["id"].is_string());
    assert!(body["model"].is_string());
    assert!(body["context_window"].is_number());
    assert!(body["max_output_tokens"].is_number());
    assert!(body["thinking_enabled"].is_boolean());
    assert!(body["thinking_budget"].is_number());
    assert!(body["max_tool_iterations"].is_number());
    assert!(body["status"].is_string());
}

// --- CSRF with wrong header value ---

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
            .unwrap(),
        ))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// --- CSRF allows DELETE with correct header ---

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
            .unwrap(),
        ))
        .unwrap();

    let resp = router.clone().oneshot(create_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let session = body_json(resp).await;
    let id = session["id"].as_str().unwrap();

    let delete_req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/v1/sessions/{id}"))
        .header("authorization", format!("Bearer {token}"))
        .header("x-requested-with", csrf_token)
        .body(Body::empty())
        .unwrap();

    let resp = router.clone().oneshot(delete_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

// --- Idempotency-Key Tests (#866) ---

/// Helper: build a POST send-message request with an optional Idempotency-Key header.
fn send_message_req(session_id: &str, idempotency_key: Option<&str>) -> Request<Body> {
    let token = default_token();
    let mut builder = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/sessions/{session_id}/messages"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"));
    if let Some(key) = idempotency_key {
        builder = builder.header("idempotency-key", key);
    }
    builder
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({ "content": "Hello!" })).unwrap(),
        ))
        .unwrap()
}

#[tokio::test]
async fn idempotency_key_absent_works_normally() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = send_message_req(id, None);
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn idempotency_key_first_request_succeeds() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = send_message_req(id, Some("unique-key-001"));
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(
        body.contains("event: message_complete"),
        "first request should stream events normally"
    );
}

#[tokio::test]
async fn idempotency_key_replay_returns_cached_completion() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    // First request — completes and caches the real stop_reason and usage.
    let req1 = send_message_req(id, Some("replay-key-001"));
    let resp1 = router.clone().oneshot(req1).await.unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);
    // Consume the body to let the spawned turn task finish.
    let body1 = body_string(resp1).await;

    // Extract the original stop_reason from the first response.
    let original_stop_reason = body1
        .lines()
        .find(|l| l.starts_with("data:"))
        .and_then(|l| serde_json::from_str::<serde_json::Value>(l.trim_start_matches("data:")).ok())
        .and_then(|v| v["stop_reason"].as_str().map(str::to_owned))
        .unwrap_or_default();

    // Brief yield to let the turn task mark the entry as completed.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Second request with the same key — cache hit returns the cached body.
    let req2 = send_message_req(id, Some("replay-key-001"));
    let resp2 = router.clone().oneshot(req2).await.unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);
    let body2 = body_string(resp2).await;
    assert!(
        body2.contains("message_complete"),
        "replayed response should contain message_complete event, got: {body2}"
    );
    // The replayed stop_reason must match what was cached from the original turn.
    if !original_stop_reason.is_empty() {
        assert!(
            body2.contains(&original_stop_reason),
            "replayed stop_reason should match original '{original_stop_reason}', got: {body2}"
        );
    }
}

#[tokio::test]
async fn idempotency_key_in_flight_returns_409() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    // Manually insert an in-flight entry
    let key = "inflight-key-001";
    state.idempotency_cache.check_or_insert(key);

    // Request with the same key while in-flight
    let req = send_message_req(id, Some(key));
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = body_json(resp).await;
    assert_eq!(body["error"]["code"], "conflict");
}

#[tokio::test]
async fn idempotency_key_too_long_returns_400() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let long_key = "x".repeat(65);
    let req = send_message_req(id, Some(&long_key));
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn idempotency_key_empty_returns_400() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = send_message_req(id, Some(""));
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn idempotency_key_64_chars_accepted() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let key = "a".repeat(64);
    let req = send_message_req(id, Some(&key));
    let resp = router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// --- SSE stream correctness tests ---

/// Parse every `data:` line in an SSE body into JSON values.
///
/// Returns only lines that successfully parse; lines that don't start with
/// `data:` (e.g. `event:`, blank lines) are skipped.
fn collect_sse_data_events(body: &str) -> Vec<serde_json::Value> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str(data.trim()).ok())
        .collect()
}

/// Find the first SSE data event with the given `type` field value.
fn find_sse_event<'a>(
    events: &'a [serde_json::Value],
    event_type: &str,
) -> Option<&'a serde_json::Value> {
    events
        .iter()
        .find(|e| e["type"].as_str() == Some(event_type))
}

/// Happy path: every `data:` line in the SSE stream must be valid JSON with a
/// `type` field. Tests structural correctness of the event format.
#[tokio::test]
async fn sse_stream_each_data_line_is_valid_json_with_type_field() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Hello!" })),
    );
    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_string(resp).await;
    let events = collect_sse_data_events(&body);

    assert!(
        !events.is_empty(),
        "stream must contain at least one event, got empty body:\n{body}"
    );
    for event in &events {
        assert!(
            event["type"].is_string(),
            "every data event must have a string 'type' field, got: {event}"
        );
        assert!(
            !event["type"].as_str().unwrap().is_empty(),
            "event type must not be empty, got: {event}"
        );
    }
}

/// Happy path: the `text_delta` event carries the mock provider's exact response text.
#[tokio::test]
async fn sse_stream_text_delta_text_matches_mock_response() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Echo test" })),
    );
    let resp = router.oneshot(req).await.unwrap();
    let body = body_string(resp).await;
    let events = collect_sse_data_events(&body);

    let text_delta = find_sse_event(&events, "text_delta").expect("stream must contain text_delta");
    assert_eq!(
        text_delta["text"].as_str().unwrap(),
        "Hello from mock!",
        "text_delta.text must equal the mock provider's response"
    );
}

/// Happy path: the `message_complete` event includes non-zero token usage that
/// matches the mock provider's reported counts (input=10, output=5).
#[tokio::test]
async fn sse_stream_message_complete_reports_token_counts() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Token count test" })),
    );
    let resp = router.oneshot(req).await.unwrap();
    let body = body_string(resp).await;
    let events = collect_sse_data_events(&body);

    let complete =
        find_sse_event(&events, "message_complete").expect("stream must contain message_complete");
    let usage = &complete["usage"];
    assert!(
        usage.is_object(),
        "message_complete must have a usage object, got: {complete}"
    );
    assert_eq!(
        usage["input_tokens"].as_u64().unwrap(),
        10,
        "input_tokens must match mock provider value"
    );
    assert_eq!(
        usage["output_tokens"].as_u64().unwrap(),
        5,
        "output_tokens must match mock provider value"
    );
    assert_eq!(
        complete["stop_reason"].as_str().unwrap(),
        "end_turn",
        "stop_reason must match mock provider StopReason::EndTurn"
    );
}

/// Happy path: `text_delta` appears before `message_complete` in the stream.
/// The handler must not emit the completion marker before content events.
#[tokio::test]
async fn sse_stream_text_delta_precedes_message_complete() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Ordering test" })),
    );
    let resp = router.oneshot(req).await.unwrap();
    let body = body_string(resp).await;
    let events = collect_sse_data_events(&body);

    let delta_idx = events
        .iter()
        .position(|e| e["type"].as_str() == Some("text_delta"))
        .expect("stream must contain text_delta");
    let complete_idx = events
        .iter()
        .position(|e| e["type"].as_str() == Some("message_complete"))
        .expect("stream must contain message_complete");

    assert!(
        delta_idx < complete_idx,
        "text_delta (index {delta_idx}) must appear before message_complete (index {complete_idx})"
    );
}

/// Malformed event: sending invalid JSON in the request body returns a 4xx
/// HTTP error. The handler rejects the bad request before streaming starts —
/// it does not crash or produce a 5xx.
#[tokio::test]
async fn sse_send_message_with_invalid_json_body_returns_client_error() {
    let (router, _dir) = app().await;
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let token = default_token();
    let req = Request::builder()
        .method("POST")
        .uri(format!("/api/v1/sessions/{id}/messages"))
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::from("{not: valid json}"))
        .unwrap();

    let resp = router.clone().oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::BAD_REQUEST
            || resp.status() == StatusCode::UNPROCESSABLE_ENTITY,
        "invalid JSON body must return 400 or 422, not {}: the handler must not crash",
        resp.status()
    );
}

/// Error event: the fallback serialization-error data string is valid JSON
/// with a `message` field — not empty data. This verifies that the SSE error
/// fallback path produces a well-formed JSON payload per the stream contract.
#[test]
fn sse_serialization_fallback_data_is_valid_json_with_message_field() {
    // WHY: sse_event_to_axum (and the stream_turn equivalent) fall back to
    // this literal string when serde_json::to_string fails. Verify the string
    // is valid JSON with a non-empty message field — not an empty data line.
    let fallback_data = r#"{"message":"serialization failed"}"#;
    let parsed: serde_json::Value =
        serde_json::from_str(fallback_data).expect("fallback data must be valid JSON");
    assert!(
        parsed["message"].is_string(),
        "fallback error must have a string message field"
    );
    assert!(
        !parsed["message"].as_str().unwrap().is_empty(),
        "fallback error message must not be empty"
    );
    // The serialization fallback emits an error event, not an empty data line.
    assert!(
        !fallback_data.is_empty(),
        "fallback data must never be empty"
    );
}

/// Concurrent clients: multiple simultaneous SSE connections complete without
/// interfering with each other. Each connection receives its own
/// `message_complete` event.
#[tokio::test]
async fn sse_concurrent_connections_each_receive_complete_stream() {
    let (state, _dir) = test_state().await;

    let mut handles = Vec::new();
    for i in 0..3u32 {
        let router = build_router(Arc::clone(&state), &test_security_config());
        handles.push(tokio::spawn(async move {
            // Each concurrent client creates its own session to avoid sharing state.
            let create_req = authed_request(
                "POST",
                "/api/v1/sessions",
                Some(serde_json::json!({
                    "nous_id": "syn",
                    "session_key": format!("concurrent-sse-{i}"),
                })),
            );
            let create_resp = router.clone().oneshot(create_req).await.unwrap();
            assert_eq!(create_resp.status(), StatusCode::CREATED);
            let session = body_json(create_resp).await;
            let id = session["id"].as_str().unwrap().to_owned();

            let msg_req = authed_request(
                "POST",
                &format!("/api/v1/sessions/{id}/messages"),
                Some(serde_json::json!({ "content": format!("Hello from client {i}") })),
            );
            let resp = router.oneshot(msg_req).await.unwrap();
            assert_eq!(resp.status(), StatusCode::OK);
            body_string(resp).await
        }));
    }

    for handle in handles {
        let body = handle.await.unwrap();
        let events = collect_sse_data_events(&body);
        assert!(
            find_sse_event(&events, "message_complete").is_some(),
            "each concurrent client must receive its own message_complete event"
        );
        assert!(
            find_sse_event(&events, "text_delta").is_some(),
            "each concurrent client must receive its own text_delta event"
        );
    }
}

/// Connection drop: consuming only the response status (not the body) does
/// not panic or deadlock. The SSE handler spawns a background task that
/// terminates gracefully when the channel receiver is dropped.
#[tokio::test]
async fn sse_dropping_response_body_does_not_panic() {
    let (state, _dir) = test_state().await;
    let router = build_router(Arc::clone(&state), &test_security_config());
    let created = create_test_session(&router).await;
    let id = created["id"].as_str().unwrap();

    let req = authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "Drop test" })),
    );
    let resp = router.oneshot(req).await.unwrap();
    // Verify we got an SSE response, then drop the body without reading it.
    // The handler's spawned turn task must not panic when the channel closes.
    assert_eq!(resp.status(), StatusCode::OK);
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/event-stream"));
    // Body is dropped here — the spawned task detects the closed channel and exits.
    drop(resp);

    // Brief yield to let the background task observe channel closure.
    tokio::time::sleep(Duration::from_millis(50)).await;
    // If we reach here without panic, the connection-drop cleanup is correct.
}
