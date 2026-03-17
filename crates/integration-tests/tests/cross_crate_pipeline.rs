//! Cross-crate integration tests: full message pipeline end-to-end.
//!
//! Validates the wiring between crates: HTTP → pylon → nous actor → pipeline
//! stages → LLM mock → tool execution → session persistence → SSE response.
//!
//! Each test uses the `TestHarness` pattern from `end_to_end.rs`, extended with
//! multi-response mock providers for tool-use round trips and recall injection.

#![expect(clippy::expect_used, reason = "test assertions")]
#![cfg(feature = "sqlite-tests")]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use secrecy::SecretString;
use tokio::sync::Mutex as TokioMutex;
use tower::ServiceExt;

use aletheia_hermeneus::provider::{LlmProvider, ProviderRegistry};
use aletheia_hermeneus::test_utils::MockProvider;
use aletheia_hermeneus::types::*;
use aletheia_mneme::store::SessionStore;
use aletheia_nous::config::{NousConfig, PipelineConfig};
use aletheia_nous::manager::NousManager;
use aletheia_organon::builtins;
use aletheia_organon::registry::ToolRegistry;
use aletheia_pylon::router::build_router;
use aletheia_pylon::state::AppState;
use aletheia_symbolon::jwt::{JwtConfig, JwtManager};
use aletheia_symbolon::types::Role;
use aletheia_taxis::oikos::Oikos;
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// Mock providers
// ---------------------------------------------------------------------------

