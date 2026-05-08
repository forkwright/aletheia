//! Split from `hooks/tests.rs` — see parent mod.

use super::*;

#[test]
fn default_before_query_returns_continue() {
    struct MinimalHook;
    impl TurnHook for MinimalHook {
        fn name(&self) -> &'static str {
            "minimal"
        }
    }
    let hook = MinimalHook;
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let mut pipeline = PipelineContext::default();
    let mut ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test",
        session_id: "ses-test",
        turn_number: 1,
        user_message: "hello",
    };
    let result = rt.block_on(hook.before_query(&mut ctx));
    assert_eq!(
        result,
        HookResult::Continue,
        "default before_query should return Continue"
    );
}

#[test]
fn default_on_turn_complete_returns_continue() {
    struct MinimalHook;
    impl TurnHook for MinimalHook {
        fn name(&self) -> &'static str {
            "minimal"
        }
    }
    let hook = MinimalHook;
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let result = test_turn_result();
    let ctx = TurnContext {
        result: &result,
        nous_id: "test",
        session_id: "ses-test",
        turn_number: 1,
        session_tokens: 0,
        reinject_identity: false,
    };
    let hook_result = rt.block_on(hook.on_turn_complete(&ctx));
    assert_eq!(
        hook_result,
        HookResult::Continue,
        "default on_turn_complete should return Continue"
    );
}

#[test]
fn default_before_tool_returns_allow() {
    struct MinimalHook;
    impl TurnHook for MinimalHook {
        fn name(&self) -> &'static str {
            "minimal"
        }
    }
    let hook = MinimalHook;
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let ctx = test_tool_hook_context();
    let result = rt.block_on(hook.before_tool("test_tool", &serde_json::json!({}), &ctx));
    assert_eq!(
        result,
        ToolHookResult::Allow,
        "default before_tool should return Allow"
    );
}

#[test]
fn default_after_tool_returns_continue() {
    struct MinimalHook;
    impl TurnHook for MinimalHook {
        fn name(&self) -> &'static str {
            "minimal"
        }
    }
    let hook = MinimalHook;
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let ctx = AfterToolContext {
        nous_id: "test",
        tool_name: "test_tool",
        tool_input: &serde_json::json!({}),
        tool_result: "success",
        is_error: false,
        turn_usage: &DEFAULT_USAGE,
    };
    let result = rt.block_on(hook.after_tool(&ctx));
    assert_eq!(
        result,
        HookResult::Continue,
        "default after_tool should return Continue"
    );
}

#[test]
fn default_session_start_returns_continue() {
    struct MinimalHook;
    impl TurnHook for MinimalHook {
        fn name(&self) -> &'static str {
            "minimal"
        }
    }
    let hook = MinimalHook;
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let ctx = super::super::SessionStartContext {
        nous_id: "test",
        session_key: "session-123",
        timestamp: "2025-01-01T00:00:00Z",
    };
    let result = rt.block_on(hook.session_start(&ctx));
    assert_eq!(
        result,
        HookResult::Continue,
        "default session_start should return Continue"
    );
}

#[test]
fn default_before_compact_returns_continue() {
    struct MinimalHook;
    impl TurnHook for MinimalHook {
        fn name(&self) -> &'static str {
            "minimal"
        }
    }
    let hook = MinimalHook;
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let ctx = super::super::CompactionContext {
        nous_id: "test",
        messages_distilled: 10,
        tokens_before: 1000,
        tokens_after: 500,
        distillation_number: 1,
    };
    let result = rt.block_on(hook.before_compact(&ctx));
    assert_eq!(
        result,
        HookResult::Continue,
        "default before_compact should return Continue"
    );
}

#[test]
fn default_after_compact_returns_continue() {
    struct MinimalHook;
    impl TurnHook for MinimalHook {
        fn name(&self) -> &'static str {
            "minimal"
        }
    }
    let hook = MinimalHook;
    let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime");
    let ctx = super::super::CompactionContext {
        nous_id: "test",
        messages_distilled: 10,
        tokens_before: 1000,
        tokens_after: 500,
        distillation_number: 1,
    };
    let result = rt.block_on(hook.after_compact(&ctx));
    assert_eq!(
        result,
        HookResult::Continue,
        "default after_compact should return Continue"
    );
}
