use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use aletheia_hermeneus::provider::ProviderRegistry;
use aletheia_hermeneus::test_utils::MockProvider;
use aletheia_koina::http::{BEARER_PREFIX, CONTENT_TYPE_JSON};
use aletheia_koina::secret::SecretString;
use aletheia_mneme::store::SessionStore;
use aletheia_nous::config::{NousConfig, PipelineConfig};
use aletheia_nous::manager::NousManager;
use aletheia_organon::registry::ToolRegistry;
use aletheia_symbolon::jwt::{JwtConfig, JwtManager};
use aletheia_taxis::oikos::Oikos;

pub(super) use crate::router::build_router;
pub(super) use crate::security::SecurityConfig;
pub(super) use crate::state::AppState;

/// Test helper: returns a `SecurityConfig` with CSRF disabled so that
/// POST/PUT/DELETE requests don't require the CSRF header in tests.
pub(super) fn test_security_config() -> SecurityConfig {
    SecurityConfig {
        csrf_enabled: false,
        ..SecurityConfig::default()
    }
}

pub(super) fn test_jwt_manager() -> Arc<JwtManager> {
    Arc::new(JwtManager::new(JwtConfig {
        signing_key: SecretString::from("test-secret-key-for-jwt".to_owned()),
        access_ttl: std::time::Duration::from_secs(3600),
        refresh_ttl: std::time::Duration::from_secs(86400),
        issuer: "aletheia-test".to_owned(),
    }))
}

pub(super) fn default_token() -> String {
    test_jwt_manager()
        .issue_access("test-user", aletheia_symbolon::types::Role::Operator, None)
        .expect("test token")
}

pub(super) async fn test_state() -> (Arc<AppState>, tempfile::TempDir) {
    test_state_with_provider(true).await
}

pub(super) async fn test_state_with_provider(
    with_provider: bool,
) -> (Arc<AppState>, tempfile::TempDir) {
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
        provider_registry.register(Box::new(
            MockProvider::new("Hello from mock!").models(&["mock-model", "claude-opus-4-20250514"]),
        ));
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

    let default_config = aletheia_taxis::config::AletheiaConfig::default();
    let (config_tx, _config_rx) = tokio::sync::watch::channel(default_config.clone());

    let state = Arc::new(AppState {
        session_store: Arc::clone(&session_store),
        nous_manager: Arc::new(nous_manager),
        provider_registry,
        tool_registry,
        oikos,
        jwt_manager,
        start_time: Instant::now(),
        auth_mode: "token".to_owned(),
        config: Arc::new(tokio::sync::RwLock::new(default_config)),
        config_tx,
        idempotency_cache: Arc::new(crate::idempotency::IdempotencyCache::new()),
        shutdown: CancellationToken::new(),
        #[cfg(feature = "knowledge-store")]
        knowledge_store: None,
    });

    (state, dir)
}

pub(super) async fn app() -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state().await;
    (build_router(state, &test_security_config()), dir)
}

pub(super) async fn app_no_providers() -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state_with_provider(false).await;
    (build_router(state, &test_security_config()), dir)
}

pub(super) fn json_request(
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", CONTENT_TYPE_JSON);

    match body {
        Some(b) => builder
            .body(Body::from(serde_json::to_vec(&b).unwrap()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

pub(super) fn authed_request(
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> Request<Body> {
    let token = default_token();
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", CONTENT_TYPE_JSON)
        .header("authorization", format!("{BEARER_PREFIX}{token}"));

    match body {
        Some(b) => builder
            .body(Body::from(serde_json::to_vec(&b).unwrap()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    }
}

pub(super) fn authed_get(uri: &str) -> Request<Body> {
    let token = default_token();
    Request::get(uri)
        .header("authorization", format!("{BEARER_PREFIX}{token}"))
        .body(Body::empty())
        .unwrap()
}

pub(super) fn authed_delete(uri: &str) -> Request<Body> {
    let token = default_token();
    Request::delete(uri)
        .header("authorization", format!("{BEARER_PREFIX}{token}"))
        .body(Body::empty())
        .unwrap()
}

pub(super) async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

pub(super) async fn body_string(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

pub(super) async fn create_test_session(app: &axum::Router) -> serde_json::Value {
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