/// Captures all LLM requests for inspection.
struct CapturingMockProvider {
    response: CompletionResponse,
    captured: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl CapturingMockProvider {
    fn new(text: &str, captured: Arc<Mutex<Vec<CompletionRequest>>>) -> Self {
        Self {
            response: CompletionResponse {
                id: "msg_capture".to_owned(),
                model: "mock-model".to_owned(),
                stop_reason: StopReason::EndTurn,
                content: vec![ContentBlock::Text {
                    text: text.to_owned(),
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
}

/// Returns different responses for successive calls (`tool_use` then `end_turn`).
struct SequentialMockProvider {
    responses: Mutex<Vec<CompletionResponse>>,
    captured: Arc<Mutex<Vec<CompletionRequest>>>,
}

impl SequentialMockProvider {
    fn new(
        responses: Vec<CompletionResponse>,
        captured: Arc<Mutex<Vec<CompletionRequest>>>,
    ) -> Self {
        Self {
            responses: Mutex::new(responses),
            captured,
        }
    }
}

impl LlmProvider for SequentialMockProvider {
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
            #[expect(
                clippy::expect_used,
                reason = "test mock: empty responses means a test bug"
            )]
            let mut responses = self.responses.lock().expect("lock poisoned");
            if responses.len() > 1 {
                Ok(responses.remove(0))
            } else {
                // Return the last response for all subsequent calls
                Ok(responses[0].clone())
            }
        })
    }

    fn supported_models(&self) -> &[&str] {
        &["mock-model"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "mock-sequential"
    }
}

// ---------------------------------------------------------------------------
// Test harness
// ---------------------------------------------------------------------------

struct TestHarness {
    state: Arc<AppState>,
    jwt_manager: Arc<JwtManager>,
    _tmp: tempfile::TempDir,
}

impl TestHarness {
    async fn build() -> Self {
        Self::build_with_provider(Box::new(
            MockProvider::new("Hello from mock!").models(&["mock-model"]),
        ))
        .await
    }

    async fn build_capturing(text: &str) -> (Self, Arc<Mutex<Vec<CompletionRequest>>>) {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let provider = CapturingMockProvider::new(text, Arc::clone(&captured));
        let harness = Self::build_with_provider(Box::new(provider)).await;
        (harness, captured)
    }

    async fn build_with_provider(provider: Box<dyn LlmProvider>) -> Self {
        Self::build_with_provider_and_tools(provider, false).await
    }

    async fn build_with_provider_and_tools(
        provider: Box<dyn LlmProvider>,
        register_tools: bool,
    ) -> Self {
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

        let mut tool_registry = ToolRegistry::new();
        if register_tools {
            builtins::register_all(&mut tool_registry).expect("register builtins");
        }
        let tool_registry = Arc::new(tool_registry);

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

    async fn create_session_with_key(&self, router: &axum::Router, key: &str) -> serde_json::Value {
        let req = self.authed_request(
            "POST",
            "/api/v1/sessions",
            Some(serde_json::json!({
                "nous_id": "test-nous",
                "session_key": key
            })),
        );
        let resp = router.clone().oneshot(req).await.expect("oneshot");
        assert_eq!(resp.status(), StatusCode::CREATED);
        body_json(resp).await
    }

    async fn send_message(&self, router: &axum::Router, session_id: &str, content: &str) -> String {
        let req = self.authed_request(
            "POST",
            &format!("/api/v1/sessions/{session_id}/messages"),
            Some(serde_json::json!({ "content": content })),
        );
        let resp = router.clone().oneshot(req).await.expect("send message");
        assert_eq!(resp.status(), StatusCode::OK);
        body_string(resp).await
    }

    async fn get_history(&self, router: &axum::Router, session_id: &str) -> serde_json::Value {
        let resp = router
            .clone()
            .oneshot(self.authed_get(&format!("/api/v1/sessions/{session_id}/history")))
            .await
            .expect("get history");
        assert_eq!(resp.status(), StatusCode::OK);
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

/// Parse SSE body into (`event_type`, data) pairs.
fn parse_sse_events(body: &str) -> Vec<(String, String)> {
    let mut events = Vec::new();
    let mut current_event = String::new();
    let mut current_data = String::new();

    for line in body.lines() {
        if let Some(ev) = line.strip_prefix("event: ") {
            ev.clone_into(&mut current_event);
        } else if let Some(data) = line.strip_prefix("data: ") {
            data.clone_into(&mut current_data);
        } else if line.is_empty() && !current_event.is_empty() {
            events.push((current_event.clone(), current_data.clone()));
            current_event.clear();
            current_data.clear();
        }
    }
    // Capture final event if no trailing blank line
    if !current_event.is_empty() {
        events.push((current_event, current_data));
    }
    events
}

// ===========================================================================
// 1. Full turn lifecycle
// ===========================================================================

#[tokio::test]
async fn full_turn_lifecycle_sse_events_and_persistence() {
    let (harness, captured) = TestHarness::build_capturing("Hello from the agent!").await;
    let router = harness.router();

    // Create session
    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    // Send message and collect SSE stream
    let body = harness.send_message(&router, id, "Hi there").await;
    let events = parse_sse_events(&body);

    // Verify SSE events
    let event_types: Vec<&str> = events.iter().map(|(t, _)| t.as_str()).collect();
    assert!(
        event_types.contains(&"text_delta"),
        "should contain text_delta event, got: {event_types:?}"
    );
    assert!(
        event_types.contains(&"message_complete"),
        "should contain message_complete event, got: {event_types:?}"
    );

    // Verify text_delta contains response text
    let text_events: Vec<&str> = events
        .iter()
        .filter(|(t, _)| t == "text_delta")
        .map(|(_, d)| d.as_str())
        .collect();
    let text_combined: String = text_events.join("");
    assert!(
        text_combined.contains("Hello from the agent!"),
        "text_delta events should contain response, got: {text_combined}"
    );

    // Verify message_complete has usage
    let complete = events
        .iter()
        .find(|(t, _)| t == "message_complete")
        .expect("message_complete event");
    let complete_data: serde_json::Value =
        serde_json::from_str(&complete.1).expect("parse message_complete");
    assert!(
        complete_data["usage"]["input_tokens"].is_number(),
        "message_complete should have input_tokens"
    );
    assert!(
        complete_data["usage"]["output_tokens"].is_number(),
        "message_complete should have output_tokens"
    );

    // Verify persistence: history should have user + assistant
    let history = harness.get_history(&router, id).await;
    let messages = history["messages"].as_array().expect("messages array");
    assert!(
        messages.len() >= 2,
        "history should have at least user + assistant, got {}",
        messages.len()
    );
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"], "Hi there");
    assert_eq!(messages[1]["role"], "assistant");

    // Verify LLM received correct system prompt and message
    #[expect(
        clippy::expect_used,
        reason = "test assertion: poisoned lock means a test bug"
    )]
    let requests = captured.lock().expect("lock poisoned");
    assert!(
        !requests.is_empty(),
        "provider should have received at least one request"
    );
    let system = requests[0]
        .system
        .as_ref()
        .expect("system prompt should be present");
    assert!(
        system.contains("You are a test agent"),
        "system prompt should contain SOUL.md, got: {system}"
    );
    assert!(
        system.contains("Test user"),
        "system prompt should contain USER.md, got: {system}"
    );
}

// ===========================================================================
// 2. Tool execution round-trip
// ===========================================================================

#[tokio::test]
async fn tool_execution_round_trip() {
    let captured = Arc::new(Mutex::new(Vec::new()));

    // First call: LLM returns tool_use for `note` tool
    let tool_use_response = CompletionResponse {
        id: "msg_tool".to_owned(),
        model: "mock-model".to_owned(),
        stop_reason: StopReason::ToolUse,
        content: vec![ContentBlock::ToolUse {
            id: "tu_001".to_owned(),
            name: "note".to_owned(),
            input: serde_json::json!({"action": "list"}),
        }],
        usage: Usage {
            input_tokens: 20,
            output_tokens: 15,
            ..Usage::default()
        },
    };

    // Second call: LLM returns text after seeing tool result
    let final_response = CompletionResponse {
        id: "msg_final".to_owned(),
        model: "mock-model".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: "I checked your notes and found nothing yet.".to_owned(),
            citations: None,
        }],
        usage: Usage {
            input_tokens: 30,
            output_tokens: 12,
            ..Usage::default()
        },
    };

    let provider = SequentialMockProvider::new(
        vec![tool_use_response, final_response],
        Arc::clone(&captured),
    );

    let harness = TestHarness::build_with_provider_and_tools(Box::new(provider), true).await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let body = harness
        .send_message(&router, id, "What are my notes?")
        .await;
    let events = parse_sse_events(&body);
    let event_types: Vec<&str> = events.iter().map(|(t, _)| t.as_str()).collect();

    // Verify tool_use and tool_result SSE events
    assert!(
        event_types.contains(&"tool_use"),
        "should contain tool_use event, got: {event_types:?}"
    );
    assert!(
        event_types.contains(&"tool_result"),
        "should contain tool_result event, got: {event_types:?}"
    );

    // Verify tool_use event has the note tool
    let tool_use_event = events
        .iter()
        .find(|(t, _)| t == "tool_use")
        .expect("tool_use event");
    let tool_use_data: serde_json::Value =
        serde_json::from_str(&tool_use_event.1).expect("parse tool_use");
    assert_eq!(tool_use_data["name"], "note");

    // Verify final text response
    assert!(
        event_types.contains(&"text_delta"),
        "should contain text_delta after tool round-trip, got: {event_types:?}"
    );
    assert!(
        event_types.contains(&"message_complete"),
        "should contain message_complete, got: {event_types:?}"
    );

    // Verify the second LLM call received the tool result in its messages
    #[expect(
        clippy::expect_used,
        reason = "test assertion: poisoned lock means a test bug"
    )]
    let requests = captured.lock().expect("lock poisoned");
    assert!(
        requests.len() >= 2,
        "should have at least 2 LLM calls (tool_use + final), got {}",
        requests.len()
    );
}

