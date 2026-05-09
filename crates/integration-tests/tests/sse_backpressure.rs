//! SSE backpressure test: slow consumer should not cause OOM.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hermeneus::provider::ProviderRegistry;
use hermeneus::test_utils::MockProvider;
use http_body_util::BodyExt;
use mneme::store::SessionStore;
use nous::config::{NousConfig, PipelineConfig};
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use pylon::router::build_router;
use pylon::state::AppState;
use symbolon::auth::{AuthConfig, AuthFacade};
use symbolon::jwt::{JwtConfig, JwtManager};
use symbolon::types::Role;
use taxis::oikos::Oikos;
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

struct TestHarness {
    state: std::sync::Arc<AppState>,
    jwt_manager: std::sync::Arc<JwtManager>,
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
            jwt_manager: std::sync::Arc::clone(&jwt_manager),
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
            std::sync::Arc::clone(&self.state),
            &pylon::security::SecurityConfig {
                csrf: pylon::security::CsrfConfig {
                    enabled: false,
                    ..pylon::security::CsrfConfig::default()
                },
                ..pylon::security::SecurityConfig::default()
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
}

/// Parse every `data:` line in an SSE body into JSON values.
fn collect_sse_data_events(body: &str) -> Vec<serde_json::Value> {
    body.lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .filter_map(|data| serde_json::from_str(data.trim()).ok())
        .collect()
}

#[tokio::test]
async fn slow_consumer_completes_without_oom() {
    let harness = TestHarness::build().await;
    let router = harness.router();

    let req = harness.authed_request(
        "POST",
        "/api/v1/sessions",
        Some(serde_json::json!({
            "nous_id": "test-nous",
            "session_key": "backpressure-test"
        })),
    );
    let resp = router.clone().oneshot(req).await.expect("create session");
    assert_eq!(resp.status(), StatusCode::CREATED);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("read body");
    let session: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
    let id = session
        .get("id")
        .and_then(|v| v.as_str())
        .expect("session id");

    let req = harness.authed_request(
        "POST",
        &format!("/api/v1/sessions/{id}/messages"),
        Some(serde_json::json!({ "content": "hello" })),
    );
    let resp = router.clone().oneshot(req).await.expect("send message");
    assert_eq!(resp.status(), StatusCode::OK);

    // Simulate a slow consumer: read one frame per second.
    let mut body = resp.into_body();
    let mut collected = Vec::new();
    while let Some(Ok(frame)) = body.frame().await {
        if let Some(chunk) = frame.data_ref() {
            collected.extend_from_slice(chunk);
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    let body_str = String::from_utf8(collected).expect("utf8");
    let events = collect_sse_data_events(&body_str);
    assert!(
        events.iter().any(|e| e["type"] == "text_delta"),
        "slow consumer should still receive text_delta, got: {body_str}"
    );
    assert!(
        events.iter().any(|e| e["type"] == "message_complete"),
        "slow consumer should still receive message_complete, got: {body_str}"
    );
}
