//! Shared test fixtures for the split `public_api_*.rs` integration test binaries.
//!
//! Cargo treats `tests/common/mod.rs` as a non-binary helper module; each
//! `tests/public_api_*.rs` pulls it in with `mod common;`.
//!
//! WHY: extracted from the monolithic `tests/public_api.rs` (1079 lines) to
//! satisfy `RUST/file-too-long`.

#![expect(clippy::expect_used, reason = "test fixtures use expect")]
#![expect(
    clippy::disallowed_methods,
    reason = "integration test fixtures write files to temp directories; synchronous std::fs I/O is required in test setup"
)]
#![expect(
    dead_code,
    reason = "shared fixtures: not every split file uses every helper"
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use hermeneus::provider::ProviderRegistry;
use hermeneus::test_utils::MockProvider;
use koina::http::BEARER_PREFIX;
use koina::secret::SecretString;
use mneme::store::SessionStore;
use nous::config::{NousConfig, NousGenerationConfig, PipelineConfig};
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use pylon::idempotency::IdempotencyCache;
use pylon::security::{CsrfConfig, SecurityConfig};
use pylon::state::AppState;
use symbolon::auth::{AuthConfig, AuthFacade};
use symbolon::jwt::{JwtConfig, JwtManager};
use symbolon::types::Role;
use taxis::config::AletheiaConfig;
use taxis::oikos::Oikos;

/// Minimal oikos tempdir with the directories and config files the
/// health-check handlers expect to be readable.
pub struct TestEnv {
    pub state: Arc<AppState>,
    pub _tmp: tempfile::TempDir,
}

impl TestEnv {
    pub async fn new() -> Self {
        Self::builder().build().await
    }

    pub fn builder() -> TestEnvBuilder {
        TestEnvBuilder::default()
    }
}

#[derive(Default)]
pub struct TestEnvBuilder {
    with_actor: bool,
    auth_mode: Option<String>,
    jwt_access_ttl: Option<Duration>,
    #[cfg(feature = "knowledge-store")]
    knowledge_store: Option<Arc<mneme::knowledge_store::KnowledgeStore>>,
}

impl TestEnvBuilder {
    pub fn with_actor(mut self, flag: bool) -> Self {
        self.with_actor = flag;
        self
    }

    pub fn auth_mode(mut self, mode: &str) -> Self {
        self.auth_mode = Some(mode.to_owned());
        self
    }

    pub fn jwt_access_ttl(mut self, ttl: Duration) -> Self {
        self.jwt_access_ttl = Some(ttl);
        self
    }

    #[cfg(feature = "knowledge-store")]
    pub fn knowledge_store(mut self, store: Arc<mneme::knowledge_store::KnowledgeStore>) -> Self {
        self.knowledge_store = Some(store);
        self
    }

    pub async fn build(self) -> TestEnv {
        let tmp = tempfile::TempDir::new().expect("tmpdir");
        let root = tmp.path();

        // WHY: oikos layout is load-bearing for health checks: missing
        // directories cause fail-closed behaviour that hides real bugs.
        for dir in [
            "nous/syn",
            "nous/workspace",
            "shared",
            "theke",
            "data",
            "config",
            "config/credentials",
        ] {
            std::fs::create_dir_all(root.join(dir)).expect("mkdir oikos subdir");
        }

        std::fs::write(root.join("nous/syn/SOUL.md"), "I am Syn, a test agent.")
            .expect("write SOUL.md");
        std::fs::write(root.join("theke/USER.md"), "Test user.").expect("write USER.md");
        std::fs::write(
            root.join("config/aletheia.toml"),
            "[gateway]\nport = 18789\nbind = \"localhost\"\n",
        )
        .expect("write config");
        std::fs::write(
            root.join("config/credentials/anthropic.json"),
            r#"{"token":"sk-ant-test-key"}"#,
        )
        .expect("write credential");

        let oikos = Arc::new(Oikos::from_root(root));
        let session_store = Arc::new(Mutex::new(
            SessionStore::open_in_memory().expect("open in-memory store"),
        ));

        // WHY: every TestEnv registers a mock provider so health checks can
        // report at least one Up provider. Tests that want a clean registry
        // should construct AppState directly.
        let mut provider_registry = ProviderRegistry::new();
        provider_registry.register(Box::new(
            MockProvider::new("Hello from mock!").models(&["mock-model"]),
        ));
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
            taxis::config::NousBehaviorConfig::default(),
            taxis::config::ToolLimitsConfig::default(),
        );

