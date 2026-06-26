//! Shared `AppState` and HTTP helpers for integration tests.

#![expect(
    clippy::expect_used,
    reason = "integration test harnesses fail fast when fixture setup breaks"
)]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hermeneus::provider::{LlmProvider, ProviderRegistry};
use hermeneus::test_utils::MockProvider;
use koina::secret::SecretString;
use mneme::embedding::{EmbeddingProvider, MockEmbeddingProvider};
#[cfg(feature = "knowledge-store")]
use mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};
use mneme::store::SessionStore;
use nous::config::{NousConfig, PipelineConfig};
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use organon::types::ToolGroupPolicy;
use pylon::router::build_router;
use pylon::state::AppState;
use symbolon::auth::{AuthConfig, AuthFacade};
use symbolon::jwt::{JwtConfig, JwtManager};
use symbolon::types::Role;
use taxis::oikos::Oikos;
use tokio::net::TcpListener;
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

/// Nous id used by the shared integration-test fixture.
pub const TEST_NOUS_ID: &str = "test-nous";

/// Session key used by generic helper-created sessions.
pub const DEFAULT_SESSION_KEY: &str = "e2e-test";

/// Dimension used by the test embedding provider and knowledge store.
pub const TEST_EMBEDDING_DIM: usize = 384;

struct TestEmbeddingProvider {
    inner: MockEmbeddingProvider,
}

impl TestEmbeddingProvider {
    fn new(dim: usize) -> Self {
        Self {
            inner: MockEmbeddingProvider::new(dim),
        }
    }
}

impl EmbeddingProvider for TestEmbeddingProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>, mneme::embedding::EmbeddingError> {
        self.inner.embed(text)
    }

    fn embed_batch(
        &self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, mneme::embedding::EmbeddingError> {
        self.inner.embed_batch(texts)
    }

    fn dimension(&self) -> usize {
        self.inner.dimension()
    }

    fn model_name(&self) -> &'static str {
        "test-embedding"
    }
}

/// Shared integration-test harness around a pylon `AppState`.
pub struct TestHarness {
    /// Shared pylon application state.
    pub state: Arc<AppState>,
    /// JWT issuer used by auth helper methods.
    pub jwt_manager: Arc<JwtManager>,
    #[cfg(feature = "knowledge-store")]
    knowledge_store: Option<Arc<KnowledgeStore>>,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    _tmp: tempfile::TempDir,
}

impl TestHarness {
    /// Build a minimal `AppState` without a knowledge store.
    pub async fn build_minimal() -> Self {
        Self::build_with_provider(Box::new(
            MockProvider::new("Hello from mock!").models(&["mock-model"]),
        ))
        .await
    }

    /// Alias for the minimal harness, used by existing tests.
    pub async fn build() -> Self {
        Self::build_minimal().await
    }

    /// Build an `AppState` backed by a real tempfile fjall knowledge store.
    #[cfg(feature = "knowledge-store")]
    pub async fn build_with_knowledge_store() -> Self {
        Self::build_with_provider_and_knowledge_store(Box::new(
            MockProvider::new("Hello from mock!").models(&["mock-model"]),
        ))
        .await
    }

    /// Build a minimal `AppState` with a caller-supplied LLM provider.
    pub async fn build_with_provider(provider: Box<dyn LlmProvider>) -> Self {
        Self::build_internal(provider, false, false, None).await
    }

    /// Build a minimal `AppState` with optional built-in tool registration.
    pub async fn build_with_provider_and_tools(
        provider: Box<dyn LlmProvider>,
        register_tools: bool,
    ) -> Self {
        Self::build_internal(provider, register_tools, false, None).await
    }

    /// Build an `AppState` with a real knowledge store and caller-supplied provider.
    #[cfg(feature = "knowledge-store")]
    pub async fn build_with_provider_and_knowledge_store(provider: Box<dyn LlmProvider>) -> Self {
        Self::build_internal(provider, false, true, None).await
    }