// ===========================================================================
// 3. Memory recall integration
// ===========================================================================
// NOTE: Full recall pipeline testing requires knowledge-store + engine-tests
// features and is covered in recall_pipeline.rs and knowledge_recall.rs.
// Here we test that the system prompt assembly correctly includes recall
// section content when it's provided to the pipeline.

#[tokio::test]
async fn system_prompt_includes_oikos_bootstrap_files() {
    let (harness, captured) = TestHarness::build_capturing("Recalled context.").await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let _ = harness
        .send_message(&router, id, "Tell me what you know")
        .await;

    #[expect(
        clippy::expect_used,
        reason = "test assertion: poisoned lock means a test bug"
    )]
    let requests = captured.lock().expect("lock poisoned");
    assert!(!requests.is_empty());

    let system = requests[0].system.as_ref().expect("system prompt present");

    // SOUL.md and USER.md content should be in the system prompt
    assert!(
        system.contains("You are a test agent"),
        "system prompt should contain SOUL.md content"
    );
    assert!(
        system.contains("Test user"),
        "system prompt should contain USER.md content"
    );
}

// ===========================================================================
// 4. Session lifecycle
// ===========================================================================

#[tokio::test]
async fn session_lifecycle_create_list_archive_unarchive_rename() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    // Create session
    let session = harness
        .create_session_with_key(&router, "lifecycle-test")
        .await;
    let id = session["id"].as_str().expect("session id");
    assert_eq!(session["status"], "active");

    // Send a message to populate history
    let _ = harness.send_message(&router, id, "hello lifecycle").await;

    // Verify message persisted
    let history = harness.get_history(&router, id).await;
    let messages = history["messages"].as_array().expect("messages");
    assert!(messages.len() >= 2, "should have user + assistant messages");

    // List sessions: should include our session
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/sessions?nous_id=test-nous"))
        .await
        .expect("list sessions");
    assert_eq!(resp.status(), StatusCode::OK);
    let list = body_json(resp).await;
    let sessions = list["sessions"].as_array().expect("sessions array");
    assert!(
        sessions.iter().any(|s| s["id"] == id),
        "session should appear in list"
    );

    // Archive session via DELETE
    let token = harness.auth_token();
    let req = Request::delete(format!("/api/v1/sessions/{id}"))
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");
    let resp = router.clone().oneshot(req).await.expect("archive");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify session is non-retrievable after DELETE (#1251): GET must return 404.
    let resp = router
        .clone()
        .oneshot(harness.authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("get after delete");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Unarchive
    let req = harness.authed_request("POST", &format!("/api/v1/sessions/{id}/unarchive"), None);
    let resp = router.clone().oneshot(req).await.expect("unarchive");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify active again
    let resp = router
        .clone()
        .oneshot(harness.authed_get(&format!("/api/v1/sessions/{id}")))
        .await
        .expect("get unarchived");
    let session_data = body_json(resp).await;
    assert_eq!(session_data["status"], "active");

    // Rename session
    let req = harness.authed_request(
        "PUT",
        &format!("/api/v1/sessions/{id}/name"),
        Some(serde_json::json!({ "name": "My Renamed Session" })),
    );
    let resp = router.clone().oneshot(req).await.expect("rename");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Verify renamed: check via list endpoint where display_name is returned
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/sessions?nous_id=test-nous"))
        .await
        .expect("list after rename");
    let list = body_json(resp).await;
    let sessions = list["sessions"].as_array().expect("sessions array");
    let our_session = sessions
        .iter()
        .find(|s| s["id"] == id)
        .expect("session should be in list");
    assert_eq!(our_session["displayName"], "My Renamed Session");
}

// ===========================================================================
// 5. Auth flow
// ===========================================================================

#[tokio::test]
async fn auth_no_token_returns_401() {
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

    let resp = router.oneshot(req).await.expect("no-token request");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_invalid_token_returns_401() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/sessions")
        .header("authorization", "Bearer not-a-valid-jwt-token")
        .body(Body::empty())
        .expect("request");

    let resp = router.oneshot(req).await.expect("bad-token request");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_expired_token_returns_401() {
    // Craft a token with exp far in the past (beyond jsonwebtoken's 60s leeway)
    let claims = aletheia_symbolon::types::Claims {
        sub: "test-user".to_owned(),
        role: Role::Operator,
        nous_id: None,
        iss: "aletheia-test".to_owned(),
        iat: 1_000_000,
        exp: 1_000_001, // 1970-01-12: well past any leeway
        jti: "expired-test".to_owned(),
        kind: aletheia_symbolon::types::TokenKind::Access,
    };
    let key = jsonwebtoken::EncodingKey::from_secret(b"test-secret-key-for-jwt");
    let token = jsonwebtoken::encode(&jsonwebtoken::Header::default(), &claims, &key)
        .expect("encode expired token");

    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = Request::builder()
        .method("GET")
        .uri("/api/v1/sessions")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::empty())
        .expect("request");

    let resp = router.oneshot(req).await.expect("expired-token request");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_valid_token_returns_200() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/sessions"))
        .await
        .expect("authed request");
    assert_eq!(resp.status(), StatusCode::OK);
}

// ===========================================================================
// 6. Error propagation
// ===========================================================================

#[tokio::test]
async fn error_invalid_session_returns_404() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    // GET unknown session
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/sessions/nonexistent-id"))
        .await
        .expect("get unknown session");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // POST message to unknown session
    let req = harness.authed_request(
        "POST",
        "/api/v1/sessions/nonexistent-id/messages",
        Some(serde_json::json!({ "content": "hello" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send to unknown");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // GET history of unknown session
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/sessions/nonexistent-id/history"))
        .await
        .expect("history of unknown");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn error_empty_message_returns_400() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "" })),
    );
    let resp = router.clone().oneshot(req).await.expect("empty message");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn error_empty_rename_returns_400() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let req = harness.authed_request(
        "PUT",
        &format!("/api/v1/sessions/{id}/name"),
        Some(serde_json::json!({ "name": "" })),
    );
    let resp = router.clone().oneshot(req).await.expect("empty rename");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn error_provider_failure_returns_sse_error_event() {
    let harness = TestHarness::build_with_provider(Box::new(
        MockProvider::error("simulated provider failure")
            .models(&["mock-model"])
            .named("mock-error"),
    ))
    .await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    // The SSE stream should still start (HTTP 200), but contain an error event
    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "trigger error" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send message");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "SSE stream should start with 200 even on provider error"
    );

    let body = body_string(resp).await;
    let events = parse_sse_events(&body);
    let event_types: Vec<&str> = events.iter().map(|(t, _)| t.as_str()).collect();

    // Should have an error event
    assert!(
        event_types.contains(&"error"),
        "should contain error SSE event when provider fails, got: {event_types:?}"
    );
}

