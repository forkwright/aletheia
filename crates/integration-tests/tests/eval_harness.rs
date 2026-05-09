//! Integration tests: run eval scenarios against a real TCP-bound pylon instance.

use dokimion::runner::{RunConfig, ScenarioRunner};
use hermeneus::test_utils::MockProvider;
use integration_tests::harness::TestHarness;
use koina::secret::SecretString;
use organon::testing::install_crypto_provider;

async fn start_test_server() -> (String, String, TestHarness) {
    install_crypto_provider();
    TestHarness::build_with_provider(Box::new(
        MockProvider::new("Hello from eval harness!").models(&["mock-model"]),
    ))
    .await
    .start_tcp_server()
    .await
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
