use super::*;

// -- Cost control tests --

#[tokio::test]
async fn allows_when_budget_is_zero() {
    let hook = CostControlHook::new(0);
    let mut pipeline = PipelineContext {
        remaining_tokens: 100,
        ..PipelineContext::default()
    };
    let mut ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test",
        session_id: "ses-test",
        turn_number: 1,
        user_message: "hello",
    };

    let result = hook.before_query(&mut ctx).await;
    assert_eq!(
        result,
        HookResult::Continue,
        "zero budget should mean unlimited"
    );
}

#[tokio::test]
async fn allows_when_tokens_sufficient() {
    let hook = CostControlHook::new(1000);
    let mut pipeline = PipelineContext {
        remaining_tokens: 500,
        ..PipelineContext::default()
    };
    let mut ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test",
        session_id: "ses-test",
        turn_number: 1,
        user_message: "hello",
    };

    let result = hook.before_query(&mut ctx).await;
    assert_eq!(
        result,
        HookResult::Continue,
        "should allow when remaining > 10% of budget"
    );
}

#[tokio::test]
async fn aborts_when_nearly_exhausted() {
    let hook = CostControlHook::new(1000);
    let mut pipeline = PipelineContext {
        remaining_tokens: 50, // less than 1000/10 = 100
        ..PipelineContext::default()
    };
    let mut ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test",
        session_id: "ses-test",
        turn_number: 1,
        user_message: "hello",
    };

    let result = hook.before_query(&mut ctx).await;
    assert!(
        matches!(result, HookResult::Abort { .. }),
        "should abort when remaining < 10% of budget"
    );
}

#[tokio::test]
async fn denies_tool_when_turn_budget_exceeded() {
    let hook = CostControlHook::new(100);
    let usage = TurnUsage {
        input_tokens: 80,
        output_tokens: 30, // total = 110 > 100
        ..TurnUsage::default()
    };
    let ctx = ToolHookContext {
        nous_id: "test",
        turn_usage: &usage,
        tool_allowlist: None,
    };

    let result = hook
        .before_tool("test_tool", &serde_json::json!({}), &ctx)
        .await;
    assert!(
        matches!(result, ToolHookResult::Deny { .. }),
        "should deny when turn usage exceeds budget"
    );
}

#[tokio::test]
async fn allows_tool_when_within_budget() {
    let hook = CostControlHook::new(200);
    let usage = TurnUsage {
        input_tokens: 80,
        output_tokens: 30, // total = 110 < 200
        ..TurnUsage::default()
    };
    let ctx = ToolHookContext {
        nous_id: "test",
        turn_usage: &usage,
        tool_allowlist: None,
    };

    let result = hook
        .before_tool("test_tool", &serde_json::json!({}), &ctx)
        .await;
    assert_eq!(
        result,
        ToolHookResult::Allow,
        "should allow when within budget"
    );
}

// -- Scope enforcement tests --

#[tokio::test]
async fn allows_when_no_allowlist() {
    let hook = ScopeEnforcementHook;
    let ctx = ToolHookContext {
        nous_id: "test",
        turn_usage: &TurnUsage::default(),
        tool_allowlist: None,
    };

    let result = hook
        .before_tool("any_tool", &serde_json::json!({}), &ctx)
        .await;
    assert_eq!(
        result,
        ToolHookResult::Allow,
        "should allow any tool when no allowlist"
    );
}

#[tokio::test]
async fn allows_tool_in_allowlist() {
    let hook = ScopeEnforcementHook;
    let allowlist = vec!["read".to_owned(), "write".to_owned()];
    let ctx = ToolHookContext {
        nous_id: "test",
        turn_usage: &TurnUsage::default(),
        tool_allowlist: Some(&allowlist),
    };

    let result = hook.before_tool("read", &serde_json::json!({}), &ctx).await;
    assert_eq!(
        result,
        ToolHookResult::Allow,
        "should allow tool in allowlist"
    );
}

#[tokio::test]
async fn denies_tool_not_in_allowlist() {
    let hook = ScopeEnforcementHook;
    let allowlist = vec!["read".to_owned(), "write".to_owned()];
    let ctx = ToolHookContext {
        nous_id: "test",
        turn_usage: &TurnUsage::default(),
        tool_allowlist: Some(&allowlist),
    };

    let result = hook.before_tool("exec", &serde_json::json!({}), &ctx).await;
    assert!(
        matches!(result, ToolHookResult::Deny { reason } if reason.contains("exec")),
        "should deny tool not in allowlist with tool name in reason"
    );
}
