//! End-to-end tests for working checkpoint hook integration.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::sync::Arc;

use organon::types::WorkingCheckpointStore;

use crate::hooks::builtins::WorkingCheckpointInjector;
use crate::hooks::{CompactionContext, HookResult, QueryContext, TurnContext, TurnHook};
use crate::pipeline::{PipelineContext, TurnResult, TurnUsage};
use crate::working_memory::FjallWorkingCheckpointStore;

fn test_turn_result() -> TurnResult {
    TurnResult {
        content: "assistant response".to_owned(),
        tool_calls: Vec::new(),
        usage: TurnUsage {
            input_tokens: 10,
            output_tokens: 10,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            llm_calls: 1,
        },
        signals: Vec::new(),
        stop_reason: "end_turn".to_owned(),
        degraded: None,
        reasoning: String::new(),
        model_used: "test-model".to_owned(),
        provider_used: None,
        tool_surface_hashes: Vec::new(),
    }
}

/// Simulate a compact event by calling the before/compact hooks.
fn compact_hooks(
    hook: &WorkingCheckpointInjector,
    _turn: u64,
) -> impl std::future::Future<Output = ()> + use<'_> {
    let compact_ctx = CompactionContext {
        nous_id: "test-agent",
        messages_distilled: 10,
        tokens_before: 5000,
        tokens_after: 1000,
        distillation_number: 1,
    };
    async move {
        let _ = hook.before_compact(&compact_ctx).await;
        let _ = hook.after_compact(&compact_ctx).await;
    }
}

#[tokio::test]
async fn checkpoint_survives_across_turns_and_compaction() {
    let store = Arc::new(FjallWorkingCheckpointStore::open_in_memory().expect("open store"));
    let hook = WorkingCheckpointInjector::new(Some(store.clone()));

    let session_id = "e2e-session";

    for turn in 1..=25 {
        // Rebuild pipeline context each turn (mirrors real pipeline behaviour).
        let mut pipeline = PipelineContext {
            system_prompt: Some("Base system prompt.".to_owned()),
            remaining_tokens: 100_000,
            ..PipelineContext::default()
        };

        let user_message = format!("user turn {turn}");
        let mut query_ctx = QueryContext {
            pipeline: &mut pipeline,
            nous_id: "test-agent",
            session_id,
            turn_number: turn,
            user_message: &user_message,
        };

        let result = hook.before_query(&mut query_ctx).await;
        assert_eq!(
            result,
            HookResult::Continue,
            "before_query should continue at turn {turn}"
        );

        // At turn 5 the agent writes a working checkpoint.
        if turn == 5 {
            store
                .write_checkpoint(session_id, turn, "turn-5-key-info")
                .expect("write checkpoint at turn 5");
        }

        // Synthetic compact event between turns 12 and 13.
        if turn == 12 {
            compact_hooks(&hook, turn).await;
        }

        let turn_result = test_turn_result();
        let turn_ctx = TurnContext {
            result: &turn_result,
            nous_id: "test-agent",
            session_id,
            turn_number: turn,
            session_tokens: turn * 20,
            reinject_identity: (turn + 1) % 10 == 0,
        };
        let result = hook.on_turn_complete(&turn_ctx).await;
        assert_eq!(
            result,
            HookResult::Continue,
            "on_turn_complete should continue at turn {turn}"
        );

        // At turn 23, verify the turn-5 checkpoint is still injected.
        if turn == 23 {
            let prompt = query_ctx
                .pipeline
                .system_prompt
                .as_ref()
                .expect("system prompt should exist");
            assert!(
                prompt.contains("<key_info>"),
                "turn 23 prompt should contain <key_info> block"
            );
            assert!(
                prompt.contains("turn-5-key-info"),
                "turn 23 prompt should contain the turn-5 checkpoint content"
            );
        }
    }
}
