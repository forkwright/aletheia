//! Tests for [`WorkingCheckpointInjector`].

#![expect(clippy::expect_used, reason = "test assertions")]

use std::sync::Arc;

use organon::types::WorkingCheckpointStore;

use crate::hooks::{HookResult, QueryContext, TurnContext, TurnHook};
use crate::pipeline::{PipelineContext, TurnResult, TurnUsage};
use crate::working_memory::FjallWorkingCheckpointStore;

use super::WorkingCheckpointInjector;

fn test_turn_result() -> TurnResult {
    TurnResult {
        content: "test response".to_owned(),
        tool_calls: Vec::new(),
        usage: TurnUsage {
            input_tokens: 100,
            output_tokens: 50,
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

#[tokio::test]
async fn injector_includes_key_info_and_history() {
    let store = Arc::new(FjallWorkingCheckpointStore::open_in_memory().expect("open store"));
    let hook = WorkingCheckpointInjector::new(Some(store.clone()));

    store
        .write_checkpoint("ses-1", 1, "first checkpoint")
        .expect("write first");
    store
        .write_checkpoint("ses-1", 2, "second checkpoint")
        .expect("write second");
    store
        .write_checkpoint("ses-1", 3, "third checkpoint")
        .expect("write third");

    let mut pipeline = PipelineContext {
        system_prompt: Some("Base prompt.".to_owned()),
        remaining_tokens: 100_000,
        ..PipelineContext::default()
    };
    let mut ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test-agent",
        session_id: "ses-1",
        turn_number: 4,
        user_message: "hello",
    };

    let result = hook.before_query(&mut ctx).await;
    assert_eq!(result, HookResult::Continue, "hook should continue");

    let prompt = ctx.pipeline.system_prompt.as_ref().expect("prompt");
    assert!(
        prompt.contains("<key_info>"),
        "prompt should contain <key_info> block"
    );
    assert!(
        prompt.contains("third checkpoint"),
        "<key_info> should contain the latest checkpoint"
    );
    assert!(
        prompt.contains("<history>"),
        "prompt should contain <history> block when multiple checkpoints exist"
    );
    assert!(
        prompt.contains("Turn 2: second checkpoint"),
        "<history> should contain older checkpoints"
    );
}

#[tokio::test]
async fn injector_skips_when_no_checkpoints() {
    let store = Arc::new(FjallWorkingCheckpointStore::open_in_memory().expect("open store"));
    let hook = WorkingCheckpointInjector::new(Some(store));

    let mut pipeline = PipelineContext {
        system_prompt: Some("Base prompt.".to_owned()),
        remaining_tokens: 100_000,
        ..PipelineContext::default()
    };
    let mut ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test-agent",
        session_id: "ses-empty",
        turn_number: 1,
        user_message: "hello",
    };

    let result = hook.before_query(&mut ctx).await;
    assert_eq!(result, HookResult::Continue, "hook should continue");
    assert_eq!(
        ctx.pipeline.system_prompt.as_ref().expect("prompt"),
        "Base prompt.",
        "prompt should be unchanged when no checkpoints exist"
    );
}

#[tokio::test]
async fn injector_truncates_oversized_content() {
    let store = Arc::new(FjallWorkingCheckpointStore::open_in_memory().expect("open store"));
    let hook = WorkingCheckpointInjector::new(Some(store.clone()));

    let long_content = "x".repeat(3000);
    store
        .write_checkpoint("ses-1", 1, &long_content)
        .expect("write");

    let mut pipeline = PipelineContext {
        system_prompt: Some("Base prompt.".to_owned()),
        remaining_tokens: 100_000,
        ..PipelineContext::default()
    };
    let mut ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test-agent",
        session_id: "ses-1",
        turn_number: 2,
        user_message: "hello",
    };

    let result = hook.before_query(&mut ctx).await;
    assert_eq!(result, HookResult::Continue, "hook should continue");

    let prompt = ctx.pipeline.system_prompt.as_ref().expect("prompt");
    assert!(
        prompt.contains("...[truncated]"),
        "oversized checkpoint should be truncated with marker"
    );
}

#[tokio::test]
async fn injector_skips_when_insufficient_budget() {
    let store = Arc::new(FjallWorkingCheckpointStore::open_in_memory().expect("open store"));
    let hook = WorkingCheckpointInjector::new(Some(store.clone()));

    store
        .write_checkpoint("ses-1", 1, "checkpoint")
        .expect("write");

    let mut pipeline = PipelineContext {
        system_prompt: Some("Base prompt.".to_owned()),
        remaining_tokens: 1,
        ..PipelineContext::default()
    };
    let mut ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test-agent",
        session_id: "ses-1",
        turn_number: 2,
        user_message: "hello",
    };

    let result = hook.before_query(&mut ctx).await;
    assert_eq!(result, HookResult::Continue, "hook should continue");
    assert_eq!(
        ctx.pipeline.system_prompt.as_ref().expect("prompt"),
        "Base prompt.",
        "prompt should be unchanged when token budget is insufficient"
    );
}

