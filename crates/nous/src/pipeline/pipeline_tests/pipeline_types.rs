use super::*;

#[test]
fn guard_result_equality() {
    assert_eq!(
        GuardResult::Allow,
        GuardResult::Allow,
        "Allow should equal Allow"
    );
    assert_ne!(
        GuardResult::Allow,
        GuardResult::Rejected {
            reason: "test".to_owned()
        },
        "Allow should not equal Rejected"
    );
}

#[test]
fn turn_usage_total() {
    let usage = TurnUsage {
        input_tokens: 1000,
        output_tokens: 500,
        cache_read_tokens: 800,
        cache_write_tokens: 200,
        llm_calls: 3,
    };
    assert_eq!(usage.total_tokens(), 1500, "total should be input + output");
}

#[test]
fn interaction_signal_serde() {
    let signal = InteractionSignal::CodeGeneration;
    let json = serde_json::to_string(&signal).expect("serialize signal");
    assert_eq!(
        json, "\"code_generation\"",
        "signal should serialize to snake_case"
    );
    let back: InteractionSignal = serde_json::from_str(&json).expect("deserialize signal");
    assert_eq!(back, signal, "roundtrip should preserve signal");
}

#[test]
fn pipeline_context_default() {
    let ctx = PipelineContext::default();
    assert!(
        ctx.system_prompt.is_none(),
        "default system_prompt should be None"
    );
    assert!(ctx.messages.is_empty(), "default messages should be empty");
    assert!(
        !ctx.needs_distillation,
        "default needs_distillation should be false"
    );
    assert_eq!(
        ctx.guard_result,
        GuardResult::Allow,
        "default guard should be Allow"
    );
    assert!(
        ctx.working_state.is_none(),
        "default working_state should be None"
    );
}