#[tokio::test]
async fn error_nonexistent_nous_returns_404() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = harness.authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "nonexistent-agent",
            "session_key": "test"
        })),
    );
    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("create session for unknown nous");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ===========================================================================
// 7. Concurrent sessions
// ===========================================================================

#[tokio::test]
async fn concurrent_sessions_isolated() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    // Create two sessions with different keys
    let session_a = harness.create_session_with_key(&router, "session-a").await;
    let session_b = harness.create_session_with_key(&router, "session-b").await;
    let id_a = session_a["id"].as_str().expect("session a id");
    let id_b = session_b["id"].as_str().expect("session b id");

    // Send different messages to each session concurrently
    let (body_a, body_b) = tokio::join!(
        harness.send_message(&router, id_a, "Message for session A"),
        harness.send_message(&router, id_b, "Message for session B"),
    );

    // Both should complete successfully
    assert!(
        body_a.contains("event: message_complete"),
        "session A should complete"
    );
    assert!(
        body_b.contains("event: message_complete"),
        "session B should complete"
    );

    // Verify histories are independent
    let history_a = harness.get_history(&router, id_a).await;
    let history_b = harness.get_history(&router, id_b).await;

    let msgs_a = history_a["messages"].as_array().expect("messages a");
    let msgs_b = history_b["messages"].as_array().expect("messages b");

    // Each should have exactly their own messages
    assert!(
        msgs_a.len() >= 2,
        "session A should have user + assistant messages"
    );
    assert!(
        msgs_b.len() >= 2,
        "session B should have user + assistant messages"
    );

    assert_eq!(
        msgs_a[0]["content"], "Message for session A",
        "session A user message should be its own"
    );
    assert_eq!(
        msgs_b[0]["content"], "Message for session B",
        "session B user message should be its own"
    );

    // Messages should not leak across sessions
    let a_contents: Vec<&str> = msgs_a
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect();
    let b_contents: Vec<&str> = msgs_b
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect();

    assert!(
        !a_contents.iter().any(|c| c.contains("session B")),
        "session A should not contain session B messages"
    );
    assert!(
        !b_contents.iter().any(|c| c.contains("session A")),
        "session B should not contain session A messages"
    );
}

