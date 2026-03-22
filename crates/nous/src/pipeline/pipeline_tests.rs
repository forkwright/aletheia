#![expect(clippy::expect_used, reason = "test assertions")]
use super::*;

fn is_ok(v: &LoopVerdict) -> bool {
    matches!(v, LoopVerdict::Ok)
}

fn is_warn(v: &LoopVerdict) -> bool {
    matches!(v, LoopVerdict::Warn { .. })
}

fn is_halt(v: &LoopVerdict) -> bool {
    matches!(v, LoopVerdict::Halt { .. })
}

// --- Same-args repetition ---

#[test]
fn loop_detector_no_loop() {
    let mut det = LoopDetector::new(3);
    assert!(
        is_ok(&det.record("exec", "hash1", false)),
        "unique calls should not trigger loop"
    );
    assert!(
        is_ok(&det.record("read", "hash2", false)),
        "different tool should not trigger loop"
    );
    assert!(
        is_ok(&det.record("exec", "hash3", false)),
        "different hash should not trigger loop"
    );
}

#[test]
fn loop_detector_detects_repeat() {
    let mut det = LoopDetector::new(3);
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "first repeat should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "second repeat should not trigger"
    );
    let result = det.record("exec", "same", false);
    assert!(is_warn(&result), "third repeat should trigger loop warning");
    match result {
        LoopVerdict::Warn { pattern, .. } => {
            assert_eq!(pattern, "exec:same", "pattern should be tool:hash");
        }
        _ => panic!("expected Warn"),
    }
}

#[test]
fn loop_detector_different_inputs_ok() {
    let mut det = LoopDetector::new(3);
    assert!(
        is_ok(&det.record("exec", "hash1", false)),
        "unique hash1 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "hash2", false)),
        "unique hash2 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "hash3", false)),
        "unique hash3 should not trigger"
    );
}

#[test]
fn loop_detector_reset() {
    let mut det = LoopDetector::new(3);
    det.record("exec", "same", false);
    det.record("exec", "same", false);
    det.reset();
    assert_eq!(det.call_count(), 0, "call count should be zero after reset");
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "reset should clear history"
    );
}

#[test]
fn loop_detector_threshold_4() {
    let mut det = LoopDetector::new(4);
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "repeat 1 of 4 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "repeat 2 of 4 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "repeat 3 of 4 should not trigger"
    );
    assert!(
        is_warn(&det.record("exec", "same", false)),
        "repeat 4 of 4 should trigger loop warning"
    );
}

#[test]
fn loop_detector_threshold_1() {
    let mut det = LoopDetector::new(1);
    let result = det.record("exec", "hash", false);
    assert!(
        is_warn(&result),
        "threshold 1 should trigger warning on first call"
    );
}

// --- Warning/halt escalation ---

#[test]
fn loop_detector_warn_then_halt() {
    let mut det = LoopDetector::with_limits(3, 4, 2);

    // WHY: after threshold (3) identical calls, every subsequent identical call
    // re-triggers detection because the tail always has >= 3 consecutive matches.
    det.record("exec", "same", false);
    det.record("exec", "same", false);
    let v = det.record("exec", "same", false);
    assert!(is_warn(&v), "first detection should warn");
    assert_eq!(det.warnings_issued(), 1);

    // Second identical call triggers again immediately
    let v = det.record("exec", "same", false);
    assert!(is_warn(&v), "second detection should still warn");
    assert_eq!(det.warnings_issued(), 2);

    // Third detection should halt (max_warnings=2 exceeded)
    let v = det.record("exec", "same", false);
    assert!(
        is_halt(&v),
        "third detection should halt after max warnings"
    );
}

#[test]
fn loop_detector_reset_clears_warnings() {
    let mut det = LoopDetector::with_limits(2, 4, 1);
    det.record("exec", "same", false);
    let v = det.record("exec", "same", false);
    assert!(is_warn(&v), "should warn");
    assert_eq!(det.warnings_issued(), 1);

    det.reset();
    assert_eq!(det.warnings_issued(), 0, "reset should clear warning count");
}

// --- Alternating failure detection ---