    /// Build an `AppState` with a caller-supplied provider and a pre-built tool registry.
    ///
    /// Use this when the test needs to register custom or synthetic tools (e.g. an
    /// Irreversible tool for the approval-gate e2e path) without pulling in the full
    /// organon builtin suite.
    pub async fn build_with_provider_and_registry(
        provider: Box<dyn LlmProvider>,
        registry: ToolRegistry,
    ) -> Self {
        Self::build_internal(provider, false, false, Some(registry)).await
    }

    #[expect(
        clippy::too_many_lines,
        reason = "central AppState fixture keeps duplicated integration setup out of test files"
    )]
    async fn build_internal(
        provider: Box<dyn LlmProvider>,
        register_tools: bool,
        with_knowledge_store: bool,
        prebuilt_registry: Option<ToolRegistry>,
    ) -> Self {
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let root = dir.path();

        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        std::fs::create_dir_all(root.join("nous").join(TEST_NOUS_ID)).expect("mkdir nous");
        std::fs::create_dir_all(root.join("nous").join("workspace")).expect("mkdir workspace");
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        std::fs::create_dir_all(root.join("shared")).expect("mkdir shared");
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        std::fs::create_dir_all(root.join("theke")).expect("mkdir theke");
        #[expect(
            clippy::disallowed_methods,
            reason = "integration tests write fixture files to temp directories; synchronous I/O is required in setup"
        )]
        // kanon:ignore RUST/blocking-in-async — test fixture writes to temp directory; synchronous I/O required in setup
        std::fs::write(
            root.join("nous").join(TEST_NOUS_ID).join("SOUL.md"),
            "You are a test agent.",
        )
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        .expect("write SOUL.md");
        #[expect(
            clippy::disallowed_methods,
            reason = "integration tests write fixture files to temp directories; synchronous I/O is required in setup"
        )]
        // kanon:ignore RUST/blocking-in-async — test fixture writes to temp directory; synchronous I/O required in setup
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        std::fs::write(root.join("theke").join("USER.md"), "Test user.").expect("write USER.md");

        let oikos = Arc::new(Oikos::from_root(root));
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        let store = SessionStore::open_in_memory().expect("in-memory store");

        let mut provider_registry = ProviderRegistry::new();
        provider_registry.register(provider);
        let provider_registry = Arc::new(provider_registry);

        let mut tool_registry = prebuilt_registry.unwrap_or_default();
        if register_tools {
            // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
            organon::builtins::register_all(&mut tool_registry).expect("register builtins");
        }
        let tool_registry = Arc::new(tool_registry);
        let session_store = Arc::new(TokioMutex::new(store));

        #[cfg(feature = "knowledge-store")]
        let knowledge_store = if with_knowledge_store {
            Some(
                KnowledgeStore::open_fjall(
                    root.join("knowledge").join("shared"),
                    KnowledgeConfig {
                        dim: TEST_EMBEDDING_DIM,
                        ..KnowledgeConfig::default()
                    },
                )
                // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
                .expect("open tempfile fjall knowledge store"),
            )
        } else {
            None
        };
        #[cfg(not(feature = "knowledge-store"))]
        let knowledge_store: Option<()> = None;

        let embedding_provider: Option<Arc<dyn EmbeddingProvider>> =
            with_knowledge_store.then(|| -> Arc<dyn EmbeddingProvider> {
                Arc::new(TestEmbeddingProvider::new(TEST_EMBEDDING_DIM))
            });
        let workspace_root = pylon::state::resolve_workspace_root(&oikos, None);

        #[cfg(feature = "knowledge-store")]
        let knowledge_stores = knowledge_store
            .as_ref()
            .map(|store| HashMap::from([("shared".to_owned(), Arc::clone(store))]));

        let mut nous_manager = NousManager::new(
            Arc::clone(&provider_registry),
            Arc::clone(&tool_registry),
            Arc::clone(&oikos),
            embedding_provider.clone(),
            None,
            Some(Arc::clone(&session_store)),
            #[cfg(feature = "knowledge-store")]
            knowledge_stores,
            Arc::new(vec![]),
            None,
            None,
            taxis::config::NousBehaviorConfig::default(),
            taxis::config::ToolLimitsConfig::default(),
        );

        let nous_config = NousConfig {
            id: Arc::from(TEST_NOUS_ID),
            generation: nous::config::NousGenerationConfig {
                model: "mock-model".to_owned(),
                ..nous::config::NousGenerationConfig::default()
            },
            recall: nous::recall::RecallConfig {
                min_score: 0.0,
                ..nous::recall::RecallConfig::default()
            },
            tool_groups: ToolGroupPolicy::AllowAll {
                reason: "integration test harness".to_owned(),
            },
            ..NousConfig::default()
        };
        nous_manager
            .spawn(nous_config, PipelineConfig::default())
            .await
            // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
            .expect("spawn nous");

        let jwt_config = JwtConfig {
            signing_key: SecretString::from("test-secret-key-for-jwt".to_owned()),
            access_ttl: Duration::from_hours(1),
            refresh_ttl: Duration::from_hours(24),
            issuer: "aletheia-test".to_owned(),
            ..JwtConfig::default()
        };
        let jwt_manager = Arc::new(JwtManager::new(jwt_config.clone()));

        let default_config = taxis::config::AletheiaConfig::default();
        let (config_tx, _config_rx) = tokio::sync::watch::channel(default_config.clone());
        let metrics_registry = koina::metrics::MetricsRegistry::new();
        metrics_registry.with_registry(pylon::metrics::register);
        let credential_runtime =
            Arc::new(pylon::credential_runtime::CredentialRuntimeManager::new(
                Arc::clone(&provider_registry),
            ));
        let state = Arc::new(AppState {
            session_store,
            nous_manager: Arc::new(nous_manager),
            provider_registry,
            tool_registry,
            oikos,
            workspace_root,
            jwt_manager: Arc::clone(&jwt_manager),
            auth_facade: Arc::new(
                // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
                AuthFacade::in_memory(AuthConfig { jwt: jwt_config }).expect("auth facade"),
            ),
            credential_runtime,
            start_time: Instant::now(),
            auth_mode: "token".to_owned(),
            none_role: "admin".to_owned(),
            config: Arc::new(tokio::sync::RwLock::new(default_config)),
            config_tx,
            idempotency_cache: Arc::new(pylon::idempotency::IdempotencyCache::new()),
            shutdown: CancellationToken::new(),
            #[cfg(feature = "knowledge-store")]
            knowledge_store: knowledge_store.clone(),
            embedding_provider: embedding_provider.clone(),
            turn_buffer_registry: Arc::new(pylon::turn_buffer::TurnBufferRegistry::new()),
            metrics_registry,
            event_bus: Arc::new(pylon::event_bus::EventBus::new(256)),
            approval_registry: Arc::new(pylon::approval_registry::ApprovalRegistry::new()),
            loopback_only_metrics: false,
        });

        Self {
            state,
            jwt_manager,
            #[cfg(feature = "knowledge-store")]
            knowledge_store,
            embedding_provider,
            _tmp: dir,
        }
    }

    /// Return a clone of the configured knowledge store.
    ///
    /// # Panics
    ///
    /// Panics when called on a minimal harness.
    #[cfg(feature = "knowledge-store")]
    #[must_use]
    pub fn knowledge_store(&self) -> Arc<KnowledgeStore> {
        self.knowledge_store
            .as_ref()
            // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
            .expect("knowledge store harness")
            .clone()
    }

    /// Return a clone of the configured embedding provider.
    ///
    /// # Panics
    ///
    /// Panics when called on a minimal harness.
    #[must_use]
    pub fn embedding_provider(&self) -> Arc<dyn EmbeddingProvider> {
        self.embedding_provider
            .as_ref()
            // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
            .expect("knowledge store harness")
            .clone()
    }

    /// Issue a test operator bearer token.
    #[must_use]
    pub fn auth_token(&self) -> String {
        self.jwt_manager
            .issue_access("test-user", Role::Operator, None)
            // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
            .expect("test token")
    }

    /// Build a pylon router with CSRF disabled for integration tests.
    pub fn router(&self) -> axum::Router {
        let security = pylon::security::SecurityConfig {
            csrf: pylon::security::CsrfConfig {
                enabled: false,
                ..pylon::security::CsrfConfig::default()
            },
            ..pylon::security::SecurityConfig::default()
        };
        self.router_with_security(&security)
    }

    /// Build a pylon router using caller-supplied security config.
    pub fn router_with_security(&self, security: &pylon::security::SecurityConfig) -> axum::Router {
        build_router(Arc::clone(&self.state), security)
    }

    /// Build an authenticated JSON request.
    pub fn authed_request(
        &self,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> Request<Body> {
        let token = self.auth_token();
        self.authed_request_with_token(&token, method, uri, body)
    }

    /// Build an authenticated JSON request with a caller-supplied token.
    pub fn authed_request_with_token(
        &self,
        token: &str,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
    ) -> Request<Body> {
        let builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"));

        match body {
            Some(b) => builder
                // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
                .body(Body::from(serde_json::to_vec(&b).expect("serialize")))
                // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
                .expect("request"),
            // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
            None => builder.body(Body::empty()).expect("request"),
        }
    }

    /// Build an authenticated GET request.
    pub fn authed_get(&self, uri: &str) -> Request<Body> {
        let token = self.auth_token();
        Request::get(uri)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
            .expect("request")
    }

    /// Create a default test session through the HTTP API.
    pub async fn create_session(&self, router: &axum::Router) -> serde_json::Value {
        self.create_session_with_key(router, DEFAULT_SESSION_KEY)
            .await
    }

    /// Create a keyed test session through the HTTP API.
    pub async fn create_session_with_key(
        &self,
        router: &axum::Router,
        key: &str,
    ) -> serde_json::Value {
        use tower::ServiceExt;

        let req = self.authed_request(
            "POST",
            "/api/v1/sessions",
            Some(serde_json::json!({
                "nous_id": TEST_NOUS_ID,
                "session_key": key
            })),
        );
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        let resp = router.clone().oneshot(req).await.expect("oneshot");
        // kanon:ignore RUST/bare-assert — HTTP status assertion in test helper; failure context obvious from line
        assert_eq!(resp.status(), StatusCode::CREATED);
        body_json(resp).await
    }

    /// Send one message through the HTTP API and return the SSE body.
    pub async fn send_message(
        &self,
        router: &axum::Router,
        session_id: &str,
        content: &str,
    ) -> String {
        use tower::ServiceExt;

        let req = self.authed_request(
            "POST",
            &format!("/api/v1/sessions/{session_id}/messages"),
            Some(serde_json::json!({ "content": content })),
        );
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        let resp = router.clone().oneshot(req).await.expect("send message");
        // kanon:ignore RUST/bare-assert — HTTP status assertion in test helper; failure context obvious from line
        assert_eq!(resp.status(), StatusCode::OK);
        body_string(resp).await
    }

    /// Fetch a session history through the HTTP API.
    pub async fn get_history(&self, router: &axum::Router, session_id: &str) -> serde_json::Value {
        use tower::ServiceExt;

        let resp = router
            .clone()
            .oneshot(self.authed_get(&format!("/api/v1/sessions/{session_id}/history")))
            .await
            // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
            .expect("get history");
        // kanon:ignore RUST/bare-assert — HTTP status assertion in test helper; failure context obvious from line
        assert_eq!(resp.status(), StatusCode::OK);
        body_json(resp).await
    }

    /// Start a real TCP-bound pylon test server.
    pub async fn start_tcp_server(self) -> (String, String, Self) {
        let token = self.auth_token();
        let router = self.router();
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        let addr = listener.local_addr().expect("local_addr");
        let base_url = format!("http://{addr}"); // kanon:ignore SECURITY/insecure-transport

        tokio::spawn(
            async move {
                // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
                axum::serve(listener, router).await.expect("serve");
            }
            .instrument(tracing::info_span!("test_server")),
        );

        (base_url, token, self)
    }
}

/// Parse an Axum response body as JSON.
pub async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        .expect("read body");
    // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
    serde_json::from_slice(&bytes).expect("parse json")
}

/// Parse an Axum response body as UTF-8 text.
pub async fn body_string(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
        .expect("read body");
    // kanon:ignore RUST/expect — test asserts invariant; panic is the failure signal
    String::from_utf8(bytes.to_vec()).expect("utf8")
}
