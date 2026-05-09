//! Config hot-reload integration test: verify `config_tx` broadcasts changes.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::time::{Duration, Instant};

use hermeneus::provider::ProviderRegistry;
use hermeneus::test_utils::MockProvider;
use mneme::store::SessionStore;
use nous::config::{NousConfig, PipelineConfig};
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use pylon::state::AppState;
use symbolon::auth::{AuthConfig, AuthFacade};
use symbolon::jwt::{JwtConfig, JwtManager};
use taxis::oikos::Oikos;
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;

struct TestHarness {
    state: std::sync::Arc<AppState>,
    _tmp: tempfile::TempDir,
}

impl TestHarness {
    async fn build() -> Self {
        let dir = tempfile::TempDir::new().expect("tmpdir");
        let root = dir.path();

        std::fs::create_dir_all(root.join("nous/test-nous")).expect("mkdir");
        std::fs::create_dir_all(root.join("shared")).expect("mkdir");
        std::fs::create_dir_all(root.join("theke")).expect("mkdir");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup writes fixtures to temp directory"
        )]
        std::fs::write(root.join("nous/test-nous/SOUL.md"), "You are a test agent.")
            .expect("write SOUL.md");
        #[expect(
            clippy::disallowed_methods,
            reason = "test setup writes fixtures to temp directory"
        )]
        std::fs::write(root.join("theke/USER.md"), "Test user.").expect("write USER.md");

        let oikos = std::sync::Arc::new(Oikos::from_root(root));
        let store = SessionStore::open_in_memory().expect("in-memory store");

        let mut provider_registry = ProviderRegistry::new();
        provider_registry.register(Box::new(
            MockProvider::new("Hello from mock!").models(&["mock-model"]),
        ));
        let provider_registry = std::sync::Arc::new(provider_registry);
        let tool_registry = std::sync::Arc::new(ToolRegistry::new());

        let session_store = std::sync::Arc::new(TokioMutex::new(store));

        let mut nous_manager = NousManager::new(
            std::sync::Arc::clone(&provider_registry),
            std::sync::Arc::clone(&tool_registry),
            std::sync::Arc::clone(&oikos),
            None,
            None,
            Some(std::sync::Arc::clone(&session_store)),
            #[cfg(feature = "knowledge-store")]
            None,
            std::sync::Arc::new(vec![]),
            None,
            None,
            taxis::config::NousBehaviorConfig::default(),
        );

        let nous_config = NousConfig {
            id: std::sync::Arc::from("test-nous"),
            generation: nous::config::NousGenerationConfig {
                model: "mock-model".to_owned(),
                ..Default::default()
            },
            ..NousConfig::default()
        };
        nous_manager
            .spawn(nous_config, PipelineConfig::default())
            .await
            .expect("spawn nous");

        let jwt_manager = std::sync::Arc::new(JwtManager::new(JwtConfig {
            signing_key: koina::secret::SecretString::from("test-secret-key-for-jwt".to_owned()),
            access_ttl: Duration::from_hours(1),
            refresh_ttl: Duration::from_hours(24),
            issuer: "aletheia-test".to_owned(),
            ..JwtConfig::default()
        }));

        let default_config = taxis::config::AletheiaConfig::default();
        let (config_tx, _config_rx) = tokio::sync::watch::channel(default_config.clone());
        let metrics_registry = koina::metrics::MetricsRegistry::new();
        metrics_registry.with_registry(pylon::metrics::register);
        let state = std::sync::Arc::new(AppState {
            session_store,
            nous_manager: std::sync::Arc::new(nous_manager),
            provider_registry,
            tool_registry,
            oikos,
            jwt_manager,
            auth_facade: std::sync::Arc::new(
                AuthFacade::in_memory(AuthConfig {
                    jwt: JwtConfig::default(),
                })
                .expect("auth facade"),
            ),
            start_time: Instant::now(),
            auth_mode: "token".to_owned(),
            none_role: "admin".to_owned(),
            config: std::sync::Arc::new(tokio::sync::RwLock::new(default_config)),
            config_tx,
            idempotency_cache: std::sync::Arc::new(pylon::idempotency::IdempotencyCache::new()),
            shutdown: CancellationToken::new(),
            #[cfg(feature = "knowledge-store")]
            knowledge_store: None,
            embedding_provider: None,
            turn_buffer_registry: std::sync::Arc::new(pylon::turn_buffer::TurnBufferRegistry::new()),
            metrics_registry,
            event_bus: std::sync::Arc::new(pylon::event_bus::EventBus::new(256)),
        });

        Self { state, _tmp: dir }
    }
}

#[tokio::test]
async fn config_tx_broadcasts_updated_config() {
    let harness = TestHarness::build().await;

    // Subscribe to config changes via the public config_tx field.
    let mut rx = harness.state.config_tx.subscribe();
    let initial = rx.borrow_and_update().clone();

    // Build a mutated config (change a field we can observe).
    let mut new_config = initial.clone();
    new_config.gateway.port = 9999;

    // Broadcast the change.
    harness
        .state
        .config_tx
        .send(new_config.clone())
        .expect("send config");

    // Wait for the subscriber to see the new value.
    let timeout = tokio::time::timeout(Duration::from_secs(5), rx.changed());
    timeout
        .await
        .expect("timed out waiting for config change")
        .expect("config_tx channel closed");

    let updated = rx.borrow_and_update().clone();
    assert_eq!(
        updated.gateway.port, 9999,
        "subscriber should receive updated config port"
    );
}
