//! Turn-level hook system for behavior correction.
//!
//! Hooks intercept the agent pipeline at three points:
//! - `before_query`: before the model call, can modify system prompt or inject messages
//! - `on_turn_complete`: after the model responds, for audit/logging/metrics
//! - `before_tool`: before tool execution, can approve/deny/modify
//!
//! Hooks run in priority order (lower number = higher priority = runs first).
//! A denied tool call short-circuits: lower-priority hooks do not run.

pub(crate) mod builtins;
pub(crate) mod registry;

#[cfg(test)]
mod tests;

use std::future::Future;
use std::pin::Pin;

use crate::pipeline::{PipelineContext, TurnResult, TurnUsage};

/// Context passed to `before_query` hooks.
///
/// Hooks can modify the system prompt or inject messages before the model call.
#[derive(Debug)]
pub(crate) struct QueryContext<'a> {
    /// Mutable reference to the pipeline context (system prompt, messages, tools).
    pub pipeline: &'a mut PipelineContext,
    /// Agent identifier.
    pub nous_id: &'a str,
    /// The user's message content for this turn.
    pub user_message: &'a str,
}

/// Context passed to `on_turn_complete` hooks.
#[derive(Debug)]
pub(crate) struct TurnContext<'a> {
    /// The completed turn result.
    pub result: &'a TurnResult,
    /// Agent identifier.
    pub nous_id: &'a str,
    /// Cumulative token usage for this session.
    pub session_tokens: u64,
}

/// Context passed to `before_tool` hooks.
#[derive(Debug)]
pub(crate) struct ToolHookContext<'a> {
    /// Agent identifier.
    pub nous_id: &'a str,
    /// Cumulative token usage for this turn so far.
    pub turn_usage: &'a TurnUsage,
    /// The allowed tool list from agent config, if any.
    pub tool_allowlist: Option<&'a [String]>,
}

/// Result from `before_query` and `on_turn_complete` hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HookResult {
    /// Continue processing.
    Continue,
    /// Abort the turn with a reason.
    Abort {
        /// Human-readable reason for the abort.
        reason: String,
    },
}

/// Result from `before_tool` hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolHookResult {
    /// Allow the tool call to proceed.
    Allow,
    /// Deny the tool call with a reason.
    Deny {
        /// Human-readable reason for the denial.
        reason: String,
    },
}

/// Async trait for turn-level behavior hooks.
///
/// Hooks intercept the agent pipeline at three points. Each method has a
/// default no-op implementation so hooks only need to implement the points
/// they care about.
///
/// WHY: Uses `Pin<Box<dyn Future>>` instead of `async fn` for object safety,
/// matching the `ToolExecutor` pattern used throughout the crate.
pub(crate) trait TurnHook: Send + Sync {
    /// Hook name for logging and diagnostics.
    fn name(&self) -> &'static str;

    /// Fires before each model call. Can modify the system prompt or inject messages.
    fn before_query<'a>(
        &'a self,
        _context: &'a mut QueryContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(std::future::ready(HookResult::Continue))
    }

    /// Fires after the model responds. For audit, logging, and metrics.
    fn on_turn_complete<'a>(
        &'a self,
        _context: &'a TurnContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(std::future::ready(HookResult::Continue))
    }

    /// Fires before tool execution. Can approve or deny the tool call.
    fn before_tool<'a>(
        &'a self,
        _tool_name: &'a str,
        _input: &'a serde_json::Value,
        _context: &'a ToolHookContext<'_>,
    ) -> Pin<Box<dyn Future<Output = ToolHookResult> + Send + 'a>> {
        Box::pin(std::future::ready(ToolHookResult::Allow))
    }
}
