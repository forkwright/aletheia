use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use mneme::store::SessionStore;
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use symbolon::auth::{AuthConfig, AuthFacade};
use symbolon::jwt::{JwtConfig, JwtManager};
use taxis::config::AletheiaConfig;
use taxis::oikos::Oikos;

use crate::server::{
    shutdown_signal, shutdown_signal_with, spawn_sighup_handler, spawn_sighup_handler_with,
};
use crate::state::AppState;
use crate::tests::helpers::test_state;

// ─── Happy paths ───

#[tokio::test]
async fn shutdown_signal_does_not_panic_on_startup() {
    let handle = tokio::spawn(shutdown_signal());
    tokio::time::sleep(Duration::from_millis(50)).await;
    handle.abort();
    let _ = handle.await;
}

#[tokio::test]
#[cfg(unix)]
async fn spawn_sighup_handler_returns_some_and_exits_on_shutdown() {
    let (state, _dir) = test_state().await;
    let handle = spawn_sighup_handler(Arc::clone(&state));
    assert!(handle.is_some(), "should return Some in normal runtime");

    state.shutdown.cancel();
    if let Some(h) = handle {
        tokio::time::timeout(Duration::from_secs(5), h)
            .await
            .expect("should exit within timeout")
            .expect("should not panic");
    }
}

// ─── Failure paths ───

/// When ctrl+c handler installation fails, `shutdown_signal_with` should warn
/// and fall back to a future that never resolves (so the server keeps serving).
#[tokio::test]
async fn shutdown_signal_with_gracefully_handles_ctrl_c_failure() {
    let handle = tokio::spawn(shutdown_signal_with(
        Err(std::io::Error::other("injected ctrl+c failure")),
        #[cfg(unix)]
        Err(std::io::Error::other("injected SIGTERM failure")),
    ));

    // The fallback future never resolves, so abort it after a short timeout.
    tokio::time::timeout(Duration::from_millis(100), handle)
        .await
        .expect_err("should never resolve when both signals fail");
}

/// When SIGHUP handler installation fails, `spawn_sighup_handler_with` should
/// return `None` instead of panicking.
#[tokio::test]
#[cfg(unix)]
async fn spawn_sighup_handler_with_gracefully_handles_install_failure() {
    let state = minimal_app_state();
    let handle =
        spawn_sighup_handler_with(state, Err(std::io::Error::other("injected SIGHUP failure")));
    assert!(handle.is_none(), "should return None on install failure");
}

// Helper: build a minimal AppState without needing a full test harness.
fn minimal_app_state() -> Arc<AppState> {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();

    let store = SessionStore::open_in_memory().expect("in-memory store");
    let session_store = Arc::new(Mutex::new(store));
    let oikos = Arc::new(Oikos::from_root(root));

    let provider_registry = Arc::new(hermeneus::provider::ProviderRegistry::new());
    let tool_registry = Arc::new(ToolRegistry::new());

    let nous_manager = NousManager::new(
        Arc::clone(&provider_registry),
        Arc::clone(&tool_registry),
        Arc::clone(&oikos),
        None,
        None,
        Some(Arc::clone(&session_store)),
        #[cfg(feature = "knowledge-store")]
        None,
        Arc::new(Vec::new()),
        None,
        None,
        taxis::config::NousBehaviorConfig::default(),
        taxis::config::ToolLimitsConfig::default(),
    );

    let default_config = AletheiaConfig::default();
    let (config_tx, _config_rx) = tokio::sync::watch::channel(default_config.clone());

    let metrics_registry = koina::metrics::MetricsRegistry::new();
    crate::metrics::init(&metrics_registry);
    let workspace_root = crate::state::resolve_workspace_root(&oikos, None);

    let credential_runtime = Arc::new(crate::credential_runtime::CredentialRuntimeManager::new(
        Arc::clone(&provider_registry),
    ));

    Arc::new(AppState {
        session_store,
        nous_manager: Arc::new(nous_manager),
        provider_registry,
        tool_registry,
        oikos,
        workspace_root,
        jwt_manager: Arc::new(JwtManager::new(JwtConfig::default())),
        auth_facade: Arc::new(
            AuthFacade::in_memory(AuthConfig {
                jwt: JwtConfig::default(),
            })
            .expect("auth facade"),
        ),
        credential_runtime,
        start_time: Instant::now(),
        auth_mode: "token".to_owned(),
        none_role: "admin".to_owned(),
        config: Arc::new(tokio::sync::RwLock::new(default_config)),
        config_tx,
        idempotency_cache: Arc::new(crate::idempotency::IdempotencyCache::new()),
        shutdown: CancellationToken::new(),
        #[cfg(feature = "knowledge-store")]
        knowledge_store: None,
        embedding_provider: None,
        turn_buffer_registry: Arc::new(crate::turn_buffer::TurnBufferRegistry::new()),
        metrics_registry,
        event_bus: Arc::new(crate::event_bus::EventBus::new(256)),
        approval_registry: Arc::new(crate::approval_registry::ApprovalRegistry::new()),
        loopback_only_metrics: false,
    })
}
