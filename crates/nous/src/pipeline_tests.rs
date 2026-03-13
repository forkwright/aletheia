#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions")]
use super::*;

// --- Loop Detector ---

#[test]
fn loop_detector_no_loop() {
    let mut det = LoopDetector::new(3);
    assert!(det.record("exec", "hash1").is_none());
    assert!(det.record("read", "hash2").is_none());
    assert!(det.record("exec", "hash3").is_none());
}

#[test]
fn loop_detector_detects_repeat() {
    let mut det = LoopDetector::new(3);
    assert!(det.record("exec", "same").is_none());
    assert!(det.record("exec", "same").is_none());
    let result = det.record("exec", "same");
    assert!(result.is_some());
    assert_eq!(result.unwrap(), "exec:same");
}

#[test]
fn loop_detector_different_inputs_ok() {
    let mut det = LoopDetector::new(3);
    assert!(det.record("exec", "hash1").is_none());
    assert!(det.record("exec", "hash2").is_none());
    assert!(det.record("exec", "hash3").is_none());
    // Different hashes, no loop
}

#[test]
fn loop_detector_reset() {
    let mut det = LoopDetector::new(3);
    det.record("exec", "same");
    det.record("exec", "same");
    det.reset();
    assert_eq!(det.call_count(), 0);
    assert!(det.record("exec", "same").is_none()); // Reset cleared history
}

#[test]
fn loop_detector_threshold_4() {
    let mut det = LoopDetector::new(4);
    assert!(det.record("exec", "same").is_none());
    assert!(det.record("exec", "same").is_none());
    assert!(det.record("exec", "same").is_none());
    // Not yet — threshold is 4
    let result = det.record("exec", "same");
    assert!(result.is_some());
}

// --- Guard Result ---

#[test]
fn guard_result_equality() {
    assert_eq!(GuardResult::Allow, GuardResult::Allow);
    assert_ne!(
        GuardResult::Allow,
        GuardResult::Rejected {
            reason: "test".to_owned()
        }
    );
}

// --- Turn Usage ---

#[test]
fn turn_usage_total() {
    let usage = TurnUsage {
        input_tokens: 1000,
        output_tokens: 500,
        cache_read_tokens: 800,
        cache_write_tokens: 200,
        llm_calls: 3,
    };
    assert_eq!(usage.total_tokens(), 1500);
}

// --- Interaction Signal ---

#[test]
fn interaction_signal_serde() {
    let signal = InteractionSignal::CodeGeneration;
    let json = serde_json::to_string(&signal).unwrap();
    assert_eq!(json, "\"code_generation\"");
    let back: InteractionSignal = serde_json::from_str(&json).unwrap();
    assert_eq!(back, signal);
}

// --- Pipeline Context ---

#[test]
fn pipeline_context_default() {
    let ctx = PipelineContext::default();
    assert!(ctx.system_prompt.is_none());
    assert!(ctx.messages.is_empty());
    assert!(!ctx.needs_distillation);
    assert_eq!(ctx.guard_result, GuardResult::Allow);
}

// --- Guard Result variants ---

#[test]
fn guard_result_rate_limited() {
    let g = GuardResult::RateLimited {
        retry_after_ms: 5000,
    };
    assert_ne!(g, GuardResult::Allow);
    match g {
        GuardResult::RateLimited { retry_after_ms } => assert_eq!(retry_after_ms, 5000),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn guard_result_loop_detected() {
    let g = GuardResult::LoopDetected {
        pattern: "exec:abc".to_owned(),
    };
    match g {
        GuardResult::LoopDetected { pattern } => assert_eq!(pattern, "exec:abc"),
        _ => panic!("wrong variant"),
    }
}

#[test]
fn guard_result_rejected() {
    let g = GuardResult::Rejected {
        reason: "unsafe content".to_owned(),
    };
    match g {
        GuardResult::Rejected { reason } => assert!(reason.contains("unsafe")),
        _ => panic!("wrong variant"),
    }
}

// --- Loop Detector edge cases ---

#[test]
fn loop_detector_threshold_1() {
    let mut det = LoopDetector::new(1);
    let result = det.record("exec", "hash");
    assert!(result.is_some());
}

#[test]
fn loop_detector_call_count_tracks() {
    let mut det = LoopDetector::new(10);
    det.record("a", "1");
    det.record("b", "2");
    det.record("c", "3");
    assert_eq!(det.call_count(), 3);
}

#[test]
fn loop_detector_many_unique_then_repeat() {
    let mut det = LoopDetector::new(3);
    for i in 0..20 {
        det.record("tool", &format!("hash{i}"));
    }
    assert!(det.record("exec", "same").is_none());
    assert!(det.record("exec", "same").is_none());
    assert!(det.record("exec", "same").is_some());
}

// --- Interaction Signal ---

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
        let json = serde_json::to_string(&signal).unwrap();
        let back: InteractionSignal = serde_json::from_str(&json).unwrap();
        assert_eq!(signal, back);
    }
}