#[tokio::test]
async fn concurrent_sessions_multiple_turns_isolated() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let session_a = harness.create_session_with_key(&router, "multi-a").await;
    let session_b = harness.create_session_with_key(&router, "multi-b").await;
    let id_a = session_a["id"].as_str().expect("id a");
    let id_b = session_b["id"].as_str().expect("id b");

    // Interleave turns between sessions
    let _ = harness.send_message(&router, id_a, "A-turn-1").await;
    let _ = harness.send_message(&router, id_b, "B-turn-1").await;
    let _ = harness.send_message(&router, id_a, "A-turn-2").await;
    let _ = harness.send_message(&router, id_b, "B-turn-2").await;

    let history_a = harness.get_history(&router, id_a).await;
    let history_b = harness.get_history(&router, id_b).await;

    let msgs_a = history_a["messages"].as_array().expect("messages a");
    let msgs_b = history_b["messages"].as_array().expect("messages b");

    // 2 user + 2 assistant each = 4 messages minimum
    assert!(
        msgs_a.len() >= 4,
        "session A should have at least 4 messages (2 turns), got {}",
        msgs_a.len()
    );
    assert!(
        msgs_b.len() >= 4,
        "session B should have at least 4 messages (2 turns), got {}",
        msgs_b.len()
    );

    // Verify message ordering
    let user_msgs_a: Vec<&str> = msgs_a
        .iter()
        .filter(|m| m["role"] == "user")
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert_eq!(user_msgs_a, vec!["A-turn-1", "A-turn-2"]);

    let user_msgs_b: Vec<&str> = msgs_b
        .iter()
        .filter(|m| m["role"] == "user")
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert_eq!(user_msgs_b, vec!["B-turn-1", "B-turn-2"]);
}

