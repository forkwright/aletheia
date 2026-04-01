//! Scope enforcement hook: validates tool calls against allowed-tool lists.

use std::future::Future;
use std::pin::Pin;

use tracing::debug;

use crate::hooks::{ToolHookContext, ToolHookResult, TurnHook};

/// Hook that enforces tool allowlists from agent configuration.
///
/// When the agent has a `tool_allowlist` configured, this hook denies
/// any tool call not in the list. When no allowlist is set, all tools
/// are allowed.
pub(crate) struct ScopeEnforcementHook;

impl TurnHook for ScopeEnforcementHook {
    fn name(&self) -> &'static str {
        "scope_enforcement"
    }

    fn before_tool<'a>(
        &'a self,
        tool_name: &'a str,
        _input: &'a serde_json::Value,
        context: &'a ToolHookContext<'_>,
    ) -> Pin<Box<dyn Future<Output = ToolHookResult> + Send + 'a>> {
        Box::pin(async move {
            let Some(allowlist) = context.tool_allowlist else {
                return ToolHookResult::Allow;
            };

            if allowlist.iter().any(|allowed| allowed == tool_name) {
                return ToolHookResult::Allow;
            }

            debug!(
                tool_name,
                nous_id = context.nous_id,
                "scope enforcement: tool not in allowlist"
            );

            ToolHookResult::Deny {
                reason: format!(
                    "tool '{tool_name}' is not in the allowed tool list for this agent"
                ),
            }
        })
    }
}
