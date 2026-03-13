//! End-to-end integration tests: HTTP → pipeline → provider → persistence.

#![expect(clippy::expect_used, reason = "test assertions")]
#![cfg(feature = "sqlite-tests")]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Mutex as TokioMutex;

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
use aletheia_pylon::router::build_router;
use aletheia_pylon::state::AppState;
use aletheia_symbolon::jwt::{JwtConfig, JwtManager};
use aletheia_symbolon::types::Role;
use aletheia_taxis::oikos::Oikos;
use tokio_util::sync::CancellationToken;

// --- Mock Providers ---

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
        &["mock-model"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "mock"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct CapturingMockProvider {
    response: CompletionResponse,
    captured: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl CapturingMockProvider {
    fn new(captured: Arc<Mutex<Vec<CompletionRequest>>>) -> Self {
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
            captured,
        }
    }
}

impl LlmProvider for CapturingMockProvider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = aletheia_hermeneus::error::Result<CompletionResponse>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async {
            #[expect(
                clippy::expect_used,
                reason = "test mock: poisoned lock means a test bug"
            )]
            self.captured
                .lock()
                .expect("lock poisoned")
                .push(request.clone());
            Ok(self.response.clone())
        })
    }

    fn supported_models(&self) -> &[&str] {
        &["mock-model"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "mock-capturing"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// --- Test Harness ---

struct TestHarness {
    state: Arc<AppState>,
    jwt_manager: Arc<JwtManager>,
    _tmp: tempfile::TempDir,
}

impl TestHarness {
    async fn build() -> Self {
        Self::build_with_provider(Box::new(MockProvider::new())).await
    }

    async fn build_capturing() -> (Self, Arc<Mutex<Vec<CompletionRequest>>>) {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let provider = CapturingMockProvider::new(Arc::clone(&captured));
        let harness = Self::build_with_provider(Box::new(provider)).await;
        (harness, captured)
    }

    async fn build_with_provider(provider: Box<dyn LlmProvider>) -> Self {
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let root = dir.path();

        std::fs::create_dir_all(root.join("nous/test-nous")).expect("mkdir nous/test-nous");
        std::fs::create_dir_all(root.join("shared")).expect("mkdir shared");
        std::fs::create_dir_all(root.join("theke")).expect("mkdir theke");
        std::fs::write(root.join("nous/test-nous/SOUL.md"), "You are a test agent.")
            .expect("write SOUL.md");
        std::fs::write(root.join("theke/USER.md"), "Test user.").expect("write USER.md");

        let oikos = Arc::new(Oikos::from_root(root));
        let store = SessionStore::open_in_memory().expect("in-memory store");

        let mut provider_registry = ProviderRegistry::new();
        provider_registry.register(provider);
        let provider_registry = Arc::new(provider_registry);
        let tool_registry = Arc::new(ToolRegistry::new());

        let session_store = Arc::new(TokioMutex::new(store));

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
            id: "test-nous".to_owned(),
            model: "mock-model".to_owned(),
            ..NousConfig::default()
        };
        nous_manager
            .spawn(nous_config, PipelineConfig::default())
            .await;

        let jwt_manager = Arc::new(JwtManager::new(JwtConfig {
            signing_key: SecretString::from("test-secret-key-for-jwt".to_owned()),
            access_ttl: Duration::from_secs(3600),
            refresh_ttl: Duration::from_secs(86400),
            issuer: "aletheia-test".to_owned(),
        }));

        let state = Arc::new(AppState {
            session_store,
            nous_manager: Arc::new(nous_manager),
            provider_registry,
            tool_registry,
            oikos,
            jwt_manager: Arc::clone(&jwt_manager),
            start_time: Instant::now(),
            auth_mode: "token".to_owned(),
            config: Arc::new(tokio::sync::RwLock::new(
                aletheia_taxis::config::AletheiaConfig::default(),
            )),
            idempotency_cache: Arc::new(aletheia_pylon::idempotency::IdempotencyCache::new()),
            shutdown: CancellationToken::new(),
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
        });

        Self {
            state,
            jwt_manager,
            _tmp: dir,
        }
    }

    fn auth_token(&self) -> String {
        self.jwt_manager
            .issue_access("test-user", Role::Operator, None)
            .expect("test token")
    }

    fn router(&self) -> axum::Router {
        build_router(
            Arc::clone(&self.state),
            &aletheia_pylon::security::SecurityConfig {
                csrf_enabled: false,
                ..aletheia_pylon::security::SecurityConfig::default()
            },
        )
    }

    fn authed_request(
        &self,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> Request<Body> {
        let token = self.auth_token();
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"));

        match body {
            Some(b) => builder
                .body(Body::from(serde_json::to_vec(&b).expect("serialize")))
                .expect("request"),
            None => builder.body(Body::empty()).expect("request"),
        }
    }

    fn authed_get(&self, uri: &str) -> Request<Body> {
        let token = self.auth_token();
        Request::get(uri)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .expect("request")
    }

    async fn create_session(&self, router: &axum::Router) -> serde_json::Value {
        let req = self.authed_request(
            "POST",
            "/api/v1/sessions",
            Some(serde_json::json!({
                "nous_id": "test-nous",
                "session_key": "e2e-test"
            })),
        );
        let resp = router.clone().oneshot(req).await.expect("oneshot");
        assert_eq!(resp.status(), StatusCode::CREATED);
        body_json(resp).await
    }
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("parse json")
}

async fn body_string(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    String::from_utf8(bytes.to_vec()).expect("utf8")
}

// --- Tests ---

#[tokio::test]
async fn http_create_session_send_message_get_history() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "hello" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send message");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_string(resp).await;
    assert!(
        body.contains("event: text_delta"),
        "SSE stream should contain text_delta event"
    );
    assert!(
        body.contains("Hello from mock!"),
        "SSE stream should contain mock response text"
    );
    assert!(
        body.contains("event: message_complete"),
        "SSE stream should contain message_complete event"
    );

    let resp = router
        .clone()
        .oneshot(harness.authed_get(&format!("/api/v1/sessions/{id}/history")))
        .await
        .expect("get history");
    assert_eq!(resp.status(), StatusCode::OK);

    let history = body_json(resp).await;
    let messages = history["messages"].as_array().expect("messages array");
    assert!(
        messages.len() >= 2,
        "history should contain user + assistant messages, got {}",
        messages.len()
    );
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "hello");
}

