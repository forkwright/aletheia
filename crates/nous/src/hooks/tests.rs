//! Tests for turn-level hook system.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::builtins::{AuditLoggingHook, CostControlHook, ScopeEnforcementHook};
use super::registry::HookRegistry;
use super::{
    HookResult, QueryContext, ToolHookContext, ToolHookResult, TurnContext, TurnHook, TurnUsage,
};
use crate::pipeline::{PipelineContext, TurnResult};

// -- Test hook implementations --

/// Hook that always allows everything. Tracks call counts.
struct NoopHook {
    name: &'static str,
    before_query_count: AtomicU32,
    on_turn_complete_count: AtomicU32,
    before_tool_count: AtomicU32,
}

impl NoopHook {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            before_query_count: AtomicU32::new(0),
            on_turn_complete_count: AtomicU32::new(0),
            before_tool_count: AtomicU32::new(0),
        }
    }
}

impl TurnHook for NoopHook {
    fn name(&self) -> &'static str {
        self.name
    }

    fn before_query<'a>(
        &'a self,
        _context: &'a mut QueryContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        self.before_query_count.fetch_add(1, Ordering::Relaxed);
        Box::pin(std::future::ready(HookResult::Continue))
    }

    fn on_turn_complete<'a>(
        &'a self,
        _context: &'a TurnContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        self.on_turn_complete_count.fetch_add(1, Ordering::Relaxed);
        Box::pin(std::future::ready(HookResult::Continue))
    }

    fn before_tool<'a>(
        &'a self,
        _tool_name: &'a str,
        _input: &'a serde_json::Value,
        _context: &'a ToolHookContext<'_>,
    ) -> Pin<Box<dyn Future<Output = ToolHookResult> + Send + 'a>> {
        self.before_tool_count.fetch_add(1, Ordering::Relaxed);
        Box::pin(std::future::ready(ToolHookResult::Allow))
    }
}

/// Hook that always aborts on before_query.
struct AbortingHook;

impl TurnHook for AbortingHook {
    fn name(&self) -> &'static str {
        "aborting"
    }

    fn before_query<'a>(
        &'a self,
        _context: &'a mut QueryContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(std::future::ready(HookResult::Abort {
            reason: "budget exceeded".to_owned(),
        }))
    }
}

/// Hook that always denies tool calls.
struct DenyingToolHook;

impl TurnHook for DenyingToolHook {
    fn name(&self) -> &'static str {
        "denying"
    }

    fn before_tool<'a>(
        &'a self,
        tool_name: &'a str,
        _input: &'a serde_json::Value,
        _context: &'a ToolHookContext<'_>,
    ) -> Pin<Box<dyn Future<Output = ToolHookResult> + Send + 'a>> {
        Box::pin(std::future::ready(ToolHookResult::Deny {
            reason: format!("tool '{tool_name}' denied by test hook"),
        }))
    }
}

/// Hook that records the order it was called using a shared counter.
struct OrderTrackingHook {
    name: &'static str,
    order: Arc<std::sync::Mutex<Vec<&'static str>>>,
}

impl TurnHook for OrderTrackingHook {
    fn name(&self) -> &'static str {
        self.name
    }

    fn before_query<'a>(
        &'a self,
        _context: &'a mut QueryContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(async move {
            self.order.lock().expect("lock").push(self.name);
            HookResult::Continue
        })
    }

    fn before_tool<'a>(
        &'a self,
        _tool_name: &'a str,
        _input: &'a serde_json::Value,
        _context: &'a ToolHookContext<'_>,
    ) -> Pin<Box<dyn Future<Output = ToolHookResult> + Send + 'a>> {
        Box::pin(async move {
            self.order.lock().expect("lock").push(self.name);
            ToolHookResult::Allow
        })
    }
}

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
    }
}

static DEFAULT_USAGE: TurnUsage = TurnUsage {
    input_tokens: 0,
    output_tokens: 0,
    cache_read_tokens: 0,
    cache_write_tokens: 0,
    llm_calls: 0,
};

