//! Cross-crate integration tests: full message pipeline end-to-end.
//!
//! Validates the wiring between crates: HTTP → pylon → nous actor → pipeline
//! stages → LLM mock → tool execution → session persistence → SSE response.
//!
//! Each test uses the `TestHarness` pattern from `end_to_end.rs`, extended with
//! multi-response mock providers for tool-use round trips and recall injection.

#![expect(clippy::expect_used, reason = "test assertions")]
#![cfg(feature = "sqlite-tests")]
#![expect(
    clippy::indexing_slicing,
    reason = "integration tests: index-based assertions on known-length slices"
)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use aletheia_hermeneus::provider::{LlmProvider, ProviderRegistry};
use aletheia_hermeneus::test_utils::MockProvider;
use aletheia_hermeneus::types::*;
use aletheia_koina::secret::SecretString;
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

        let default_config = aletheia_taxis::config::AletheiaConfig::default();
        let (config_tx, _config_rx) = tokio::sync::watch::channel(default_config.clone());
        let state = Arc::new(AppState {
            session_store,
            nous_manager: Arc::new(nous_manager),
            provider_registry,
            tool_registry,
            oikos,
            jwt_manager: Arc::clone(&jwt_manager),
            start_time: Instant::now(),
            auth_mode: "token".to_owned(),
            config: Arc::new(tokio::sync::RwLock::new(default_config)),
            config_tx,
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

#[path = "cross_crate_pipeline/auth_errors.rs"]
mod auth_errors;
#[path = "cross_crate_pipeline/concurrent.rs"]
mod concurrent;
#[path = "cross_crate_pipeline/lifecycle.rs"]
mod lifecycle;
#[path = "cross_crate_pipeline/wiring.rs"]
mod wiring;
