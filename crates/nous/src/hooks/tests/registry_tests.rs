//! Split from `hooks/tests.rs` — see parent mod.

use super::*;

#[test]
fn empty_registry_has_zero_hooks() {
    let registry = HookRegistry::new();
    assert_eq!(registry.len(), 0, "new registry should have zero hooks");
}

#[test]
fn register_adds_hooks() {
    let mut registry = HookRegistry::new();
    registry.register(10, Box::new(NoopHook::new("first")));
    registry.register(20, Box::new(NoopHook::new("second")));
    assert_eq!(registry.len(), 2, "should have two registered hooks");
}

#[tokio::test]
async fn before_query_runs_all_hooks_in_order() {
    let order = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
    let mut registry = HookRegistry::new();
    registry.register(
        30,
        Box::new(OrderTrackingHook {
            name: "third",
            order: Arc::clone(&order),
        }),
    );
    registry.register(
        10,
        Box::new(OrderTrackingHook {
            name: "first",
            order: Arc::clone(&order),
        }),
    );
    registry.register(
        20,
        Box::new(OrderTrackingHook {
            name: "second",
            order: Arc::clone(&order),
        }),
    );

    let mut pipeline = PipelineContext::default();
    let mut ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test",
        session_id: "ses-test",
        turn_number: 1,
        user_message: "hello",
    };

    let result = registry.run_before_query(&mut ctx).await;
    assert_eq!(result, HookResult::Continue, "all hooks should allow");

    let calls = order.lock().expect("lock");
    assert_eq!(
        *calls,
        vec!["first", "second", "third"],
        "hooks should run in priority order (lower number first)"
    );
}

#[tokio::test]
async fn before_query_short_circuits_on_abort() {
    let order = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
    let mut registry = HookRegistry::new();
    registry.register(
        10,
        Box::new(OrderTrackingHook {
            name: "first",
            order: Arc::clone(&order),
        }),
    );
    registry.register(20, Box::new(AbortingHook));
    registry.register(
        30,
        Box::new(OrderTrackingHook {
            name: "should_not_run",
            order: Arc::clone(&order),
        }),
    );

    let mut pipeline = PipelineContext::default();
    let mut ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test",
        session_id: "ses-test",
        turn_number: 1,
        user_message: "hello",
    };

    let result = registry.run_before_query(&mut ctx).await;
    assert!(
        matches!(result, HookResult::Abort { .. }),
        "should abort from aborting hook"
    );

    let calls = order.lock().expect("lock");
    assert_eq!(
        *calls,
        vec!["first"],
        "only first hook should have run before abort"
    );
}

#[tokio::test]
async fn before_tool_short_circuits_on_deny() {
    let order = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
    let mut registry = HookRegistry::new();
    registry.register(
        10,
        Box::new(OrderTrackingHook {
            name: "first",
            order: Arc::clone(&order),
        }),
    );
    registry.register(20, Box::new(DenyingToolHook));
    registry.register(
        30,
        Box::new(OrderTrackingHook {
            name: "should_not_run",
            order: Arc::clone(&order),
        }),
    );

    let ctx = test_tool_hook_context();
    let result = registry
        .run_before_tool("test_tool", &serde_json::json!({}), &ctx)
        .await;

    assert!(
        matches!(result, ToolHookResult::Deny { .. }),
        "should deny from denying hook"
    );

    let calls = order.lock().expect("lock");
    assert_eq!(
        *calls,
        vec!["first"],
        "only first hook should have run before deny"
    );
}

#[tokio::test]
async fn on_turn_complete_runs_all_hooks_without_short_circuit() {
    let hook1 = Arc::new(NoopHook::new("hook1"));
    let hook2 = Arc::new(NoopHook::new("hook2"));

    let mut registry = HookRegistry::new();
    registry.register(10, Box::new(NoopHook::new("first")));
    registry.register(20, Box::new(NoopHook::new("second")));

    let result = test_turn_result();
    let ctx = TurnContext {
        result: &result,
        nous_id: "test",
        session_id: "ses-test",
        turn_number: 1,
        session_tokens: 100,
        reinject_identity: false,
    };

    registry.run_on_turn_complete(&ctx).await;

    // NOTE: we can't directly check call counts on the registered hooks
    // because they're moved into boxes. The test verifies it runs without panic.
    drop(hook1);
    drop(hook2);
}

#[tokio::test]
async fn equal_priority_hooks_run_in_insertion_order() {
    let order = Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
    let mut registry = HookRegistry::new();
    registry.register(
        10,
        Box::new(OrderTrackingHook {
            name: "a",
            order: Arc::clone(&order),
        }),
    );
    registry.register(
        10,
        Box::new(OrderTrackingHook {
            name: "b",
            order: Arc::clone(&order),
        }),
    );
    registry.register(
        10,
        Box::new(OrderTrackingHook {
            name: "c",
            order: Arc::clone(&order),
        }),
    );

    let mut pipeline = PipelineContext::default();
    let mut ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test",
        session_id: "ses-test",
        turn_number: 1,
        user_message: "hello",
    };

    registry.run_before_query(&mut ctx).await;

    let calls = order.lock().expect("lock");
    assert_eq!(
        *calls,
        vec!["a", "b", "c"],
        "equal-priority hooks should run in insertion order"
    );
}

#[tokio::test]
async fn after_tool_fires_and_does_not_short_circuit() {
    let mut registry = HookRegistry::new();
    registry.register(10, Box::new(NoopHook::new("first")));
    registry.register(20, Box::new(NoopHook::new("second")));

    let ctx = super::super::AfterToolContext {
        nous_id: "test",
        tool_name: "test_tool",
        tool_input: &serde_json::json!({"arg": "value"}),
        tool_result: crate::hooks::ToolResultRecord::Present("tool succeeded"),
        is_error: false,
        turn_usage: &DEFAULT_USAGE,
    };

    registry.run_after_tool(&ctx).await;
    // Test passes if no panic occurs
}

#[tokio::test]
async fn session_start_fires_and_can_short_circuit() {
    let mut registry = HookRegistry::new();
    registry.register(10, Box::new(NoopHook::new("first")));
    registry.register(20, Box::new(AbortingHook));

    let ctx = super::super::SessionStartContext {
        nous_id: "test",
        session_key: "session-123",
        timestamp: "2025-01-01T00:00:00Z",
    };

    let result = registry.run_session_start(&ctx).await;
    assert!(
        matches!(result, HookResult::Abort { .. }),
        "should abort from aborting hook"
    );
}

#[tokio::test]
async fn before_compact_fires_and_can_short_circuit() {
    let mut registry = HookRegistry::new();
    registry.register(10, Box::new(NoopHook::new("first")));
    registry.register(20, Box::new(AbortingHook));

    let ctx = super::super::CompactionContext {
        nous_id: "test",
        messages_distilled: 10,
        tokens_before: 1000,
        tokens_after: 500,
        distillation_number: 1,
    };

    let result = registry.run_before_compact(&ctx).await;
    assert!(
        matches!(result, HookResult::Abort { .. }),
        "should abort from aborting hook"
    );
}

#[tokio::test]
async fn after_compact_fires_and_does_not_short_circuit() {
    let mut registry = HookRegistry::new();
    registry.register(10, Box::new(NoopHook::new("first")));
    registry.register(20, Box::new(NoopHook::new("second")));

    let ctx = super::super::CompactionContext {
        nous_id: "test",
        messages_distilled: 10,
        tokens_before: 1000,
        tokens_after: 500,
        distillation_number: 1,
    };

    registry.run_after_compact(&ctx).await;
    // Test passes if no panic occurs
}
