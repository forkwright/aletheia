use std::sync::Arc;
use std::time::Instant;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use hermeneus::provider::{LlmProvider, ProviderRegistry};
use hermeneus::test_utils::MockProvider;
use koina::http::{BEARER_PREFIX, CONTENT_TYPE_JSON};
use koina::secret::SecretString;
use mneme::embedding::MockEmbeddingProvider;
use mneme::store::SessionStore;
use nous::config::{NousConfig, PipelineConfig};
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use symbolon::auth::{AuthConfig, AuthFacade};
use symbolon::jwt::{JwtConfig, JwtManager};
use taxis::oikos::Oikos;

pub(super) use crate::router::build_router;
pub(super) use crate::security::SecurityConfig;
pub(super) use crate::state::AppState;

/// Test helper: returns a `SecurityConfig` with CSRF disabled so that
/// POST/PUT/DELETE requests don't require the CSRF header in tests.
pub(super) fn test_security_config() -> SecurityConfig {
    SecurityConfig {
        csrf: crate::security::CsrfConfig {
            enabled: false,
            disable_acknowledged: true,
            ..crate::security::CsrfConfig::default()
        },
        ..SecurityConfig::default()
    }
}

pub(super) fn test_jwt_config() -> JwtConfig {
    JwtConfig {
        signing_key: SecretString::from("test-secret-key-for-jwt".to_owned()),
        access_ttl: std::time::Duration::from_hours(1),
        refresh_ttl: std::time::Duration::from_hours(24),
        issuer: "aletheia-test".to_owned(),
        ..JwtConfig::default()
    }
}

pub(super) fn test_jwt_manager() -> Arc<JwtManager> {
    Arc::new(JwtManager::new(test_jwt_config()))
}

pub(super) fn test_auth_facade() -> Arc<AuthFacade> {
    Arc::new(
        AuthFacade::in_memory(AuthConfig {
            jwt: test_jwt_config(),
        })
        .expect("in-memory auth facade"),
    )
}

pub(super) fn default_token() -> String {
    token_for_role(symbolon::types::Role::Operator)
}

pub(super) fn token_for_role(role: symbolon::types::Role) -> String {
    test_jwt_manager()
        .issue_access("test-user", role, None)
        .expect("test token")
}

/// Test helper: issue a JWT scoped to `nous_id`. Combined with the `Claims`
/// extractor, the production `require_nous_access` helper rejects calls
/// targeting any other agent's resources.
pub(super) fn token_scoped_to(role: symbolon::types::Role, nous_id: &str) -> String {
    test_jwt_manager()
        .issue_access("test-user", role, Some(nous_id))
        .expect("test scoped token")
}

pub(super) async fn test_state() -> (Arc<AppState>, tempfile::TempDir) {
    test_state_with_provider_private_and_auth_mode(true, false, "token").await
}

pub(super) async fn test_state_with_provider(
    with_provider: bool,
) -> (Arc<AppState>, tempfile::TempDir) {
    test_state_with_provider_private_and_auth_mode(with_provider, false, "token").await
}

pub(super) async fn test_state_with_private_nous() -> (Arc<AppState>, tempfile::TempDir) {
    test_state_with_provider_private_and_auth_mode(true, true, "token").await
}

pub(super) async fn test_state_with_auth_mode(
    auth_mode: &str,
) -> (Arc<AppState>, tempfile::TempDir) {
    test_state_with_provider_private_and_auth_mode(true, false, auth_mode).await
}

async fn test_state_with_provider_private_and_auth_mode(
    with_provider: bool,
    include_private_nous: bool,
    auth_mode: &str,
) -> (Arc<AppState>, tempfile::TempDir) {
    test_state_with_provider_name_private_and_auth_mode(
        with_provider,
        None,
        include_private_nous,
        auth_mode,
    )
    .await
}

async fn test_state_with_provider_name_private_and_auth_mode(
    with_provider: bool,
    provider_name: Option<&str>,
    include_private_nous: bool,
    auth_mode: &str,
) -> (Arc<AppState>, tempfile::TempDir) {
    let provider = with_provider.then(|| {
        let name = provider_name.unwrap_or("Hello from mock!");
        MockProvider::new(name).models(&["mock-model", "claude-opus-4-20250514"])
    });
    test_state_with_mock_provider(provider, include_private_nous, auth_mode).await
}

pub(super) async fn test_state_with_error_provider(
    message: &str,
) -> (Arc<AppState>, tempfile::TempDir) {
    test_state_with_mock_provider(
        Some(MockProvider::error(message).models(&["mock-model", "claude-opus-4-20250514"])),
        false,
        "token",
    )
    .await
}

