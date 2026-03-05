//! Integration tests: run eval scenarios against a real TCP-bound pylon instance.
#![cfg(feature = "sqlite-tests")]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use secrecy::SecretString;
use tokio::net::TcpListener;

use aletheia_dokimion::runner::{RunConfig, ScenarioRunner};
use aletheia_hermeneus::provider::{LlmProvider, ProviderRegistry};
use aletheia_hermeneus::types::*;
use aletheia_mneme::store::SessionStore;
use aletheia_nous::config::{NousConfig, PipelineConfig};
use aletheia_nous::manager::NousManager;
use aletheia_organon::registry::ToolRegistry;
use aletheia_pylon::router::build_router;
use aletheia_pylon::state::AppState;
use aletheia_symbolon::jwt::{JwtConfig, JwtManager};
use aletheia_symbolon::types::Role;
use aletheia_taxis::oikos::Oikos;

struct MockProvider {
    response: CompletionResponse,
}

impl MockProvider {
    fn new() -> Self {
        Self {
            response: CompletionResponse {
                id: "msg_test".to_owned(),
                model: "mock-model".to_owned(),
                stop_reason: StopReason::EndTurn,
                content: vec![ContentBlock::Text {
                    text: "Hello from eval harness!".to_owned(),
                    citations: None,
                }],
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                    ..Usage::default()
                },
            },
        }
    }
}

impl LlmProvider for MockProvider {
    fn complete(
        &self,
        _request: &CompletionRequest,
    ) -> aletheia_hermeneus::error::Result<CompletionResponse> {
        Ok(self.response.clone())
    }

    fn supported_models(&self) -> &[&str] {
        &["mock-model"]
    }

    #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str")]
    fn name(&self) -> &str {
        "mock"
    }
}

async fn start_test_server() -> (String, String, tempfile::TempDir) {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("nous/test-nous")).expect("mkdir nous");
    std::fs::create_dir_all(root.join("shared")).expect("mkdir shared");
    std::fs::create_dir_all(root.join("theke")).expect("mkdir theke");
    std::fs::write(root.join("nous/test-nous/SOUL.md"), "You are a test agent.")
        .expect("write SOUL.md");
    std::fs::write(root.join("theke/USER.md"), "Test user.").expect("write USER.md");

    let oikos = Arc::new(Oikos::from_root(root));
    let store = SessionStore::open_in_memory().expect("in-memory store");

    let mut provider_registry = ProviderRegistry::new();
    provider_registry.register(Box::new(MockProvider::new()));
    let provider_registry = Arc::new(provider_registry);
    let tool_registry = Arc::new(ToolRegistry::new());
    let session_store = Arc::new(Mutex::new(store));

    let mut nous_manager = NousManager::new(
        Arc::clone(&provider_registry),
        Arc::clone(&tool_registry),
        Arc::clone(&oikos),
        None,
        None,
        Some(Arc::clone(&session_store)),
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

    let token = jwt_manager
        .issue_access("test-user", Role::Operator, None)
        .expect("test token");

    let state = Arc::new(AppState {
        session_store,
        nous_manager: Arc::new(nous_manager),
        provider_registry,
        tool_registry,
        oikos,
        jwt_manager,
        start_time: Instant::now(),
        config: Arc::new(tokio::sync::RwLock::new(
            aletheia_taxis::config::AletheiaConfig::default(),
        )),
    });

    let router = build_router(
        Arc::clone(&state),
        &aletheia_pylon::security::SecurityConfig::default(),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let base_url = format!("http://{addr}");

    tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });

    (base_url, token, dir)
}

#[tokio::test]
async fn eval_health_scenarios_pass() {
    let (base_url, _token, _dir) = start_test_server().await;

    let config = RunConfig {
        base_url,
        token: None,
        filter: Some("health".to_owned()),
        fail_fast: false,
        timeout_secs: 10,
        json_output: false,
    };

    let runner = ScenarioRunner::new(config);
    let report = runner.run().await;

    assert_eq!(report.failed, 0, "health scenarios should all pass");
    assert!(report.passed > 0, "at least one health scenario should run");
}

#[tokio::test]
async fn eval_auth_scenarios_pass() {
    let (base_url, _token, _dir) = start_test_server().await;

    let config = RunConfig {
        base_url,
        token: None,
        filter: Some("auth".to_owned()),
        fail_fast: false,
        timeout_secs: 10,
        json_output: false,
    };

    let runner = ScenarioRunner::new(config);
    let report = runner.run().await;

    assert_eq!(report.failed, 0, "auth scenarios should all pass");
    assert!(report.passed > 0, "at least one auth scenario should run");
}

#[tokio::test]
async fn eval_nous_scenarios_pass() {
    let (base_url, token, _dir) = start_test_server().await;

    let config = RunConfig {
        base_url,
        token: Some(token),
        filter: Some("nous".to_owned()),
        fail_fast: false,
        timeout_secs: 10,
        json_output: false,
    };

    let runner = ScenarioRunner::new(config);
    let report = runner.run().await;

    assert_eq!(report.failed, 0, "nous scenarios should all pass");
    assert!(report.passed > 0, "at least one nous scenario should run");
}

#[tokio::test]
async fn eval_session_scenarios_pass() {
    let (base_url, token, _dir) = start_test_server().await;

    let config = RunConfig {
        base_url,
        token: Some(token),
        filter: Some("session".to_owned()),
        fail_fast: false,
        timeout_secs: 10,
        json_output: false,
    };

    let runner = ScenarioRunner::new(config);
    let report = runner.run().await;

    assert_eq!(report.failed, 0, "session scenarios should all pass");
    assert!(
        report.passed > 0,
        "at least one session scenario should run"
    );
}

#[tokio::test]
async fn eval_conversation_scenarios_pass() {
    let (base_url, token, _dir) = start_test_server().await;

    let config = RunConfig {
        base_url,
        token: Some(token),
        filter: Some("conversation".to_owned()),
        fail_fast: false,
        timeout_secs: 15,
        json_output: false,
    };

    let runner = ScenarioRunner::new(config);
    let report = runner.run().await;

    assert_eq!(report.failed, 0, "conversation scenarios should all pass");
    assert!(
        report.passed > 0,
        "at least one conversation scenario should run"
    );
}

#[tokio::test]
async fn eval_full_run_with_json_output() {
    let (base_url, token, _dir) = start_test_server().await;

    let config = RunConfig {
        base_url,
        token: Some(token),
        filter: None,
        fail_fast: false,
        timeout_secs: 15,
        json_output: true,
    };

    let runner = ScenarioRunner::new(config);
    let report = runner.run().await;

    assert_eq!(
        report.failed, 0,
        "all scenarios should pass against test harness"
    );
    assert!(report.passed >= 10, "expect at least 10 passing scenarios");
    assert_eq!(
        report.skipped, 0,
        "no scenarios should skip with full auth + nous"
    );
}