#[tokio::test]
async fn injector_is_noop_without_store() {
    let hook = WorkingCheckpointInjector::new(None);

    let mut pipeline = PipelineContext {
        system_prompt: Some("Base prompt.".to_owned()),
        remaining_tokens: 100_000,
        ..PipelineContext::default()
    };
    let mut ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test-agent",
        session_id: "ses-1",
        turn_number: 1,
        user_message: "hello",
    };

    let result = hook.before_query(&mut ctx).await;
    assert_eq!(
        result,
        HookResult::Continue,
        "hook should continue without store"
    );
    assert_eq!(
        ctx.pipeline.system_prompt.as_ref().expect("prompt"),
        "Base prompt.",
        "prompt should be unchanged when store is None"
    );
}

#[tokio::test]
async fn on_turn_complete_verifies_checkpoint() {
    let store = Arc::new(FjallWorkingCheckpointStore::open_in_memory().expect("open store"));
    let hook = WorkingCheckpointInjector::new(Some(store.clone()));

    store
        .write_checkpoint("ses-1", 5, "turn-5-checkpoint")
        .expect("write");

    let turn_result = test_turn_result();
    let ctx = TurnContext {
        result: &turn_result,
        nous_id: "test-agent",
        session_id: "ses-1",
        turn_number: 5,
        session_tokens: 0,
        reinject_identity: false,
    };

    let result = hook.on_turn_complete(&ctx).await;
    assert_eq!(
        result,
        HookResult::Continue,
        "on_turn_complete should return Continue"
    );
}

#[tokio::test]
async fn mod_n_boundary_triggers_identity_reinjection() {
    let store = Arc::new(FjallWorkingCheckpointStore::open_in_memory().expect("open store"));
    let hook = WorkingCheckpointInjector::new(Some(store.clone()));

    store
        .write_checkpoint("ses-1", 1, "checkpoint")
        .expect("write");

    // Turn 9 completes with reinject_identity=true (because (9+1)%10==0).
    let turn_result = test_turn_result();
    let turn_ctx = TurnContext {
        result: &turn_result,
        nous_id: "test-agent",
        session_id: "ses-1",
        turn_number: 9,
        session_tokens: 0,
        reinject_identity: true,
    };
    let result = hook.on_turn_complete(&turn_ctx).await;
    assert_eq!(result, HookResult::Continue);

    // Next turn (10) should see the identity reminder injected.
    let mut pipeline = PipelineContext {
        system_prompt: Some("Base.".to_owned()),
        remaining_tokens: 100_000,
        ..PipelineContext::default()
    };
    let mut query_ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test-agent",
        session_id: "ses-1",
        turn_number: 10,
        user_message: "hello",
    };
    let result = hook.before_query(&mut query_ctx).await;
    assert_eq!(result, HookResult::Continue);

    let prompt = query_ctx.pipeline.system_prompt.as_ref().expect("prompt");
    assert!(
        prompt.contains("<identity_reminder>"),
        "mod-N boundary should inject identity reminder"
    );
}

#[tokio::test]
async fn injector_truncates_multibyte_content_without_panic() {
    // WHY(#4736): truncation must occur on a char boundary. A byte-index
    // truncate at key_info_max_chars would split a 2-byte char and panic.
    let store = Arc::new(FjallWorkingCheckpointStore::open_in_memory().expect("open store"));
    let hook = WorkingCheckpointInjector::new(Some(store.clone()));

    // 2-byte chars guarantee a byte-index cut at 2000 lands mid-character.
    let long_content = "é".repeat(2500);
    store
        .write_checkpoint("ses-mb", 1, &long_content)
        .expect("write");

    let mut pipeline = PipelineContext {
        system_prompt: Some("Base prompt.".to_owned()),
        remaining_tokens: 100_000,
        ..PipelineContext::default()
    };
    let mut ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test-agent",
        session_id: "ses-mb",
        turn_number: 2,
        user_message: "hello",
    };

    let result = hook.before_query(&mut ctx).await;
    assert_eq!(result, HookResult::Continue, "hook should continue");

    let prompt = ctx.pipeline.system_prompt.as_ref().expect("prompt");
    assert!(
        prompt.contains("...[truncated]"),
        "oversized multibyte checkpoint should be truncated with marker"
    );
}