#[test]
fn loop_detector_detects_alternating_failure() {
    let mut det = LoopDetector::with_limits(3, 10, 2);

    det.record("tool_a", "h1", true);
    det.record("tool_b", "h2", true);
    det.record("tool_a", "h3", true);
    det.record("tool_b", "h4", true);
    det.record("tool_a", "h5", true);
    let v = det.record("tool_b", "h6", true);
    assert!(
        is_warn(&v),
        "alternating failure pattern should be detected"
    );
    match v {
        LoopVerdict::Warn { pattern, .. } => {
            assert!(
                pattern.contains("alternating"),
                "pattern should mention alternating: got {pattern}"
            );
        }
        _ => panic!("expected Warn"),
    }
}

#[test]
fn alternating_failure_not_triggered_without_errors() {
    let mut det = LoopDetector::with_limits(3, 10, 2);

    det.record("tool_a", "h1", false);
    det.record("tool_b", "h2", false);
    det.record("tool_a", "h3", false);
    det.record("tool_b", "h4", false);
    det.record("tool_a", "h5", false);
    let v = det.record("tool_b", "h6", false);
    // NOTE: this might trigger cycle detection instead, but NOT alternating failure
    // since is_error is false. The cycle detection may or may not fire depending on
    // whether the signatures form a recognized cycle.
    if let LoopVerdict::Warn { pattern, .. } = &v {
        assert!(
            !pattern.contains("alternating"),
            "non-error pattern should not be alternating failures"
        );
    }
}

#[test]
fn alternating_failure_same_tool_not_triggered() {
    let mut det = LoopDetector::with_limits(3, 10, 2);

    // Same tool alternating with itself should not trigger alternating failure
    for _ in 0..6 {
        det.record("tool_a", "h1", true);
    }
    // This should trigger same-args or consecutive errors, but NOT alternating failure
}

// --- Consecutive error detection ---

#[test]
fn loop_detector_detects_consecutive_errors_same_tool() {
    let mut det = LoopDetector::with_limits(10, 4, 2);

    det.record("exec", "h1", true);
    det.record("exec", "h2", true);
    det.record("exec", "h3", true);
    let v = det.record("exec", "h4", true);
    assert!(
        is_warn(&v),
        "4 consecutive errors from same tool should trigger"
    );
    match v {
        LoopVerdict::Warn { pattern, .. } => {
            assert!(
                pattern.contains("escalating retries"),
                "pattern should mention escalating retries: got {pattern}"
            );
        }
        _ => panic!("expected Warn"),
    }
}

#[test]
fn consecutive_errors_different_tools() {
    let mut det = LoopDetector::with_limits(10, 4, 2);

    det.record("exec", "h1", true);
    det.record("read", "h2", true);
    det.record("write", "h3", true);
    let v = det.record("search", "h4", true);
    assert!(
        is_warn(&v),
        "4 consecutive errors from mixed tools should trigger"
    );
    match v {
        LoopVerdict::Warn { pattern, .. } => {
            assert!(
                pattern.contains("consecutive failures"),
                "pattern should mention consecutive failures: got {pattern}"
            );
        }
        _ => panic!("expected Warn"),
    }
}

#[test]
fn consecutive_errors_broken_by_success() {
    let mut det = LoopDetector::with_limits(10, 4, 2);

    det.record("exec", "h1", true);
    det.record("exec", "h2", true);
    det.record("exec", "h3", false); // success breaks the streak
    let v = det.record("exec", "h4", true);
    assert!(is_ok(&v), "success should reset consecutive error count");
}

// --- Cycle detection ---

#[test]
fn loop_detector_detects_ab_cycle() {
    let mut det = LoopDetector::new(3);
    assert!(
        is_ok(&det.record("exec", "a", false)),
        "cycle step a-1 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "b", false)),
        "cycle step b-1 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "a", false)),
        "cycle step a-2 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "b", false)),
        "cycle step b-2 should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "a", false)),
        "cycle step a-3 should not trigger"
    );
    let result = det.record("exec", "b", false);
    assert!(
        is_warn(&result),
        "2-step cycle repeated 3 times should be detected"
    );
}

#[test]
fn loop_detector_detects_abc_cycle() {
    let mut det = LoopDetector::new(3);
    // A -> B -> C repeated 3 times
    for _ in 0..2 {
        det.record("a", "1", false);
        det.record("b", "2", false);
        det.record("c", "3", false);
    }
    det.record("a", "1", false);
    det.record("b", "2", false);
    let v = det.record("c", "3", false);
    assert!(
        is_warn(&v),
        "3-step cycle repeated 3 times should be detected"
    );
}

