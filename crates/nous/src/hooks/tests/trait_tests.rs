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
        session_tokens: 0,
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
