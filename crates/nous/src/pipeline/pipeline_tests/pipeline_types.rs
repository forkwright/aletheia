use std::sync::Arc;

use super::*;

#[test]
fn guard_result_equality() {
    let reason = "test".to_owned();
    let r1 = GuardResult::Rejected {
        reason: reason.clone(),
    };
    let r2 = GuardResult::Rejected { reason };
    assert_eq!(r1, r2, "Rejected with same reason should be equal");
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
    assert!(
        ctx.reflection_result.is_none(),
        "default reflection_result should be None"
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

    use taxis::oikos::Oikos;

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
        id: Arc::from("test-agent"),
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
async fn assemble_context_conditional_turn_one_selects_cold_start() {
    use std::fs;

    use tempfile::TempDir;

    use taxis::oikos::Oikos;

    use crate::bootstrap::TaskHint;
    use crate::config::{NousConfig, PipelineConfig};

    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    fs::create_dir_all(root.join("nous/test-agent")).expect("create nous dir");
    fs::create_dir_all(root.join("shared")).expect("create shared dir");
    fs::create_dir_all(root.join("theke")).expect("create theke dir");
    fs::create_dir_all(root.join("_llm")).expect("create _llm dir");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("nous/test-agent/SOUL.md"), "I am a test agent.").expect("write SOUL.md");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("_llm/l1-context.md"), {
        let mut s = String::from("cold start l1 signal: ");
        s.push_str(&"word ".repeat(200));
        s
    })
    .expect("write L1 context");
    #[expect(
        clippy::disallowed_methods,
        reason = "nous bootstrap and test setup writes configuration files to temp directories; synchronous I/O is required in test contexts"
    )]
    fs::write(root.join("_llm/manifest.toml"), "").expect("write manifest.toml");

    let oikos = Oikos::from_root(root);
    let mut config = NousConfig {
        id: Arc::from("test-agent"),
        ..NousConfig::default()
    };
    // WHY: keep the bootstrap cap small so the large L1 section only fits when
    // it is Required (cold start). In-session turns should drop it as Optional.
    config.generation.bootstrap_max_tokens = 20;
    let pipeline_config = PipelineConfig::default();

    let mut cold_ctx = PipelineContext::default();
    assemble_context_conditional(
        &oikos,
        &config,
        &pipeline_config,
        &mut cold_ctx,
        Vec::new(),
        TaskHint::General,
        1,
    )
    .await
    .expect("turn 1 context assembly should succeed");
    let cold_prompt = cold_ctx.system_prompt.expect("system prompt present");
    assert!(
        cold_prompt.contains("cold start l1 signal"),
        "turn 1 (cold start) should keep L1 content as Required"
    );

    let mut warm_ctx = PipelineContext::default();
    assemble_context_conditional(
        &oikos,
        &config,
        &pipeline_config,
        &mut warm_ctx,
        Vec::new(),
        TaskHint::General,
        2,
    )
    .await
    .expect("turn 2 context assembly should succeed");
    let warm_prompt = warm_ctx.system_prompt.expect("system prompt present");
    assert!(
        !warm_prompt.contains("cold start l1 signal"),
        "turn 2 (in session) should drop L1 content as Optional under budget pressure"
    );
}

#[tokio::test]
async fn run_pipeline_simple() {
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, RwLock};

    use tempfile::TempDir;

    use hermeneus::provider::ProviderRegistry;
    use hermeneus::test_utils::MockProvider;
    use koina::id::{NousId, SessionId};
    use organon::registry::ToolRegistry;
    use organon::types::ToolContext;

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
        id: Arc::from("test-agent"),
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
        turn_number: 0,
        workspace: PathBuf::from("/tmp/test"),
        allowed_roots: vec![PathBuf::from("/tmp")],
        services: None,
        active_tools: Arc::new(RwLock::new(HashSet::new())),
        tool_config: Arc::new(taxis::config::ToolLimitsConfig::default()),
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
        None,
        Vec::new(),
        None,
        None,
        None,
        None,
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

    use taxis::oikos::Oikos;

    let dir = TempDir::new().expect("create temp dir");
    let root = dir.path();
    std::fs::create_dir_all(root.join("nous/test-agent")).expect("create nous dir");
    std::fs::create_dir_all(root.join("shared")).expect("create shared dir");
    std::fs::create_dir_all(root.join("theke")).expect("create theke dir");

    let oikos = Oikos::from_root(root);
    let config = crate::config::NousConfig {
        id: Arc::from("test-agent"),
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
    let msg = PipelineMessage::text("user", "Hello world", 3);
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
        approval: None,
        receipt: None,
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
        approval: None,
        receipt: None,
    };
    assert!(tc.is_error, "error tool call should have is_error=true");
    assert!(tc.result.is_none(), "error tool call should have no result");
}

#[test]
fn check_guard_allows_below_session_token_cap() {
    let config = crate::config::NousConfig::default();
    let session = crate::session::SessionState::new("s-1".to_owned(), "main".to_owned(), &config);
    assert_eq!(check_guard(&session, &config), GuardResult::Allow);
}

#[test]
fn check_guard_rejects_at_session_token_cap() {
    let mut config = crate::config::NousConfig::default();
    config.limits.session_token_cap = 10;
    let mut session =
        crate::session::SessionState::new("s-1".to_owned(), "main".to_owned(), &config);
    session.cumulative_tokens = 10;

    assert!(matches!(
        check_guard(&session, &config),
        GuardResult::Rejected { reason } if reason.contains("token budget exhausted")
    ));
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