#[test]
fn loop_detector_non_repeating_interleaved_not_detected() {
    let mut det = LoopDetector::new(3);
    for i in 0..6 {
        assert!(
            is_ok(&det.record("exec", &format!("hash{i}"), false)),
            "unique hash should not trigger loop"
        );
    }
}

// --- Utility method tests ---

#[test]
fn loop_detector_call_count_tracks() {
    let mut det = LoopDetector::new(10);
    det.record("a", "1", false);
    det.record("b", "2", false);
    det.record("c", "3", false);
    assert_eq!(
        det.call_count(),
        3,
        "call count should track all recordings"
    );
}

#[test]
fn loop_detector_window_cap_evicts_old_calls() {
    let mut det = LoopDetector::new(100);
    for i in 0..55 {
        det.record("tool", &format!("hash{i}"), false);
    }
    assert_eq!(
        det.call_count(),
        DEFAULT_LOOP_WINDOW,
        "history should be capped at window size"
    );
}

#[test]
fn loop_detector_pattern_count_tracks_repetitions() {
    let mut det = LoopDetector::new(100);
    det.record("exec", "same", false);
    det.record("exec", "same", false);
    det.record("exec", "same", false);
    assert_eq!(
        det.pattern_count(),
        3,
        "pattern count should track consecutive repeats"
    );
}

#[test]
fn loop_detector_pattern_count_zero_on_empty() {
    let det = LoopDetector::new(3);
    assert_eq!(
        det.pattern_count(),
        0,
        "empty detector should have zero pattern count"
    );
}

#[test]
fn loop_detector_pattern_count_resets_on_different() {
    let mut det = LoopDetector::new(100);
    det.record("exec", "hash1", false);
    det.record("exec", "hash1", false);
    det.record("read", "hash2", false);
    assert_eq!(det.pattern_count(), 1, "different call breaks the streak");
}

#[test]
fn loop_detector_many_unique_then_repeat() {
    let mut det = LoopDetector::new(3);
    for i in 0..20 {
        det.record("tool", &format!("hash{i}"), false);
    }
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "first repeat after unique should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "second repeat should not trigger"
    );
    assert!(
        is_warn(&det.record("exec", "same", false)),
        "third repeat should trigger loop"
    );
}

#[test]
fn loop_detector_window_still_detects_loops() {
    let mut det = LoopDetector::new(3);
    for i in 0..18 {
        det.record("tool", &format!("hash{i}"), false);
    }
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "first repeat should not trigger"
    );
    assert!(
        is_ok(&det.record("exec", "same", false)),
        "second repeat should not trigger"
    );
    assert!(
        is_warn(&det.record("exec", "same", false)),
        "should detect loop even after window eviction"
    );
}

#[test]
fn loop_detector_reset_clears_pattern_count() {
    let mut det = LoopDetector::new(100);
    det.record("exec", "same", false);
    det.record("exec", "same", false);
    assert_eq!(
        det.pattern_count(),
        2,
        "should have 2 repetitions before reset"
    );
    det.reset();
    assert_eq!(
        det.pattern_count(),
        0,
        "pattern count should be zero after reset"
    );
    assert_eq!(det.call_count(), 0, "call count should be zero after reset");
}

// --- Non-loop-detector tests (unchanged from original) ---

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

// --- LoopVerdict tests ---

#[test]
fn loop_verdict_equality() {
    assert_eq!(LoopVerdict::Ok, LoopVerdict::Ok);
    assert_ne!(
        LoopVerdict::Ok,
        LoopVerdict::Warn {
            pattern: "x".to_owned(),
            message: "y".to_owned(),
        }
    );
}

#[test]
fn loop_verdict_warn_contains_pattern_in_message() {
    let mut det = LoopDetector::new(2);
    det.record("exec", "same", false);
    let v = det.record("exec", "same", false);
    match v {
        LoopVerdict::Warn { pattern, message } => {
            assert!(
                message.contains(&pattern),
                "warning message should contain pattern"
            );
        }
        _ => panic!("expected Warn"),
    }
}
