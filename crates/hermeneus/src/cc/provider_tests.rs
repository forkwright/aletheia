use super::*;
use crate::types::{CompletionRequest, Content, Message, Role};

fn retry_test_script_path(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "hermeneus_cc_provider_{name}_{}_{nonce}.sh",
        std::process::id()
    ))
}

fn write_executable_script(path: &Path, body: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::PermissionsExt as _;

    let script = format!("#!/bin/sh\n{body}\n");
    let mut file = std::fs::File::create(path)?;
    file.write_all(script.as_bytes())?;
    file.sync_all()?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

fn remove_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn one_message_request(model: String) -> CompletionRequest {
    CompletionRequest {
        model,
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hello".to_owned()),
            cache_breakpoint: false,
        }],
        ..Default::default()
    }
}

fn assert_text_response(response: &CompletionResponse, expected: &str) {
    assert!(
        response
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Text { text, .. } if text == expected)),
        "response should contain text {expected:?}: {response:?}"
    );
}

#[test]
fn format_prompt_single_message() {
    let request = CompletionRequest {
        messages: vec![Message {
            role: Role::User,
            content: Content::Text("hello world".to_owned()),
            cache_breakpoint: false,
        }],
        ..Default::default()
    };
    let prompt = CcProvider::format_prompt(&request);
    assert_eq!(prompt, "hello world");
}

#[test]
fn format_prompt_multi_turn() {
    let request = CompletionRequest {
        messages: vec![
            Message {
                role: Role::User,
                content: Content::Text("What is 2+2?".to_owned()),
                cache_breakpoint: false,
            },
            Message {
                role: Role::Assistant,
                content: Content::Text("4".to_owned()),
                cache_breakpoint: false,
            },
            Message {
                role: Role::User,
                content: Content::Text("And 3+3?".to_owned()),
                cache_breakpoint: false,
            },
        ],
        ..Default::default()
    };
    let prompt = CcProvider::format_prompt(&request);
    assert!(prompt.contains("Human: What is 2+2?"));
    assert!(prompt.contains("Assistant: 4"));
    assert!(prompt.contains("Human: And 3+3?"));
}

#[test]
fn resolve_model_strips_prefix() {
    let model = format!("{CC_MODEL_PREFIX}{}", crate::models::names::sonnet());
    let stripped = model
        .strip_prefix(CC_MODEL_PREFIX)
        .unwrap_or(model.as_str());
    assert_eq!(stripped, crate::models::names::sonnet());
}

#[test]
fn supports_model_with_prefix() {
    let model = format!("{CC_MODEL_PREFIX}{}", crate::models::names::sonnet());
    assert!(model.starts_with(CC_MODEL_PREFIX));
}

#[test]
fn supports_model_known() {
    let provider = CcProvider {
        name: "cc".to_owned(),
        cc_binary: PathBuf::from("claude"),
        working_directory: None,
        models: Vec::new(),
        default_model: crate::models::names::opus().to_owned(),
        timeout: Duration::from_secs(1),
        deployment_target: DeploymentTarget::Cloud,
        circuit_breaker: CircuitBreaker::with_defaults("cc"),
    };
    assert!(provider.supports_model(crate::models::names::sonnet()));
    assert!(provider.supports_model("claude-future-family-model"));
    assert!(!provider.supports_model("gpt-4"));
}

#[test]
fn configured_models_are_exact_claims() {
    let provider = CcProvider {
        name: "cc-seat".to_owned(),
        cc_binary: PathBuf::from("claude"),
        working_directory: None,
        models: vec!["team-claude".to_owned()],
        default_model: "team-claude".to_owned(),
        timeout: Duration::from_secs(1),
        deployment_target: DeploymentTarget::Cloud,
        circuit_breaker: CircuitBreaker::with_defaults("cc-seat"),
    };

    assert_eq!(
        provider.match_specificity("team-claude"),
        Some(MatchKind::Exact)
    );
    assert_eq!(
        provider.match_specificity("cc/claude-opus-4-6"),
        Some(MatchKind::Prefix)
    );
    assert_eq!(
        provider.match_specificity("claude-future-family-model"),
        None
    );
    assert_eq!(provider.name(), "cc-seat");
}

#[test]
fn cc_provider_reports_cloud_deployment_target() {
    let provider = CcProvider {
        name: "cc".to_owned(),
        cc_binary: PathBuf::from("claude"),
        working_directory: None,
        models: Vec::new(),
        default_model: crate::models::names::opus().to_owned(),
        timeout: Duration::from_secs(1),
        deployment_target: DeploymentTarget::Cloud,
        circuit_breaker: CircuitBreaker::with_defaults("cc"),
    };
    assert_eq!(provider.deployment_target(), DeploymentTarget::Cloud);
}

