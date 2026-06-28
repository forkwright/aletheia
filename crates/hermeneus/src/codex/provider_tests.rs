use super::*;
use crate::types::{CompletionRequest, Content, ContentBlock, Message, Role, ToolResultContent};

fn retry_test_script_path(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};

    static NONCE: AtomicU64 = AtomicU64::new(0);
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "hermeneus_codex_provider_{name}_{}_{nonce}.sh",
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
    let prompt = CodexProvider::format_prompt(&request);
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
    let prompt = CodexProvider::format_prompt(&request);
    assert!(prompt.contains("User: What is 2+2?"));
    assert!(prompt.contains("Assistant: 4"));
    assert!(prompt.contains("User: And 3+3?"));
}

// WHY(#3980): ToolUse blocks must be rendered so assistant turns with tool
// calls do not disappear from the Codex prompt.
#[test]
fn extract_text_content_renders_tool_use_blocks() {
    let content = Content::Blocks(vec![
        ContentBlock::Text {
            text: "Let me check that.".to_owned(),
            citations: None,
        },
        ContentBlock::ToolUse {
            id: "toolu_01".to_owned(),
            name: "read_file".to_owned(),
            input: serde_json::json!({"path": "/etc/hosts"}),
        },
    ]);
    let text = extract_text_content(&content);
    assert!(
        text.contains("Let me check that."),
        "text block must be present: {text}"
    );
    assert!(
        text.contains("[Tool call: read_file("),
        "tool-use block must be rendered, not dropped: {text}"
    );
    assert!(
        text.contains("/etc/hosts"),
        "tool input must appear in rendered marker: {text}"
    );
}

// WHY(#3980): tool-use assistant turns must round-trip through format_prompt
// so a later tool-result still has matching call context.
#[test]
fn format_prompt_preserves_tool_use_turns() {
    let request = CompletionRequest {
        messages: vec![
            Message {
                role: Role::User,
                content: Content::Text("What is in /etc/hosts?".to_owned()),
                cache_breakpoint: false,
            },
            Message {
                role: Role::Assistant,
                content: Content::Blocks(vec![
                    ContentBlock::Text {
                        text: "I will read the file.".to_owned(),
                        citations: None,
                    },
                    ContentBlock::ToolUse {
                        id: "toolu_01".to_owned(),
                        name: "read_file".to_owned(),
                        input: serde_json::json!({"path": "/etc/hosts"}),
                    },
                ]),
                cache_breakpoint: false,
            },
            Message {
                role: Role::User,
                content: Content::Blocks(vec![ContentBlock::ToolResult {
                    tool_use_id: "toolu_01".to_owned(),
                    content: ToolResultContent::text("127.0.0.1 localhost"),
                    is_error: None,
                }]),
                cache_breakpoint: false,
            },
        ],
        ..Default::default()
    };
    let prompt = CodexProvider::format_prompt(&request);
    // All three turns must appear.
    assert!(
        prompt.contains("User: What is in /etc/hosts?"),
        "first user turn missing: {prompt}"
    );
    assert!(
        prompt.contains("I will read the file."),
        "assistant text missing: {prompt}"
    );
    assert!(
        prompt.contains("[Tool call: read_file("),
        "tool-use marker missing: {prompt}"
    );
    assert!(
        prompt.contains("127.0.0.1 localhost"),
        "tool result missing: {prompt}"
    );
}

#[test]
fn resolve_model_strips_prefix() {
    let provider = CodexProvider {
        name: "codex".to_owned(),
        codex_binary: PathBuf::from("codex"),
        working_directory: None,
        models: Vec::new(),
        default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
        timeout: Duration::from_secs(1),
        deployment_target: DeploymentTarget::Cloud,
        circuit_breaker: CircuitBreaker::with_defaults("codex"),
    };
    assert_eq!(
        provider.resolve_model(&format!(
            "{CODEX_MODEL_PREFIX}{}",
            koina::models::names::codex()
        )),
        koina::models::names::codex()
    );
    assert_eq!(provider.resolve_model(""), koina::models::names::codex());
}

#[test]
fn supports_model_with_prefix() {
    let provider = CodexProvider {
        name: "codex".to_owned(),
        codex_binary: PathBuf::from("codex"),
        working_directory: None,
        models: Vec::new(),
        default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
        timeout: Duration::from_secs(1),
        deployment_target: DeploymentTarget::Cloud,
        circuit_breaker: CircuitBreaker::with_defaults("codex"),
    };
    assert!(provider.supports_model(&format!(
        "{CODEX_MODEL_PREFIX}{}",
        koina::models::names::codex()
    )));
    assert!(provider.supports_model(koina::models::names::codex()));
    assert!(!provider.supports_model("claude-sonnet-4-6"));
}

#[test]
fn match_specificity_prefers_prefix_and_exact() {
    let provider = CodexProvider {
        name: "codex".to_owned(),
        codex_binary: PathBuf::from("codex"),
        working_directory: None,
        models: Vec::new(),
        default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
        timeout: Duration::from_secs(1),
        deployment_target: DeploymentTarget::Cloud,
        circuit_breaker: CircuitBreaker::with_defaults("codex"),
    };
    assert_eq!(
        provider.match_specificity(&format!(
            "{CODEX_MODEL_PREFIX}{}",
            koina::models::names::codex()
        )),
        Some(MatchKind::Prefix)
    );
    assert_eq!(
        provider.match_specificity(koina::models::names::codex()),
        Some(MatchKind::Exact)
    );
    assert_eq!(provider.match_specificity("claude-sonnet-4-6"), None);
}

