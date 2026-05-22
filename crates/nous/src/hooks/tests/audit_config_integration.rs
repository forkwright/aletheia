//! Split from `hooks/tests.rs` — see parent mod.

use super::*;
use crate::config::HookConfig;

// -- Audit logging tests --

#[tokio::test]
async fn before_query_returns_continue() {
    let hook = AuditLoggingHook;
    let mut pipeline = PipelineContext::default();
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
        "audit hook should always continue on before_query"
    );
}

#[tokio::test]
async fn on_turn_complete_returns_continue() {
    let hook = AuditLoggingHook;
    let turn_result = test_turn_result();
    let ctx = TurnContext {
        result: &turn_result,
        nous_id: "test",
        session_id: "ses-test",
        turn_number: 1,
        session_tokens: 150,
        reinject_identity: false,
    };

    let result = hook.on_turn_complete(&ctx).await;
    assert_eq!(
        result,
        HookResult::Continue,
        "audit hook should always continue on turn_complete"
    );
}

// -- Config tests --

use crate::hooks::registry::HookRegistry;

#[test]
fn default_config_enables_all_hooks() {
    let config = HookConfig::default();
    assert!(
        config.cost_control_enabled,
        "cost control should be enabled by default"
    );
    assert!(
        config.scope_enforcement_enabled,
        "scope enforcement should be enabled by default"
    );
    assert!(
        config.correction_hooks_enabled,
        "correction hooks should be enabled by default"
    );
    assert!(
        config.audit_logging_enabled,
        "audit logging should be enabled by default"
    );
}

#[test]
fn register_all_builtins_from_default_config() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let mut registry = HookRegistry::new();
    let config = HookConfig::default();
    register_builtin_hooks(&mut registry, &config, dir.path(), None);
    assert_eq!(
        registry.len(),
        6,
        "default config should register 6 built-in hooks (cost, scope, correction_injector, correction_detector, working_checkpoint, audit)"
    );
}

#[test]
fn disabling_hooks_reduces_count() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let mut registry = HookRegistry::new();
    let config = HookConfig {
        cost_control_enabled: false,
        scope_enforcement_enabled: false,
        correction_hooks_enabled: false,
        audit_logging_enabled: true,
        self_audit_enabled: true,
        working_checkpoint_enabled: false,
        turn_token_budget: 0,
    };
    register_builtin_hooks(&mut registry, &config, dir.path(), None);
    assert_eq!(
        registry.len(),
        1,
        "only audit logging hook should be registered"
    );
}

#[test]
fn all_hooks_disabled_gives_empty_registry() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let mut registry = HookRegistry::new();
    let config = HookConfig {
        cost_control_enabled: false,
        scope_enforcement_enabled: false,
        correction_hooks_enabled: false,
        audit_logging_enabled: false,
        self_audit_enabled: false,
        working_checkpoint_enabled: false,
        turn_token_budget: 0,
    };
    register_builtin_hooks(&mut registry, &config, dir.path(), None);
    assert_eq!(
        registry.len(),
        0,
        "disabling all hooks should give empty registry"
    );
}

#[test]
fn hook_config_serde_roundtrip() {
    let config = HookConfig::default();
    let json = serde_json::to_string(&config).expect("serialize HookConfig");
    let back: HookConfig = serde_json::from_str(&json).expect("deserialize HookConfig");
    assert_eq!(config.cost_control_enabled, back.cost_control_enabled);
    assert_eq!(
        config.scope_enforcement_enabled,
        back.scope_enforcement_enabled
    );
    assert_eq!(
        config.correction_hooks_enabled,
        back.correction_hooks_enabled
    );
    assert_eq!(config.audit_logging_enabled, back.audit_logging_enabled);
    assert_eq!(config.turn_token_budget, back.turn_token_budget);
}

// -- Integration tests --

use crate::hooks::builtins::register_builtin_hooks;

#[tokio::test]
async fn full_hook_lifecycle_with_builtins() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let mut registry = HookRegistry::new();
    register_builtin_hooks(&mut registry, &HookConfig::default(), dir.path(), None);

    // before_query should succeed
    let mut pipeline = PipelineContext {
        remaining_tokens: 100_000,
        system_prompt: Some("Base prompt.".to_owned()),
        ..PipelineContext::default()
    };
    let mut query_ctx = QueryContext {
        pipeline: &mut pipeline,
        nous_id: "test-agent",
        session_id: "ses-test",
        turn_number: 1,
        user_message: "hello",
    };
    let result = registry.run_before_query(&mut query_ctx).await;
    assert_eq!(
        result,
        HookResult::Continue,
        "built-in hooks should allow normal query"
    );

    // before_tool with no allowlist should succeed
    let usage = TurnUsage::default();
    let tool_ctx = ToolHookContext {
        nous_id: "test-agent",
        turn_usage: &usage,
        tool_allowlist: None,
    };
    let tool_result = registry
        .run_before_tool("read", &serde_json::json!({}), &tool_ctx)
        .await;
    assert_eq!(
        tool_result,
        ToolHookResult::Allow,
        "built-in hooks should allow tool with no allowlist"
    );

    // on_turn_complete should run without error
    let turn_result = test_turn_result();
    let turn_ctx = TurnContext {
        result: &turn_result,
        nous_id: "test-agent",
        session_id: "ses-test",
        turn_number: 1,
        session_tokens: 150,
        reinject_identity: false,
    };
    registry.run_on_turn_complete(&turn_ctx).await;
}

#[tokio::test]
async fn scope_enforcement_denies_through_registry() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let allowlist = vec!["read".to_owned()];
    let mut registry = HookRegistry::new();
    register_builtin_hooks(&mut registry, &HookConfig::default(), dir.path(), None);

    let usage = TurnUsage::default();
    let tool_ctx = ToolHookContext {
        nous_id: "test-agent",
        turn_usage: &usage,
        tool_allowlist: Some(&allowlist),
    };

    let result = registry
        .run_before_tool("write", &serde_json::json!({}), &tool_ctx)
        .await;
    assert!(
        matches!(result, ToolHookResult::Deny { .. }),
        "scope enforcement should deny 'write' when only 'read' is allowed"
    );
}

#[tokio::test]
async fn cost_control_denies_through_registry() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let config = HookConfig {
        cost_control_enabled: true,
        turn_token_budget: 100,
        scope_enforcement_enabled: false,
        correction_hooks_enabled: false,
        audit_logging_enabled: false,
        self_audit_enabled: false,
        working_checkpoint_enabled: false,
    };
    let mut registry = HookRegistry::new();
    register_builtin_hooks(&mut registry, &config, dir.path(), None);

    let usage = TurnUsage {
        input_tokens: 80,
        output_tokens: 30, // total = 110 > 100
        ..TurnUsage::default()
    };
    let tool_ctx = ToolHookContext {
        nous_id: "test-agent",
        turn_usage: &usage,
        tool_allowlist: None,
    };

    let result = registry
        .run_before_tool("test_tool", &serde_json::json!({}), &tool_ctx)
        .await;
    assert!(
        matches!(result, ToolHookResult::Deny { .. }),
        "cost control should deny tool when turn budget exceeded"
    );
}