#[test]
fn seat_bridged_fields() {
    let provider = CcProvider {
        name: "cc".to_owned(),
        cc_binary: PathBuf::from("/usr/local/bin/claude"),
        working_directory: None,
        models: Vec::new(),
        default_model: crate::models::names::opus().to_owned(),
        timeout: Duration::from_mins(5),
        deployment_target: DeploymentTarget::Cloud,
        circuit_breaker: CircuitBreaker::with_defaults("cc"),
    };
    assert_eq!(
        provider.cli_binary(),
        &PathBuf::from("/usr/local/bin/claude")
    );
    assert_eq!(provider.subprocess_timeout(), Duration::from_mins(5));
    assert_eq!(provider.cli_product_name(), "claude");
}

#[test]
fn warns_once_for_dropped_tools() {
    assert!(!CcProvider::warn_dropped_tools(0));
    assert!(CcProvider::warn_dropped_tools(1));
    assert!(!CcProvider::warn_dropped_tools(2));
}

#[tokio::test]
async fn retries_fluke_spawn_failure_before_returning_error() {
    const SCRIPT: &str = r#"cat > /dev/null
printf '{"type":"result","subtype":"success","result":"retried ok","is_error":false}\n'"#;

    let script_path = retry_test_script_path("spawn_retry");
    write_executable_script(&script_path, SCRIPT).expect("write script");
    let provider = CcProvider::new(&CcProviderConfig {
        cc_binary: Some(script_path.clone()),
        timeout: Duration::from_secs(5),
        ..CcProviderConfig::default()
    })
    .expect("provider init");
    remove_if_exists(&script_path).expect("remove script");

    let restore_path = script_path.clone();
    let restore = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await; // kanon:ignore TESTING/sleep-in-test WHY: tests real subprocess retry timing; cannot use deterministic mock
        write_executable_script(&restore_path, SCRIPT)
    });

    let request = one_message_request(format!(
        "{CC_MODEL_PREFIX}{}",
        crate::models::names::sonnet()
    ));
    let result = provider.execute(&request).await;
    restore
        .await
        .expect("restore task join")
        .expect("restore script write");
    let response = result.expect("provider execute");

    assert_text_response(&response, "retried ok");
    remove_if_exists(&script_path).expect("cleanup");
}

#[tokio::test]
async fn live_provider_failure_records_circuit_transition_metric() {
    use koina::metrics::MetricsRegistry;

    use crate::circuit_breaker::CircuitBreakerConfig;
    use crate::metrics::register;

    let registry = MetricsRegistry::new();
    registry.with_registry(register);
    let provider = CcProvider {
        name: "cc-circuit-test".to_owned(),
        cc_binary: retry_test_script_path("missing_binary"),
        working_directory: None,
        models: Vec::new(),
        default_model: crate::models::names::opus().to_owned(),
        timeout: Duration::from_secs(1),
        deployment_target: DeploymentTarget::Cloud,
        circuit_breaker: CircuitBreaker::new(
            "cc-circuit-test",
            CircuitBreakerConfig {
                failure_threshold: 1,
                ..CircuitBreakerConfig::default()
            },
        ),
    };
    let request = one_message_request(format!(
        "{CC_MODEL_PREFIX}{}",
        crate::models::names::sonnet()
    ));

    let result = provider.execute(&request).await;
    assert!(
        result.is_err(),
        "missing binary should fail the provider call"
    );

    let mut buf = String::new();
    #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
    registry.encode(&mut buf).unwrap();
    assert!(
        buf.contains("aletheia_llm_circuit_breaker_transitions_total{provider=\"cc-circuit-test\",from=\"closed\",to=\"open\"} 1"),
        "missing circuit transition metric: {buf}"
    );
}

#[test]
fn records_cache_metrics_from_response() {
    use koina::metrics::MetricsRegistry;

    use crate::metrics::register;
    use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

    let r = MetricsRegistry::new();
    r.with_registry(register);

    let response = CompletionResponse {
        id: "cc_1".to_owned(),
        model: "claude-sonnet-4-6".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: "hi".to_owned(),
            citations: None,
        }],
        usage: Usage {
            input_tokens: 20,
            output_tokens: 10,
            cache_read_tokens: 5,
            cache_write_tokens: 2,
        },
        cost_usd: None,
        duration_ms: None,
    };
    crate::metrics::record_cache_tokens(
        "cc",
        response.usage.cache_read_tokens,
        response.usage.cache_write_tokens,
    );

    let mut buf = String::new();
    #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
    r.encode(&mut buf).unwrap();
    assert!(
        buf.contains("aletheia_llm_cache_tokens_total{provider=\"cc\",direction=\"read\"} 5"),
        "missing cache read metrics: {buf}"
    );
    assert!(
        buf.contains("aletheia_llm_cache_tokens_total{provider=\"cc\",direction=\"write\"} 2"),
        "missing cache write metrics: {buf}"
    );
}