async fn test_state_with_mock_provider(
    provider: Option<MockProvider>,
    include_private_nous: bool,
    auth_mode: &str,
) -> (Arc<AppState>, tempfile::TempDir) {
    test_state_with_llm_provider(
        provider.map(|p| {
            let boxed: Box<dyn LlmProvider> = Box::new(p);
            boxed
        }),
        include_private_nous,
        auth_mode,
    )
    .await
}

pub(super) async fn test_state_with_llm_provider(
    provider: Option<Box<dyn LlmProvider>>,
    include_private_nous: bool,
    auth_mode: &str,
) -> (Arc<AppState>, tempfile::TempDir) {
    test_state_with_llm_provider_and_tool_registry(
        provider,
        include_private_nous,
        auth_mode,
        ToolRegistry::new(),
        None,
    )
    .await
}

pub(super) async fn test_state_with_approval_test_tool(
    provider: Option<Box<dyn LlmProvider>>,
    tool_registry: ToolRegistry,
) -> (Arc<AppState>, tempfile::TempDir) {
    test_state_with_llm_provider_and_tool_registry(
        provider,
        false,
        "token",
        tool_registry,
        Some(organon::types::ToolGroupPolicy::AllowAll {
            reason: "approval endpoint regression fixture".to_owned(),
        }),
    )
    .await
}

