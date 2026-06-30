//! Tests for turn-level hook system.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::builtins::{AuditLoggingHook, CostControlHook, ScopeEnforcementHook};
use super::registry::HookRegistry;
use super::{
    AfterToolContext, CompactionContext, HookResult, QueryContext, SessionStartContext,
    ToolHookContext, ToolHookResult, TurnContext, TurnHook, TurnUsage,
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

/// Hook that always aborts on any hook point.
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

    fn session_start<'a>(
        &'a self,
        _context: &'a SessionStartContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(std::future::ready(HookResult::Abort {
            reason: "session start failed".to_owned(),
        }))
    }

    fn before_compact<'a>(
        &'a self,
        _context: &'a CompactionContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(std::future::ready(HookResult::Abort {
            reason: "compaction aborted".to_owned(),
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
        reasoning: String::new(),
        model_used: "test-model".to_owned(),
        provider_used: None,
        tool_surface_hashes: Vec::new(),
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

fn test_after_tool_context(tool_input: &serde_json::Value) -> AfterToolContext<'_> {
    AfterToolContext {
        nous_id: "test-agent",
        tool_name: "test_tool",
        tool_input,
        tool_result: crate::hooks::ToolResultRecord::Present("test result"),
        is_error: false,
        turn_usage: &DEFAULT_USAGE,
    }
}

fn test_session_start_context() -> SessionStartContext<'static> {
    SessionStartContext {
        nous_id: "test-agent",
        session_key: "test-session",
        timestamp: "2024-01-01T00:00:00Z",
    }
}

fn test_compaction_context() -> CompactionContext<'static> {
    CompactionContext {
        nous_id: "test-agent",
        messages_distilled: 10,
        tokens_before: 5000,
        tokens_after: 1000,
        distillation_number: 1,
    }
}

// -- Trait tests --

mod audit_config_integration;
mod cost_and_scope;
mod new_hooks_tests;
mod registry_tests;
mod trait_tests;
mod working_checkpoint;
