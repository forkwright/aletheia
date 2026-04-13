//! Integration tests: run eval scenarios against a real TCP-bound pylon instance.

#![expect(clippy::expect_used, reason = "test assertions")]
#![cfg(feature = "sqlite-tests")]

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::TcpListener;
use tokio::sync::Mutex as TokioMutex;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use dokimion::runner::{RunConfig, ScenarioRunner};
use hermeneus::provider::ProviderRegistry;
use hermeneus::test_utils::MockProvider;
use koina::secret::SecretString;
use mneme::store::SessionStore;
use nous::config::{NousConfig, PipelineConfig};
use nous::manager::NousManager;
use organon::registry::ToolRegistry;
use organon::testing::install_crypto_provider;
use pylon::router::build_router;
use pylon::state::AppState;
use symbolon::jwt::{JwtConfig, JwtManager};
use symbolon::types::Role;
use taxis::oikos::Oikos;

#[expect(clippy::too_many_lines, reason = "test server setup is inherently verbose")]
async fn start_test_server() -> (String, String, tempfile::TempDir) {
    install_crypto_provider();
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("nous/test-nous")).expect("mkdir nous");
    std::fs::create_dir_all(root.join("shared")).expect("mkdir shared");
    std::fs::create_dir_all(root.join("theke")).expect("mkdir theke");
    #[expect(
        clippy::disallowed_methods,
        reason = "integration tests write fixture files to temp directories; synchronous I/O is required in test setup"
    )]
    std::fs::write(root.join("nous/test-nous/SOUL.md"), "You are a test agent.")
        .expect("write SOUL.md");
    #[expect(
        clippy::disallowed_methods,
        reason = "integration tests write fixture files to temp directories; synchronous I/O is required in test setup"
    )]
    std::fs::write(root.join("theke/USER.md"), "Test user.").expect("write USER.md");

    let oikos = Arc::new(Oikos::from_root(root));
    let store = SessionStore::open_in_memory().expect("in-memory store");

    let mut provider_registry = ProviderRegistry::new();
    provider_registry.register(Box::new(
        MockProvider::new("Hello from eval harness!").models(&["mock-model"]),
    ));
    let provider_registry = Arc::new(provider_registry);
    let tool_registry = Arc::new(ToolRegistry::new());
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
        taxis::config::NousBehaviorConfig::default(),
    );

    let nous_config = NousConfig {
        id: Arc::from("test-nous"),
        generation: nous::config::NousGenerationConfig {
            model: "mock-model".to_owned(),
            ..Default::default()
        },
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

    let default_config = taxis::config::AletheiaConfig::default();
    let (config_tx, _config_rx) = tokio::sync::watch::channel(default_config.clone());
    let state = Arc::new(AppState {
        session_store,
        nous_manager: Arc::new(nous_manager),
        provider_registry,
        tool_registry,
        oikos,
        jwt_manager,
        start_time: Instant::now(),
        auth_mode: "token".to_owned(),
        none_role: "admin".to_owned(),
        config: Arc::new(tokio::sync::RwLock::new(default_config)),
        config_tx,
        idempotency_cache: Arc::new(pylon::idempotency::IdempotencyCache::new()),
        shutdown: CancellationToken::new(),
        #[cfg(feature = "knowledge-store")]
        knowledge_store: None,
    });

    let router = build_router(
        Arc::clone(&state),
        &pylon::security::SecurityConfig {
            csrf: pylon::security::CsrfConfig {
                enabled: false,
                ..pylon::security::CsrfConfig::default()
            },
            ..pylon::security::SecurityConfig::default()
        },
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let base_url = format!("http://{addr}");

    tokio::spawn(
        async move {
            axum::serve(listener, router).await.expect("serve");
        }
        .instrument(tracing::info_span!("test_server")),
    );

    (base_url, token, dir)
}

#[tokio::test]
async fn eval_health_scenarios_pass() {
    let (base_url, _token, _dir) = start_test_server().await;

    let config = RunConfig {
        base_url,
        token: None,
        filter: None,
        category_filter: Some("health".to_owned()),
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
        filter: None,
        category_filter: Some("auth".to_owned()),
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
        token: Some(SecretString::from(token)),
        filter: None,
        category_filter: Some("nous".to_owned()),
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
    // WHY: filter by EXACT category to exclude `canary-session-*` scenarios
    // that exercise the LLM and would fail against the mock provider. Bug
    // #2999: previous version used `filter: Some("session")` which matched
    // any id containing "session" — including the canary-session ids.
    let (base_url, token, _dir) = start_test_server().await;

    let config = RunConfig {
        base_url,
        token: Some(SecretString::from(token)),
        filter: None,
        category_filter: Some("session".to_owned()),
        fail_fast: false,
        timeout_secs: 10,
        json_output: false,
    };

    let runner = ScenarioRunner::new(config);
    let report = runner.run().await;

    let failures: Vec<String> = report
        .results
        .iter()
        .filter_map(|r| match &r.outcome {
            dokimion::scenario::ScenarioOutcome::Failed { error, .. } => {
                Some(format!("{}: {error}", r.meta.id))
            }
            _ => None,
        })
        .collect();

    assert_eq!(
        report.failed, 0,
        "session scenarios should all pass; failures: {failures:#?}"
    );
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
        token: Some(SecretString::from(token)),
        filter: None,
        category_filter: Some("conversation".to_owned()),
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
async fn eval_full_run_excludes_canary() {
    // WHY: full run excludes the `canary-*` categories which exercise a real
    // LLM and would fail against the mock provider. Run all OTHER categories
    // to confirm cross-category orchestration works end to end.
    //
    // Categories that need an LLM: canary-recall, canary-session, canary-conversation.
    // Categories that don't: health, auth, nous, session, conversation.
    let (base_url, token, _dir) = start_test_server().await;

    // No filter at all → include everything except canary categories. We
    // accomplish that by running each non-canary category in turn and
    // accumulating the result. This is more honest than asserting on a
    // specific count and lets new non-canary scenarios just work.
    let mut total_passed = 0_usize;
    let mut total_failed = 0_usize;
    for category in ["health", "auth", "nous", "session", "conversation"] {
        let config = RunConfig {
            base_url: base_url.clone(),
            token: Some(SecretString::from(token.clone())),
            filter: None,
            category_filter: Some(category.to_owned()),
            fail_fast: false,
            timeout_secs: 15,
            json_output: true,
        };
        let runner = ScenarioRunner::new(config);
        let report = runner.run().await;
        total_passed += report.passed;
        total_failed += report.failed;
    }

    assert_eq!(
        total_failed, 0,
        "all non-canary scenarios should pass against test harness"
    );
    assert!(
        total_passed >= 10,
        "expect at least 10 passing non-canary scenarios; got {total_passed}"
    );
}