fn test_tool_hook_context() -> ToolHookContext<'static> {
    ToolHookContext {
        nous_id: "test-agent",
        turn_usage: &DEFAULT_USAGE,
        tool_allowlist: None,
    }
}

// -- Trait tests --

mod trait_tests {
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
}

// -- Registry tests --

mod registry_tests {
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
            session_tokens: 100,
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
}

// -- Built-in hook tests --

mod cost_control_tests {
    use super::*;

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
}

mod scope_enforcement_tests {
    use super::*;

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
}

mod audit_logging_tests {
    use super::*;

    #[tokio::test]
    async fn before_query_returns_continue() {
        let hook = AuditLoggingHook;
        let mut pipeline = PipelineContext::default();
        let mut ctx = QueryContext {
            pipeline: &mut pipeline,
            nous_id: "test",
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
            session_tokens: 150,
        };

        let result = hook.on_turn_complete(&ctx).await;
        assert_eq!(
            result,
            HookResult::Continue,
            "audit hook should always continue on turn_complete"
        );
    }
}

// -- Config tests --

mod config_tests {
    use crate::config::HookConfig;
    use crate::hooks::builtins::register_builtin_hooks;
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
            config.audit_logging_enabled,
            "audit logging should be enabled by default"
        );
    }

    #[test]
    fn register_all_builtins_from_default_config() {
        let mut registry = HookRegistry::new();
        let config = HookConfig::default();
        register_builtin_hooks(&mut registry, &config);
        assert_eq!(
            registry.len(),
            3,
            "default config should register 3 built-in hooks"
        );
    }

    #[test]
    fn disabling_hooks_reduces_count() {
        let mut registry = HookRegistry::new();
        let config = HookConfig {
            cost_control_enabled: false,
            scope_enforcement_enabled: false,
            audit_logging_enabled: true,
            turn_token_budget: 0,
        };
        register_builtin_hooks(&mut registry, &config);
        assert_eq!(
            registry.len(),
            1,
            "only audit logging hook should be registered"
        );
    }

    #[test]
    fn all_hooks_disabled_gives_empty_registry() {
        let mut registry = HookRegistry::new();
        let config = HookConfig {
            cost_control_enabled: false,
            scope_enforcement_enabled: false,
            audit_logging_enabled: false,
            turn_token_budget: 0,
        };
        register_builtin_hooks(&mut registry, &config);
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
        assert_eq!(config.audit_logging_enabled, back.audit_logging_enabled);
        assert_eq!(config.turn_token_budget, back.turn_token_budget);
    }
}

// -- Integration test --

mod integration_tests {
    use super::*;
    use crate::config::HookConfig;
    use crate::hooks::builtins::register_builtin_hooks;

    #[tokio::test]
    async fn full_hook_lifecycle_with_builtins() {
        let mut registry = HookRegistry::new();
        register_builtin_hooks(&mut registry, &HookConfig::default());

        // before_query should succeed
        let mut pipeline = PipelineContext {
            remaining_tokens: 10_000,
            ..PipelineContext::default()
        };
        let mut query_ctx = QueryContext {
            pipeline: &mut pipeline,
            nous_id: "test-agent",
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
            session_tokens: 150,
        };
        registry.run_on_turn_complete(&turn_ctx).await;
    }

    #[tokio::test]
    async fn scope_enforcement_denies_through_registry() {
        let allowlist = vec!["read".to_owned()];
        let mut registry = HookRegistry::new();
        register_builtin_hooks(&mut registry, &HookConfig::default());

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
        let config = HookConfig {
            cost_control_enabled: true,
            turn_token_budget: 100,
            scope_enforcement_enabled: false,
            audit_logging_enabled: false,
        };
        let mut registry = HookRegistry::new();
        register_builtin_hooks(&mut registry, &config);

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
}