#[test]
fn configured_models_are_exact_claims() {
    let provider = CodexProvider {
        name: "codex-seat".to_owned(),
        codex_binary: PathBuf::from("codex"),
        working_directory: None,
        models: vec!["team-codex".to_owned()],
        default_model: "team-codex".to_owned(),
        timeout: Duration::from_secs(1),
        deployment_target: DeploymentTarget::Cloud,
        circuit_breaker: CircuitBreaker::with_defaults("codex-seat"),
    };

    assert_eq!(
        provider.match_specificity("team-codex"),
        Some(MatchKind::Exact)
    );
    assert_eq!(
        provider.match_specificity("codex/gpt-5-codex"),
        Some(MatchKind::Prefix)
    );
    assert_eq!(
        provider.match_specificity(koina::models::names::codex()),
        None
    );
    assert_eq!(provider.name(), "codex-seat");
}

#[test]
fn codex_provider_reports_cloud_deployment_target() {
    let provider = CodexProvider {
        name: "codex".to_owned(),
        codex_binary: PathBuf::from("codex"),
        working_directory: None,
        models: Vec::new(),
        default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
        timeout: Duration::from_secs(1),
        deployment_target: DeploymentTarget::Cloud,
        circuit_breaker: CircuitBreaker::with_defaults("codex"),
    };
    assert_eq!(provider.deployment_target(), DeploymentTarget::Cloud);
}

#[test]
fn codex_provider_supports_streaming() {
    let provider = CodexProvider {
        name: "codex".to_owned(),
        codex_binary: PathBuf::from("codex"),
        working_directory: None,
        models: Vec::new(),
        default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
        timeout: Duration::from_secs(1),
        deployment_target: DeploymentTarget::Cloud,
        circuit_breaker: CircuitBreaker::with_defaults("codex"),
    };
    assert!(
        provider.supports_streaming(),
        "CodexProvider must report supports_streaming=true after #3980"
    );
}

#[tokio::test]
async fn retries_fluke_spawn_failure_before_returning_error() {
    const SCRIPT: &str = r#"cat > /dev/null
printf '{"type":"item.completed","item":{"type":"agent_message","text":"retried ok"}}\n'
printf '{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1}}\n'"#;

    let script_path = retry_test_script_path("spawn_retry");
    write_executable_script(&script_path, SCRIPT).expect("write script");
    let provider = CodexProvider::new(&CodexProviderConfig {
        codex_binary: Some(script_path.clone()),
        timeout: Duration::from_secs(5),
        ..CodexProviderConfig::default()
    })
    .expect("provider init");
    remove_if_exists(&script_path).expect("remove script");

    let restore_path = script_path.clone();
    let restore = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await; // kanon:ignore TESTING/sleep-in-test WHY: tests real subprocess retry timing; cannot use deterministic mock
        write_executable_script(&restore_path, SCRIPT)
    });

    let request = one_message_request(format!(
        "{CODEX_MODEL_PREFIX}{}",
        koina::models::names::codex()
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

#[test]
fn seat_bridged_fields() {
    let provider = CodexProvider {
        name: "codex".to_owned(),
        codex_binary: PathBuf::from("/usr/local/bin/codex"),
        working_directory: None,
        models: Vec::new(),
        default_model: format!("{CODEX_MODEL_PREFIX}{}", koina::models::names::codex()),
        timeout: Duration::from_mins(5),
        deployment_target: DeploymentTarget::Cloud,
        circuit_breaker: CircuitBreaker::with_defaults("codex"),
    };
    assert_eq!(
        provider.cli_binary(),
        &PathBuf::from("/usr/local/bin/codex")
    );
    assert_eq!(provider.subprocess_timeout(), Duration::from_mins(5));
    assert_eq!(provider.cli_product_name(), "codex");
}

#[test]
fn warns_once_for_dropped_tools() {
    assert!(!CodexProvider::warn_dropped_tools(0));
    assert!(CodexProvider::warn_dropped_tools(1));
    assert!(!CodexProvider::warn_dropped_tools(2));
}

#[test]
fn records_cache_metrics_from_response() {
    use koina::metrics::MetricsRegistry;

    use crate::metrics::register;
    use crate::types::{CompletionResponse, ContentBlock, StopReason, Usage};

    let r = MetricsRegistry::new();
    r.with_registry(register);

    let response = CompletionResponse {
        id: "codex_1".to_owned(),
        model: "codex".to_owned(),
        stop_reason: StopReason::EndTurn,
        content: vec![ContentBlock::Text {
            text: "hi".to_owned(),
            citations: None,
        }],
        usage: Usage {
            input_tokens: 20,
            output_tokens: 10,
            cache_read_tokens: 5,
            cache_write_tokens: 0,
        },
        cost_usd: None,
        duration_ms: None,
    };
    crate::metrics::record_cache_tokens(
        "codex",
        response.usage.cache_read_tokens,
        response.usage.cache_write_tokens,
    );

    let mut buf = String::new();
    #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
    r.encode(&mut buf).unwrap();
    assert!(
        buf.contains("aletheia_llm_cache_tokens_total{provider=\"codex\",direction=\"read\"} 5"),
        "missing cache read metrics: {buf}"
    );
    // WHY: Codex only reports cache reads; write direction must be absent.
    assert!(
        !buf.contains("aletheia_llm_cache_tokens_total{provider=\"codex\",direction=\"write\"}"),
        "codex must not emit zero cache write metrics: {buf}"
    );
}