#[test]
fn guard_result_rate_limited() {
    let g = GuardResult::RateLimited {
        retry_after_ms: 5000,
    };
    assert_ne!(g, GuardResult::Allow, "RateLimited should not equal Allow");
    match g {
        GuardResult::RateLimited { retry_after_ms } => {
            assert_eq!(retry_after_ms, 5000, "retry_after_ms should match");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn guard_result_loop_detected() {
    let g = GuardResult::LoopDetected {
        pattern: "exec:abc".to_owned(),
    };
    match g {
        GuardResult::LoopDetected { pattern } => {
            assert_eq!(pattern, "exec:abc", "pattern should match");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn guard_result_rejected() {
    let g = GuardResult::Rejected {
        reason: "unsafe content".to_owned(),
    };
    match g {
        GuardResult::Rejected { reason } => {
            assert!(reason.contains("unsafe"), "reason should contain unsafe");
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn all_interaction_signals_serde_roundtrip() {
    let signals = [
        InteractionSignal::Conversation,
        InteractionSignal::ToolExecution,
        InteractionSignal::CodeGeneration,
        InteractionSignal::Research,
        InteractionSignal::Planning,
        InteractionSignal::ErrorRecovery,
    ];
    for signal in signals {
        let json = serde_json::to_string(&signal).expect("serialize signal");
        let back: InteractionSignal = serde_json::from_str(&json).expect("deserialize signal");
        assert_eq!(
            signal, back,
            "serde roundtrip should preserve signal variant"
        );
    }
}

#[test]
fn turn_usage_default_is_zero() {
    let usage = TurnUsage::default();
    assert_eq!(
        usage.total_tokens(),
        0,
        "default total tokens should be zero"
    );
    assert_eq!(usage.llm_calls, 0, "default llm_calls should be zero");
}

#[test]
fn turn_usage_serde_roundtrip() {
    let usage = TurnUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_read_tokens: 80,
        cache_write_tokens: 20,
        llm_calls: 2,
    };
    let json = serde_json::to_string(&usage).expect("serialize usage");
    let back: TurnUsage = serde_json::from_str(&json).expect("deserialize usage");
    assert_eq!(
        usage.total_tokens(),
        back.total_tokens(),
        "roundtrip should preserve total tokens"
    );
}

#[tokio::test]
async fn assemble_context_populates_pipeline() {
    use std::fs;

    use tempfile::TempDir;

    use aletheia_taxis::oikos::Oikos;

    use crate::config::{NousConfig, PipelineConfig};

    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("nous/test-agent")).expect("create nous dir");
    fs::create_dir_all(root.join("shared")).expect("create shared dir");
    fs::create_dir_all(root.join("theke")).expect("create theke dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("nous/test-agent/SOUL.md"), "I am a test agent.").expect("write SOUL.md");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("theke/USER.md"), "Test user.").expect("write USER.md");

    let oikos = Oikos::from_root(root);
    let nous_config = NousConfig {
        id: "test-agent".to_owned(),
        ..NousConfig::default()
    };
    let pipeline_config = PipelineConfig::default();
    let mut ctx = PipelineContext::default();

    assemble_context(&oikos, &nous_config, &pipeline_config, &mut ctx)
        .await
        .expect("assemble_context should succeed");

    assert!(
        ctx.system_prompt.is_some(),
        "system prompt should be populated"
    );
    let prompt = ctx.system_prompt.expect("system prompt present");
    assert!(
        prompt.contains("I am a test agent."),
        "prompt should contain SOUL.md content"
    );
    assert!(
        prompt.contains("Test user."),
        "prompt should contain USER.md content"
    );
    assert!(
        ctx.remaining_tokens > 0,
        "remaining tokens should be positive"
    );
}

#[tokio::test]
async fn run_pipeline_simple() {
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, RwLock};

    use tempfile::TempDir;

    use aletheia_hermeneus::provider::ProviderRegistry;
    use aletheia_hermeneus::test_utils::MockProvider;
    use aletheia_koina::id::{NousId, SessionId};
    use aletheia_organon::registry::ToolRegistry;
    use aletheia_organon::types::ToolContext;

    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("nous/test-agent")).expect("create nous dir");
    fs::create_dir_all(root.join("shared")).expect("create shared dir");
    fs::create_dir_all(root.join("theke")).expect("create theke dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("nous/test-agent/SOUL.md"), "I am a test agent.").expect("write SOUL.md");

    let oikos = Oikos::from_root(root);
    let nous_config = NousConfig {
        id: "test-agent".to_owned(),
        generation: crate::config::NousGenerationConfig {
            model: "test-model".to_owned(),
            ..crate::config::NousGenerationConfig::default()
        },
        ..NousConfig::default()
    };
    let pipeline_config = PipelineConfig::default();

    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(
        MockProvider::new("Hello from pipeline!").models(&["test-model"]),
    ));

    let tools = ToolRegistry::new();
    let tool_ctx = ToolContext {
        nous_id: NousId::new("test-agent").expect("valid"),
        session_id: SessionId::new(),
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
    };

    let session = crate::session::SessionState::new(
        "test-session".to_owned(),
        "main".to_owned(),
        &nous_config,
    );
    let input = PipelineInput {
        content: "Hello".to_owned(),
        session,
        config: pipeline_config.clone(),
    };

    let result = run_pipeline(
        input,
        &oikos,
        &nous_config,
        &pipeline_config,
        &providers,
        &tools,
        &tool_ctx,
        None,
        None,
        None,
        None,
        Vec::new(),
        None,
        None,
    )
    .await
    .expect("pipeline should succeed");

    assert_eq!(
        result.content, "Hello from pipeline!",
        "pipeline should return mock response"
    );
    assert!(
        result.tool_calls.is_empty(),
        "simple pipeline should have no tool calls"
    );
    assert_eq!(
        result.usage.llm_calls, 1,
        "should have exactly one LLM call"
    );
    assert_eq!(
        result.stop_reason, "end_turn",
        "stop reason should be end_turn"
    );
}

#[tokio::test]
async fn assemble_context_missing_soul_returns_error() {
    use tempfile::TempDir;

    use aletheia_taxis::oikos::Oikos;

    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("nous/test-agent")).expect("create nous dir");
    std::fs::create_dir_all(root.join("shared")).expect("create shared dir");
    std::fs::create_dir_all(root.join("theke")).expect("create theke dir");

    let oikos = Oikos::from_root(root);
    let config = crate::config::NousConfig {
        id: "test-agent".to_owned(),
        ..crate::config::NousConfig::default()
    };
    let pipeline_config = crate::config::PipelineConfig::default();
    let mut ctx = PipelineContext::default();

    let err = assemble_context(&oikos, &config, &pipeline_config, &mut ctx).await;
    assert!(err.is_err(), "missing SOUL.md should produce error");
    let msg = err.expect_err("should be error").to_string();
    assert!(msg.contains("SOUL.md"), "got: {msg}");
}

#[test]
fn pipeline_message_serde_roundtrip() {
    let msg = PipelineMessage {
        role: "user".to_owned(),
        content: "Hello world".to_owned(),
        token_estimate: 3,
    };
    let json = serde_json::to_string(&msg).expect("serialize message");
    let back: PipelineMessage = serde_json::from_str(&json).expect("deserialize message");
    assert_eq!(msg.role, back.role, "role should roundtrip");
    assert_eq!(msg.content, back.content, "content should roundtrip");
    assert_eq!(
        msg.token_estimate, back.token_estimate,
        "token_estimate should roundtrip"
    );
}

#[test]
fn tool_call_serde_roundtrip() {
    let tc = ToolCall {
        id: "tc-1".to_owned(),
        name: "exec".to_owned(),
        input: serde_json::json!({"cmd": "ls"}),
        result: Some("output".to_owned()),
        is_error: false,
        duration_ms: 42,
    };
    let json = serde_json::to_string(&tc).expect("serialize tool call");
    let back: ToolCall = serde_json::from_str(&json).expect("deserialize tool call");
    assert_eq!(tc.id, back.id, "id should roundtrip");
    assert_eq!(tc.name, back.name, "name should roundtrip");
    assert_eq!(
        tc.duration_ms, back.duration_ms,
        "duration_ms should roundtrip"
    );
}

#[test]
fn tool_call_with_error() {
    let tc = ToolCall {
        id: "tc-1".to_owned(),
        name: "exec".to_owned(),
        input: serde_json::json!({}),
        result: None,
        is_error: true,
        duration_ms: 0,
    };
    assert!(tc.is_error, "error tool call should have is_error=true");
    assert!(tc.result.is_none(), "error tool call should have no result");
}

#[test]
fn check_guard_always_allows() {
    let config = crate::config::NousConfig::default();
    let session = crate::session::SessionState::new("s-1".to_owned(), "main".to_owned(), &config);
    assert_eq!(
        check_guard(&session, &config),
        GuardResult::Allow,
        "guard should always allow"
    );
}

#[test]
fn turn_usage_cache_tokens_not_counted_in_total() {
    let usage = TurnUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_read_tokens: 80,
        cache_write_tokens: 20,
        llm_calls: 1,
    };
    assert_eq!(
        usage.total_tokens(),
        150,
        "cache tokens should not be in total"
    );
}