// --- Turn Usage ---

#[test]
fn turn_usage_default_is_zero() {
    let usage = TurnUsage::default();
    assert_eq!(usage.total_tokens(), 0);
    assert_eq!(usage.llm_calls, 0);
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
    let json = serde_json::to_string(&usage).unwrap();
    let back: TurnUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(usage.total_tokens(), back.total_tokens());
}

// --- Context assembly ---

#[tokio::test]
async fn assemble_context_populates_pipeline() {
    use crate::config::{NousConfig, PipelineConfig};
    use aletheia_taxis::oikos::Oikos;
    use std::fs;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("nous/test-agent")).unwrap();
    fs::create_dir_all(root.join("shared")).unwrap();
    fs::create_dir_all(root.join("theke")).unwrap();
    fs::write(root.join("nous/test-agent/SOUL.md"), "I am a test agent.").unwrap();
    fs::write(root.join("theke/USER.md"), "Test user.").unwrap();

    let oikos = Oikos::from_root(root);
    let nous_config = NousConfig {
        id: "test-agent".to_owned(),
        ..NousConfig::default()
    };
    let pipeline_config = PipelineConfig::default();
    let mut ctx = PipelineContext::default();

    assemble_context(&oikos, &nous_config, &pipeline_config, &mut ctx)
        .await
        .unwrap();

    assert!(ctx.system_prompt.is_some());
    let prompt = ctx.system_prompt.unwrap();
    assert!(prompt.contains("I am a test agent."));
    assert!(prompt.contains("Test user."));
    assert!(ctx.remaining_tokens > 0);
}

// --- run_pipeline ---

#[tokio::test]
#[expect(
    clippy::too_many_lines,
    reason = "integration test with full pipeline setup"
)]
async fn run_pipeline_simple() {
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, RwLock};

    use aletheia_hermeneus::provider::{LlmProvider, ProviderRegistry};
    use aletheia_hermeneus::types::{
        CompletionRequest, CompletionResponse, ContentBlock, StopReason, Usage,
    };
    use aletheia_koina::id::{NousId, SessionId};
    use aletheia_organon::registry::ToolRegistry;
    use aletheia_organon::types::ToolContext;
    use tempfile::TempDir;

    struct MockProvider {
        response: Mutex<CompletionResponse>,
    }
    impl LlmProvider for MockProvider {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = aletheia_hermeneus::error::Result<CompletionResponse>,
                    > + Send
                    + 'a,
            >,
        > {
            Box::pin(async {
                #[expect(
                    clippy::expect_used,
                    reason = "test mock: poisoned lock means a test bug"
                )]
                Ok(self.response.lock().expect("lock poisoned").clone())
            })
        }
        fn supported_models(&self) -> &[&str] {
            &["test-model"]
        }
        #[expect(
            clippy::unnecessary_literal_bound,
            reason = "trait requires &str return"
        )]
        fn name(&self) -> &str {
            "mock"
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    let dir = TempDir::new().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("nous/test-agent")).unwrap();
    fs::create_dir_all(root.join("shared")).unwrap();
    fs::create_dir_all(root.join("theke")).unwrap();
    fs::write(root.join("nous/test-agent/SOUL.md"), "I am a test agent.").unwrap();

    let oikos = Oikos::from_root(root);
    let nous_config = NousConfig {
        id: "test-agent".to_owned(),
        model: "test-model".to_owned(),
        ..NousConfig::default()
    };
    let pipeline_config = PipelineConfig::default();

    let mut providers = ProviderRegistry::new();
    providers.register(Box::new(MockProvider {
        response: Mutex::new(CompletionResponse {
            id: "resp-1".to_owned(),
            model: "test-model".to_owned(),
            stop_reason: StopReason::EndTurn,
            content: vec![ContentBlock::Text {
                text: "Hello from pipeline!".to_owned(),
                citations: None,
            }],
            usage: Usage {
                input_tokens: 100,
                output_tokens: 50,
                ..Usage::default()
            },
        }),
    }));

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
    )
    .await
    .expect("pipeline should succeed");

    assert_eq!(result.content, "Hello from pipeline!");
    assert!(result.tool_calls.is_empty());
    assert_eq!(result.usage.llm_calls, 1);
    assert_eq!(result.stop_reason, "end_turn");
}

