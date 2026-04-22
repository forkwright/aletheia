//! Turn-level and session-level hook system for behavior correction.
//!
//! Hooks intercept the agent pipeline at seven points:
//! - `before_query`: before the model call, can modify system prompt or inject messages
//! - `on_turn_complete`: after the model responds, for audit/logging/metrics
//! - `before_tool`: before tool execution, can approve/deny/modify
//! - `after_tool`: after tool execution, for result post-processing and audit
//! - `session_start`: at the start of a new nous session
//! - `before_compact`: right before context distillation begins
//! - `after_compact`: after distillation completes and summary is committed
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

/// Context passed to `after_tool` hooks.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "context fields are exposed to hooks for optional use"
)]
pub(crate) struct AfterToolContext<'a> {
    /// Agent identifier.
    pub nous_id: &'a str,
    /// The tool name that was executed.
    pub tool_name: &'a str,
    /// The tool input that was sent.
    pub tool_input: &'a serde_json::Value,
    /// The tool result content.
    pub tool_result: &'a str,
    /// Whether the tool execution was an error.
    pub is_error: bool,
    /// Cumulative token usage for this turn.
    pub turn_usage: &'a TurnUsage,
}

/// Context passed to `session_start` hooks.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "context fields are exposed to hooks for optional use"
)]
pub(crate) struct SessionStartContext<'a> {
    /// Agent identifier.
    pub nous_id: &'a str,
    /// Session identifier.
    pub session_key: &'a str,
    /// Session timestamp (ISO 8601 format).
    pub timestamp: &'a str,
}

/// Context passed to `before_compact` and `after_compact` hooks.
#[derive(Debug)]
#[expect(
    dead_code,
    reason = "context fields are exposed to hooks for optional use"
)]
pub(crate) struct CompactionContext<'a> {
    /// Agent identifier.
    pub nous_id: &'a str,
    /// Number of messages that were distilled.
    pub messages_distilled: usize,
    /// Token count before distillation.
    pub tokens_before: u64,
    /// Token count of the generated summary.
    pub tokens_after: u64,
    /// Which distillation pass this is (1-indexed).
    pub distillation_number: u32,
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

/// Enumeration of hook points in the agent pipeline.
///
/// WHY: Provides a named representation of each hook point for reflection,
/// logging, filtering, and configuration. Useful for operators to understand
/// which hooks are available and in what order they fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HookPoint {
    /// Fires before each model call. Can modify system prompt or inject messages.
    BeforeQuery,
    /// Fires after the model responds. For audit, logging, and metrics.
    OnTurnComplete,
    /// Fires before tool execution. Can approve or deny the tool call.
    BeforeTool,
    /// Fires after tool execution completes. For result post-processing and audit.
    AfterTool,
    /// Fires at the start of a new nous session.
    SessionStart,
    /// Fires right before context distillation begins.
    BeforeCompact,
    /// Fires after distillation completes and summary is committed.
    AfterCompact,
}

impl HookPoint {
    /// Human-readable name for this hook point.
    #[must_use]
    #[expect(dead_code, reason = "method is part of the public hook infrastructure")]
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::BeforeQuery => "before_query",
            Self::OnTurnComplete => "on_turn_complete",
            Self::BeforeTool => "before_tool",
            Self::AfterTool => "after_tool",
            Self::SessionStart => "session_start",
            Self::BeforeCompact => "before_compact",
            Self::AfterCompact => "after_compact",
        }
    }

    /// All hook points in order of typical execution.
    #[must_use]
    #[expect(dead_code, reason = "method is part of the public hook infrastructure")]
    pub(crate) fn all() -> &'static [HookPoint] {
        &[
            Self::SessionStart,
            Self::BeforeQuery,
            Self::BeforeTool,
            Self::AfterTool,
            Self::OnTurnComplete,
            Self::BeforeCompact,
            Self::AfterCompact,
        ]
    }
}

/// Async trait for turn-level and session-level behavior hooks.
///
/// Hooks intercept the agent pipeline at seven points. Each method has a
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

    /// Fires after tool execution completes. For result post-processing and audit.
    fn after_tool<'a>(
        &'a self,
        _context: &'a AfterToolContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(std::future::ready(HookResult::Continue))
    }

    /// Fires at the start of a new nous session.
    fn session_start<'a>(
        &'a self,
        _context: &'a SessionStartContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(std::future::ready(HookResult::Continue))
    }

    /// Fires right before context distillation begins.
    fn before_compact<'a>(
        &'a self,
        _context: &'a CompactionContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(std::future::ready(HookResult::Continue))
    }

    /// Fires after distillation completes and summary is committed.
    fn after_compact<'a>(
        &'a self,
        _context: &'a CompactionContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(std::future::ready(HookResult::Continue))
    }
}