#[tokio::test]
async fn session_persists_across_turns() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "first" })),
    );
    let resp = router.clone().oneshot(req).await.expect("first turn");
    let _ = body_string(resp).await;

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "second" })),
    );
    let resp = router.clone().oneshot(req).await.expect("second turn");
    let _ = body_string(resp).await;

    let resp = router
        .clone()
        .oneshot(harness.authed_get(&format!("/api/v1/sessions/{id}/history")))
        .await
        .expect("get history");

    let history = body_json(resp).await;
    let messages = history["messages"].as_array().expect("messages array");
    assert!(
        messages.len() >= 4,
        "should have at least 4 messages (2 user + 2 assistant), got {}",
        messages.len()
    );

    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "first");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[2]["role"], "user");
    assert_eq!(messages[2]["content"], "second");
    assert_eq!(messages[3]["role"], "assistant");
}

#[tokio::test]
async fn nous_status_reflects_configuration() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/nous"))
        .await
        .expect("list nous");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let agents = body["nous"].as_array().expect("nous array");
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0]["id"], "test-nous");

    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/nous/test-nous"))
        .await
        .expect("get status");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["id"], "test-nous");
    assert!(body["context_window"].is_number());
    assert!(body["max_output_tokens"].is_number());

    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/nous/nonexistent"))
        .await
        .expect("get nonexistent");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bootstrap_assembles_from_oikos() {
    let (harness, captured) = TestHarness::build_capturing().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "trigger bootstrap" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send message");
    let _ = body_string(resp).await;

    #[expect(
        clippy::expect_used,
        reason = "test assertion: poisoned lock means a test bug"
    )]
    let requests = captured.lock().expect("lock poisoned");
    assert!(
        !requests.is_empty(),
        "mock provider should have received at least one request"
    );

    let system_prompt = requests[0]
        .system
        .as_ref()
        .expect("system prompt should be present");
    assert!(
        system_prompt.contains("You are a test agent"),
        "system prompt should contain SOUL.md content, got: {system_prompt}"
    );
    assert!(
        system_prompt.contains("Test user"),
        "system prompt should contain USER.md content, got: {system_prompt}"
    );
}

#[tokio::test]
async fn unknown_session_returns_404() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/sessions/nonexistent"))
        .await
        .expect("get unknown session");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let req = harness.authed_request(
        "POST",
        "/api/v1/sessions/nonexistent/messages",
        Some(serde_json::json!({ "content": "hello" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send to unknown");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn health_returns_status() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let resp = router
        .oneshot(
            Request::get("/api/health")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("health check");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    assert!(
        body["status"].is_string(),
        "health response should have status field"
    );
    assert_eq!(body["status"], "healthy");
}

#[tokio::test]
async fn unauthenticated_request_rejected() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/sessions")
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "nous_id": "test-nous",
                "session_key": "unauthed"
            }))
            .expect("serialize"),
        ))
        .expect("request");

    let resp = router.oneshot(req).await.expect("unauthenticated request");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn close_session_archives_it() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let token = harness.auth_token();
    let req = Request::delete(format!("/api/v1/sessions/{id}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    let resp = router.clone().oneshot(req).await.expect("close session");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    let resp = router
        .clone()
        .oneshot(harness.authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("get archived session");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "archived");
}

#[tokio::test]
async fn send_message_stores_both_roles_in_history() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "test message" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send message");
    let _ = body_string(resp).await;

    let resp = router
        .clone()
        .oneshot(harness.authed_get(&format!("/api/v1/sessions/{id}/history")))
        .await
        .expect("get history");
    let history = body_json(resp).await;
    let messages = history["messages"].as_array().expect("messages array");

    assert_eq!(messages.len(), 2, "should have exactly user + assistant");
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "test message");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"], "Hello from mock!");
}