// --- Loop Detector window cap ---

#[test]
fn loop_detector_window_cap_evicts_old_calls() {
    let mut det = LoopDetector::new(100); // high threshold so no loop triggers
    for i in 0..25 {
        det.record("tool", &format!("hash{i}"));
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
    det.record("exec", "same");
    det.record("exec", "same");
    det.record("exec", "same");
    assert_eq!(det.pattern_count(), 3);
}

#[test]
fn loop_detector_pattern_count_zero_on_empty() {
    let det = LoopDetector::new(3);
    assert_eq!(det.pattern_count(), 0);
}

#[test]
fn loop_detector_pattern_count_resets_on_different() {
    let mut det = LoopDetector::new(100);
    det.record("exec", "hash1");
    det.record("exec", "hash1");
    det.record("read", "hash2");
    assert_eq!(det.pattern_count(), 1, "different call breaks the streak");
}

#[test]
fn loop_detector_window_still_detects_loops() {
    let mut det = LoopDetector::new(3);
    // Fill window with unique calls
    for i in 0..18 {
        det.record("tool", &format!("hash{i}"));
    }
    // Now trigger a loop within the window
    assert!(det.record("exec", "same").is_none());
    assert!(det.record("exec", "same").is_none());
    assert!(
        det.record("exec", "same").is_some(),
        "should detect loop even after window eviction"
    );
}

// --- Additional edge cases ---

#[test]
fn loop_detector_interleaved_not_detected() {
    let mut det = LoopDetector::new(3);
    // a, b, a, b, a, b — interleaved, not consecutive
    for _ in 0..3 {
        assert!(det.record("exec", "a").is_none());
        assert!(det.record("exec", "b").is_none());
    }
}

#[test]
fn loop_detector_reset_clears_pattern_count() {
    let mut det = LoopDetector::new(100);
    det.record("exec", "same");
    det.record("exec", "same");
    assert_eq!(det.pattern_count(), 2);
    det.reset();
    assert_eq!(det.pattern_count(), 0);
    assert_eq!(det.call_count(), 0);
}

#[test]
fn pipeline_message_serde_roundtrip() {
    let msg = PipelineMessage {
        role: "user".to_owned(),
        content: "Hello world".to_owned(),
        token_estimate: 3,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: PipelineMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg.role, back.role);
    assert_eq!(msg.content, back.content);
    assert_eq!(msg.token_estimate, back.token_estimate);
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
    let json = serde_json::to_string(&tc).unwrap();
    let back: ToolCall = serde_json::from_str(&json).unwrap();
    assert_eq!(tc.id, back.id);
    assert_eq!(tc.name, back.name);
    assert_eq!(tc.duration_ms, back.duration_ms);
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
    assert!(tc.is_error);
    assert!(tc.result.is_none());
}

#[test]
fn check_guard_always_allows() {
    let config = crate::config::NousConfig::default();
    let session = crate::session::SessionState::new("s-1".to_owned(), "main".to_owned(), &config);
    assert_eq!(check_guard(&session, &config), GuardResult::Allow);
}

#[tokio::test]
async fn assemble_context_missing_soul_returns_error() {
    use aletheia_taxis::oikos::Oikos;
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("nous/test-agent")).unwrap();
    std::fs::create_dir_all(root.join("shared")).unwrap();
    std::fs::create_dir_all(root.join("theke")).unwrap();
    // No SOUL.md anywhere

    let oikos = Oikos::from_root(root);
    let config = crate::config::NousConfig {
        id: "test-agent".to_owned(),
        ..crate::config::NousConfig::default()
    };
    let pipeline_config = crate::config::PipelineConfig::default();
    let mut ctx = PipelineContext::default();

    let err = assemble_context(&oikos, &config, &pipeline_config, &mut ctx).await;
    assert!(err.is_err());
    let msg = err.unwrap_err().to_string();
    assert!(msg.contains("SOUL.md"), "got: {msg}");
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
    assert_eq!(usage.total_tokens(), 150);
}
