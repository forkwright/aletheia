//! Integration: nous pipeline types assembled from config + session.

#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "integration tests: index-based assertions on known-length slices"
)]

use aletheia_nous::config::{NousConfig, PipelineConfig};
use aletheia_nous::pipeline::{
    GuardResult, InteractionSignal, LoopDetector, LoopVerdict, PipelineContext, PipelineInput,
    PipelineMessage, ToolCall, TurnResult, TurnUsage,
};
use aletheia_nous::session::{SessionManager, SessionState};

#[test]
fn pipeline_input_from_session_and_config() {
    let config = NousConfig::default();
    let state = SessionState::new("ses-1".to_owned(), "main".to_owned(), &config);
    let pipeline_config = PipelineConfig::default();

    let input = PipelineInput {
        content: "hello".to_owned(),
        session: state,
        config: pipeline_config,
    };

    assert_eq!(input.content, "hello");
    assert_eq!(input.session.turn, 0);
}

#[test]
fn pipeline_context_guard_blocks_flow() {
    let mut ctx = PipelineContext::default();
    assert_eq!(ctx.guard_result, GuardResult::Allow);

    ctx.guard_result = GuardResult::Rejected {
        reason: "test block".to_owned(),
    };
    assert_ne!(ctx.guard_result, GuardResult::Allow);
}

#[test]
fn loop_detector_integrates_with_guard() {
    let mut detector = LoopDetector::new(3);

    detector.record("exec", "hash1", false);
    detector.record("exec", "hash1", false);
    let loop_result = detector.record("exec", "hash1", false);

    match loop_result {
        LoopVerdict::Halt { ref pattern, .. } | LoopVerdict::Warn { ref pattern, .. } => {
            assert!(pattern.contains("exec"));
        }
        LoopVerdict::Ok | _ => panic!("expected Halt or Warn after 3 identical calls"),
    }
}

#[test]
fn turn_result_with_tool_calls() {
    let result = TurnResult {
        content: "I'll run that tool.".to_owned(),
        tool_calls: vec![ToolCall {
            id: "tc-1".to_owned(),
            name: "exec".to_owned(),
            input: serde_json::json!({"command": "ls"}),
            result: Some("file1.txt\nfile2.txt".to_owned()),
            is_error: false,
            duration_ms: 42,
        }],
        usage: TurnUsage {
            input_tokens: 200,
            output_tokens: 50,
            cache_read_tokens: 150,
            cache_write_tokens: 50,
            llm_calls: 2,
        },
        signals: vec![InteractionSignal::ToolExecution],
        stop_reason: "end_turn".to_owned(),
    };

    assert_eq!(result.tool_calls.len(), 1);
    assert_eq!(result.usage.total_tokens(), 250);
    assert!(!result.tool_calls[0].is_error);
}

#[test]
fn session_state_flows_through_pipeline() {
    let config = NousConfig {
        id: "syn".to_owned(),
        ..NousConfig::default()
    };
    let mgr = SessionManager::new(config);
    let mut state = mgr.create_session("ses-1", "main");

    assert_eq!(state.turn, 0);
    state.next_turn();
    state.next_turn();
    assert_eq!(state.turn, 2);

    assert!(!SessionManager::is_ephemeral("main"));
    assert!(SessionManager::is_ephemeral("ask:demiurge"));
}

#[test]
fn pipeline_message_serde() {
    let msg = PipelineMessage {
        role: "user".to_owned(),
        content: "hello".to_owned(),
        token_estimate: 5,
    };
    let json = serde_json::to_string(&msg).unwrap();
    let back: PipelineMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(msg.role, back.role);
    assert_eq!(msg.content, back.content);
}
