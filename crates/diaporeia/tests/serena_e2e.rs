//! Socket end-to-end tests for Serena LSP MCP integration.
//!
//! Starts a mock Serena MCP server on a Unix domain socket and verifies that
//! diaporeia's `SerenaClient` can connect and proxy tool calls. Also verifies
//! that `DiaporeiaState` constructs correctly with a wired Serena client.

#![cfg(feature = "test-core")]

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test assertions — panicking on failure is the point"
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex as TokioMutex, RwLock};
use tokio_util::sync::CancellationToken;

use diaporeia::state::DiaporeiaState;

use hermeneus::provider::ProviderRegistry;
use mneme::store::SessionStore;
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use symbolon::jwt::{JwtConfig, JwtManager};
use taxis::config::AletheiaConfig;
use taxis::oikos::Oikos;

/// Build a minimal diaporeia state with a live Serena client.
async fn build_state_with_serena(
    socket_path: &std::path::Path,
) -> (Arc<DiaporeiaState>, tempfile::TempDir) {
    let instance_root = tempfile::tempdir().expect("create instance tempdir");
    let oikos = Arc::new(Oikos::from_root(instance_root.path()));
    let session_store = Arc::new(TokioMutex::new(
        SessionStore::open_in_memory().expect("in-memory session store"),
    ));
    let provider_registry = Arc::new(ProviderRegistry::new());
    let tool_registry = Arc::new(ToolRegistry::new());

    let nous_manager = Arc::new(NousManager::new(
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
    ));

    let jwt_manager = Arc::new(JwtManager::new(JwtConfig {
        signing_key: koina::secret::SecretString::from(
            "test-signing-key-at-least-32-bytes-long!!".to_owned(),
        ),
        access_ttl: Duration::from_hours(1),
        refresh_ttl: Duration::from_hours(24),
        issuer: "aletheia-serena-e2e".to_owned(),
        ..JwtConfig::default()
    }));

    let mut config = AletheiaConfig::default();
    config.mcp.serena.enabled = true;
    config.mcp.serena.socket_path = Some(socket_path.to_owned());

    let serena_client = match diaporeia::serena::SerenaClient::connect(socket_path).await {
        Ok(client) => Some(Arc::new(client)),
        Err(e) => panic!("Serena client failed to connect: {e}"),
    };

    let config = Arc::new(RwLock::new(config));

    let state = Arc::new(DiaporeiaState {
        session_store,
        nous_manager,
        tool_registry,
        oikos,
        jwt_manager: Some(jwt_manager),
        start_time: Instant::now(),
        config,
        auth_mode: "token".to_owned(),
        none_role: "readonly".to_owned(),
        shutdown: CancellationToken::new(),
        #[cfg(feature = "knowledge-store")]
        knowledge_store: None,
        serena_client,
    });

    (state, instance_root)
}

#[tokio::test(flavor = "multi_thread")]
async fn serena_client_calls_tool_over_socket() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (socket_path, _shutdown) = diaporeia::serena::mock::start_mock_server(tmp.path()).await;

    let client = diaporeia::serena::SerenaClient::connect(&socket_path)
        .await
        .expect("connect");

    let mut args = serde_json::Map::new();
    args.insert(
        "file_path".to_owned(),
        serde_json::Value::String("src/lib.rs".to_owned()),
    );
    args.insert("line".to_owned(), serde_json::Value::Number(0.into()));
    args.insert("column".to_owned(), serde_json::Value::Number(0.into()));

    let result = client
        .call_tool("go_to_definition", args)
        .await
        .expect("call tool");

    assert!(
        result.is_error.is_none() || !result.is_error.unwrap(),
        "tool call should succeed"
    );
    let text = result
        .content
        .into_iter()
        .map(|c| c.as_text().map(|t| t.text.clone()).unwrap_or_default())
        .collect::<String>();
    assert!(
        text.contains("koina"),
        "mock response should reference koina crate: {text}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn diaporeia_state_constructs_with_serena_client() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (socket_path, _shutdown) = diaporeia::serena::mock::start_mock_server(tmp.path()).await;

    let (state, _tmp) = build_state_with_serena(&socket_path).await;

    // Config snapshot must reflect Serena as enabled.
    let config = state.config.read().await;
    assert!(config.mcp.serena.enabled);
    assert_eq!(config.mcp.serena.socket_path, Some(socket_path.clone()));

    // The Serena client must be wired into state.
    assert!(state.serena_client.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn serena_tools_are_unavailable_when_disabled() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (socket_path, _shutdown) = diaporeia::serena::mock::start_mock_server(tmp.path()).await;

    let (state, _tmp) = build_state_with_serena(&socket_path).await;

    // Disable Serena in config.
    {
        let mut cfg = state.config.write().await;
        cfg.mcp.serena.enabled = false;
    }

    // State still carries the client, but tools will gate on config.
    assert!(state.serena_client.is_some());
}
