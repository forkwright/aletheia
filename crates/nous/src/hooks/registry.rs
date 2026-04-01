//! Hook registry: stores hooks with priority ordering.

use tracing::{debug, warn};

use super::{HookResult, QueryContext, ToolHookContext, ToolHookResult, TurnContext, TurnHook};

/// Entry in the hook registry: a hook with its priority.
struct HookEntry {
    /// Lower number = higher priority = runs first.
    priority: i32,
    hook: Box<dyn TurnHook>,
}

/// Registry of turn-level hooks, ordered by priority.
///
/// Hooks run in priority order (lower number = higher priority).
/// The expected hook count is small (under 20) and insertion is rare,
/// so a sorted `Vec` is sufficient.
pub(crate) struct HookRegistry {
    hooks: Vec<HookEntry>,
}

impl Default for HookRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl HookRegistry {
    /// Create an empty registry.
    pub(crate) fn new() -> Self {
        Self { hooks: Vec::new() }
    }

    /// Register a hook with the given priority.
    ///
    /// Lower priority numbers run first. Hooks with equal priority
    /// run in insertion order.
    pub(crate) fn register(&mut self, priority: i32, hook: Box<dyn TurnHook>) {
        debug!(hook = hook.name(), priority, "registering turn hook");
        let pos = self
            .hooks
            .partition_point(|entry| entry.priority <= priority);
        self.hooks.insert(pos, HookEntry { priority, hook });
    }

    /// Number of registered hooks.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.hooks.len()
    }

    /// Run all `before_query` hooks in priority order.
    ///
    /// Short-circuits on `HookResult::Abort`.
    pub(crate) async fn run_before_query(&self, context: &mut QueryContext<'_>) -> HookResult {
        for entry in &self.hooks {
            let result = entry.hook.before_query(context).await;
            if let HookResult::Abort { ref reason } = result {
                warn!(
                    hook = entry.hook.name(),
                    priority = entry.priority,
                    reason = reason.as_str(),
                    "before_query hook aborted turn"
                );
                return result;
            }
        }
        HookResult::Continue
    }

    /// Run all `on_turn_complete` hooks in priority order.
    ///
    /// Does not short-circuit: all hooks run even if one returns Abort,
    /// because the turn is already complete and audit hooks should always fire.
    pub(crate) async fn run_on_turn_complete(&self, context: &TurnContext<'_>) {
        for entry in &self.hooks {
            let result = entry.hook.on_turn_complete(context).await;
            if let HookResult::Abort { ref reason } = result {
                // NOTE: log but do not short-circuit — turn is already complete
                debug!(
                    hook = entry.hook.name(),
                    priority = entry.priority,
                    reason = reason.as_str(),
                    "on_turn_complete hook returned abort (ignored, turn already complete)"
                );
            }
        }
    }

    /// Run all `before_tool` hooks in priority order.
    ///
    /// Short-circuits on `ToolHookResult::Deny`: a denied tool call
    /// does not proceed through lower-priority hooks.
    pub(crate) async fn run_before_tool(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        context: &ToolHookContext<'_>,
    ) -> ToolHookResult {
        for entry in &self.hooks {
            let result = entry.hook.before_tool(tool_name, input, context).await;
            if let ToolHookResult::Deny { ref reason } = result {
                warn!(
                    hook = entry.hook.name(),
                    priority = entry.priority,
                    tool_name,
                    reason = reason.as_str(),
                    "before_tool hook denied tool call"
                );
                return result;
            }
        }
        ToolHookResult::Allow
    }
}