#[expect(
    clippy::too_many_lines,
    reason = "WHY(#4872): test harness AppState construction with per-provider fixture setup is inherently long; inner expect(disallowed_methods) annotations inflate line count"
)]
async fn test_state_with_llm_provider_and_tool_registry(
    provider: Option<Box<dyn LlmProvider>>,
    include_private_nous: bool,
    auth_mode: &str,
    tool_registry: ToolRegistry,
    tool_groups: Option<organon::types::ToolGroupPolicy>,
) -> (Arc<AppState>, tempfile::TempDir) {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();

    // WHY: Create oikos directory structure required by the actor pipeline
    std::fs::create_dir_all(root.join("nous/syn")).expect("mkdir nous/syn");
    std::fs::create_dir_all(root.join("nous/workspace/src")).expect("mkdir nous/workspace/src");
    if include_private_nous {
        std::fs::create_dir_all(root.join("nous/hidden")).expect("mkdir nous/hidden");
    }
    std::fs::create_dir_all(root.join("shared")).expect("mkdir shared");
    std::fs::create_dir_all(root.join("theke/src")).expect("mkdir theke/src");
    std::fs::create_dir_all(root.join("data")).expect("mkdir data");
    std::fs::create_dir_all(root.join("config")).expect("mkdir config");

    // Create a minimal config file for health checks
    #[expect(
        clippy::disallowed_methods,
        reason = "pylon test helpers write config fixtures to temp directories; synchronous I/O is required in test setup"
    )]
    std::fs::write(
        root.join("config/aletheia.toml"),
        r#"
[gateway]
port = 18789
bind = "localhost"
"#,
    )
    .expect("write config file");

    // Create credentials directory and a mock credential file for health checks
    std::fs::create_dir_all(root.join("config/credentials")).expect("mkdir credentials");
    #[expect(
        clippy::disallowed_methods,
        reason = "pylon test helpers write credential fixtures to temp directories; synchronous I/O is required in test setup"
    )]
    std::fs::write(
        root.join("config/credentials/anthropic.json"),
        r#"{"token":"sk-ant-test-key-for-health-checks"}"#,
    )
    .expect("write credential file");
    #[expect(
        clippy::disallowed_methods,
        reason = "pylon test helpers write TLS fixtures to temp directories; synchronous I/O is required in test setup"
    )]
    std::fs::write(root.join("nous/syn/SOUL.md"), "I am Syn, a test agent.")
        .expect("write SOUL.md");
    #[expect(
        clippy::disallowed_methods,
        reason = "pylon test helpers write workspace fixtures to temp directories; synchronous I/O is required in test setup"
    )]
    std::fs::write(root.join("theke/README.md"), "Workspace root fixture.\n")
        .expect("write workspace README");
    #[expect(
        clippy::disallowed_methods,
        reason = "pylon test helpers write workspace fixtures to temp directories; synchronous I/O is required in test setup"
    )]
    std::fs::write(
        root.join("theke/src/main.rs"),
        "fn main() {\n    println!(\"hello from workspace\");\n}\n",
    )
    .expect("write workspace source");
    if include_private_nous {
        #[expect(
            clippy::disallowed_methods,
            reason = "pylon test helpers write private nous fixtures to temp directories; synchronous I/O is required in test setup"
        )]
        std::fs::write(
            root.join("nous/hidden/SOUL.md"),
            "I am Hidden, a private test agent.",
        )
        .expect("write hidden SOUL.md");
    }

    let store = SessionStore::open_in_memory().expect("in-memory store");
    let session_store = Arc::new(Mutex::new(store));
    let oikos = Arc::new(Oikos::from_root(root));

    let mut provider_registry = ProviderRegistry::new();
    if let Some(provider) = provider {
        provider_registry.register(provider);
    }
    let provider_registry = Arc::new(provider_registry);
    let tool_registry = Arc::new(tool_registry);

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

    let nous_config = NousConfig {
        id: Arc::from("syn"),
        generation: nous::config::NousGenerationConfig {
            model: "mock-model".to_owned(),
            ..Default::default()
        },
        tool_groups: tool_groups.clone().unwrap_or_default(),
        ..NousConfig::default()
    };
    nous_manager
        .spawn(nous_config, PipelineConfig::default())
        .await
        .expect("spawn nous in test harness");
    if include_private_nous {
        let hidden_config = NousConfig {
            id: Arc::from("hidden"),
            private: true,
            generation: nous::config::NousGenerationConfig {
                model: "mock-model".to_owned(),
                ..Default::default()
            },
            tool_groups: tool_groups.clone().unwrap_or_default(),
            ..NousConfig::default()
        };
        nous_manager
            .spawn(hidden_config, PipelineConfig::default())
            .await
            .expect("spawn private nous in test harness");
    }

    let jwt_manager = test_jwt_manager();
    let auth_facade = test_auth_facade();
    let workspace_root = crate::state::resolve_workspace_root(&oikos, None);

    let mut default_config = taxis::config::AletheiaConfig::default();
    default_config.gateway.sse_heartbeat_interval_secs = 1;
    let (config_tx, _config_rx) = tokio::sync::watch::channel(default_config.clone());

    // WHY: pylon's /metrics handler requires a registry; tests need pylon's
    // own families registered to match production behaviour.
    let metrics_registry = koina::metrics::MetricsRegistry::new();
    crate::metrics::init(&metrics_registry);

    let credential_runtime = Arc::new(crate::credential_runtime::CredentialRuntimeManager::new(
        Arc::clone(&provider_registry),
    ));

    let state = Arc::new(AppState {
        session_store: Arc::clone(&session_store),
        nous_manager: Arc::new(nous_manager),
        provider_registry,
        tool_registry,
        oikos,
        workspace_root,
        jwt_manager,
        auth_facade,
        credential_runtime,
        start_time: Instant::now(),
        auth_mode: auth_mode.to_owned(),
        none_role: "admin".to_owned(),
        config: Arc::new(tokio::sync::RwLock::new(default_config)),
        config_tx,
        idempotency_cache: Arc::new(crate::idempotency::IdempotencyCache::new()),
        shutdown: CancellationToken::new(),
        #[cfg(feature = "knowledge-store")]
        knowledge_store: None,
        embedding_provider: Some(Arc::new(MockEmbeddingProvider::new(384))),
        turn_buffer_registry: Arc::new(crate::turn_buffer::TurnBufferRegistry::new()),
        metrics_registry,
        event_bus: Arc::new(crate::event_bus::EventBus::new(256)),
        approval_registry: Arc::new(crate::approval_registry::ApprovalRegistry::new()),
        loopback_only_metrics: false,
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

pub(super) async fn app_with_auth_mode(auth_mode: &str) -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state_with_auth_mode(auth_mode).await;
    (build_router(state, &test_security_config()), dir)
}

/// Test helper: app with a registered provider named "anthropic" so that
/// credential-management mutations exercise the canonical managed-provider path.
pub(super) async fn app_with_anthropic_provider() -> (axum::Router, tempfile::TempDir) {
    let (state, dir) = test_state_with_provider_name_private_and_auth_mode(
        true,
        Some("anthropic"),
        false,
        "token",
    )
    .await;
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
    authed_request_as(method, uri, body, symbolon::types::Role::Operator)
}

pub(super) fn authed_request_as(
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
    role: symbolon::types::Role,
) -> Request<Body> {
    let token = token_for_role(role);
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
    authed_get_as(uri, symbolon::types::Role::Operator)
}

pub(super) fn authed_get_as(uri: &str, role: symbolon::types::Role) -> Request<Body> {
    let token = token_for_role(role);
    Request::get(uri)
        .header("authorization", format!("{BEARER_PREFIX}{token}"))
        .body(Body::empty())
        .unwrap()
}

pub(super) fn authed_get_scoped_as(
    uri: &str,
    role: symbolon::types::Role,
    nous_id: &str,
) -> Request<Body> {
    let token = token_scoped_to(role, nous_id);
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