        if self.with_actor {
            let nous_config = NousConfig {
                id: Arc::from("syn"),
                generation: NousGenerationConfig {
                    model: "mock-model".to_owned(),
                    ..Default::default()
                },
                ..NousConfig::default()
            };
            nous_manager
                .spawn(nous_config, PipelineConfig::default())
                .await
                .expect("spawn nous in test harness");
        }

        let (jwt_manager, auth_facade) = test_auth_state(self.jwt_access_ttl);
        let workspace_root = pylon::state::resolve_workspace_root(&oikos, None);

        let default_config = AletheiaConfig::default();
        let (config_tx, _config_rx) = tokio::sync::watch::channel(default_config.clone());
        let metrics_registry = koina::metrics::MetricsRegistry::new();
        metrics_registry.with_registry(pylon::metrics::register);
        let state = Arc::new(AppState {
            session_store,
            nous_manager: Arc::new(nous_manager),
            provider_registry,
            tool_registry,
            oikos,
            workspace_root,
            jwt_manager,
            auth_facade,
            start_time: Instant::now(),
            auth_mode: self.auth_mode.unwrap_or_else(|| "token".to_owned()),
            none_role: "admin".to_owned(),
            config: Arc::new(tokio::sync::RwLock::new(default_config)),
            config_tx,
            idempotency_cache: Arc::new(IdempotencyCache::new()),
            shutdown: CancellationToken::new(),
            #[cfg(feature = "knowledge-store")]
            knowledge_store: self.knowledge_store,
            embedding_provider: None,
            turn_buffer_registry: Arc::new(pylon::turn_buffer::TurnBufferRegistry::new()),
            metrics_registry,
            event_bus: Arc::new(pylon::event_bus::EventBus::new(256)),
            approval_registry: Arc::new(pylon::approval_registry::ApprovalRegistry::new()),
            loopback_only_metrics: false,
        });

        TestEnv { state, _tmp: tmp }
    }
}

fn test_auth_state(access_ttl: Option<Duration>) -> (Arc<JwtManager>, Arc<AuthFacade>) {
    let jwt_config = JwtConfig {
        signing_key: SecretString::from("test-secret-key-for-jwt".to_owned()),
        access_ttl: access_ttl.unwrap_or(Duration::from_hours(1)),
        refresh_ttl: Duration::from_hours(24),
        issuer: "aletheia-test".to_owned(),
        // WHY: explicit zero leeway so the short-TTL expiry test observes
        // immediate expiry rather than the 30s default clock-skew tolerance.
        clock_skew_leeway_secs: 0,
    };
    let jwt_manager = Arc::new(JwtManager::new(jwt_config.clone()));
    let auth_facade =
        Arc::new(AuthFacade::in_memory(AuthConfig { jwt: jwt_config }).expect("auth facade"));
    (jwt_manager, auth_facade)
}

/// `SecurityConfig` with CSRF disabled: exercises the default route matrix
/// without requiring the CSRF header on mutations.
pub fn permissive_security() -> SecurityConfig {
    SecurityConfig {
        csrf: CsrfConfig {
            enabled: false,
            disable_acknowledged: true,
            ..CsrfConfig::default()
        },
        ..SecurityConfig::default()
    }
}

pub fn issue_test_token(state: &AppState) -> String {
    issue_test_token_as(state, Role::Operator)
}

pub fn issue_test_token_as(state: &AppState, role: Role) -> String {
    state
        .jwt_manager
        .issue_access("test-user", role, None)
        .expect("issue test token")
}

/// Issue a JWT scoped to a single `nous_id`, mirroring how an agent-scoped
/// token would be minted in production. Combined with `Claims` extraction,
/// scope enforcement via `require_nous_access` rejects cross-agent calls.
pub fn issue_test_token_scoped(state: &AppState, role: Role, nous_id: &str) -> String {
    state
        .jwt_manager
        .issue_access("test-user", role, Some(nous_id))
        .expect("issue test token")
}

pub fn bearer(token: &str) -> String {
    format!("{BEARER_PREFIX}{token}")
}

pub async fn read_body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("parse json")
}