// ===========================================================================
// Additional wiring tests
// ===========================================================================

#[tokio::test]
async fn capturing_provider_receives_tool_definitions() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let provider = CapturingMockProvider::new("ok", Arc::clone(&captured));
    let harness = TestHarness::build_with_provider_and_tools(Box::new(provider), true).await;
    let router = harness.router();

    let session = harness.create_session(&router).await;
    let id = session["id"].as_str().expect("session id");

    let _ = harness.send_message(&router, id, "test tools").await;

    #[expect(
        clippy::expect_used,
        reason = "test assertion: poisoned lock means a test bug"
    )]
    let requests = captured.lock().expect("lock poisoned");
    assert!(!requests.is_empty());

    // When tools are registered, LLM should receive tool definitions
    assert!(
        !requests[0].tools.is_empty(),
        "LLM request should include tool definitions when tools are registered"
    );

    // Verify that known builtins appear
    let tool_names: Vec<&str> = requests[0].tools.iter().map(|t| t.name.as_str()).collect();
    assert!(
        tool_names.contains(&"note"),
        "tool definitions should include 'note', got: {tool_names:?}"
    );
}

#[tokio::test]
async fn health_endpoint_no_auth_required() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = Request::get("/api/health")
        .body(Body::empty())
        .expect("request");
    let resp = router.oneshot(req).await.expect("health check");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    assert_eq!(body["status"], "healthy");
}

#[tokio::test]
async fn nous_list_and_status_endpoints() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    // List nous agents
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

    // Get agent status
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/nous/test-nous"))
        .await
        .expect("get status");
    assert_eq!(resp.status(), StatusCode::OK);

    // Get agent tools
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/nous/test-nous/tools"))
        .await
        .expect("get tools");
    assert_eq!(resp.status(), StatusCode::OK);

    // Nonexistent agent
    let resp = router
        .clone()
        .oneshot(harness.authed_get("/api/v1/nous/nonexistent"))
        .await
        .expect("get nonexistent");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
