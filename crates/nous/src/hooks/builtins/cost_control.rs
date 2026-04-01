//! Cost control hook: tracks token usage and aborts turns exceeding budget.

use std::future::Future;
use std::pin::Pin;

use tracing::debug;

use crate::hooks::{HookResult, QueryContext, ToolHookContext, ToolHookResult, TurnHook};

/// Hook that enforces per-turn token budgets.
///
/// Checks cumulative session token usage against a configured budget
/// before each query. Aborts the turn if the budget is exceeded.
pub(crate) struct CostControlHook {
    /// Maximum tokens allowed per turn. 0 = unlimited.
    turn_token_budget: u64,
}

impl CostControlHook {
    /// Create a new cost control hook with the given per-turn token budget.
    pub(crate) fn new(turn_token_budget: u64) -> Self {
        Self { turn_token_budget }
    }
}

impl TurnHook for CostControlHook {
    fn name(&self) -> &'static str {
        "cost_control"
    }

    fn before_query<'a>(
        &'a self,
        context: &'a mut QueryContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(async move {
            if self.turn_token_budget == 0 {
                return HookResult::Continue;
            }

            // NOTE: check remaining token budget from the pipeline context
            #[expect(
                clippy::cast_sign_loss,
                clippy::as_conversions,
                reason = "i64→u64: remaining_tokens is clamped to non-negative in pipeline"
            )]
            let remaining = context.pipeline.remaining_tokens.max(0) as u64; // kanon:ignore RUST/as-cast

            if remaining < self.turn_token_budget / 10 {
                debug!(
                    remaining,
                    budget = self.turn_token_budget,
                    "cost control: token budget nearly exhausted"
                );
                return HookResult::Abort {
                    reason: format!(
                        "token budget nearly exhausted: {remaining} tokens remaining \
                         (budget: {})",
                        self.turn_token_budget
                    ),
                };
            }

            HookResult::Continue
        })
    }

    fn before_tool<'a>(
        &'a self,
        tool_name: &'a str,
        _input: &'a serde_json::Value,
        context: &'a ToolHookContext<'_>,
    ) -> Pin<Box<dyn Future<Output = ToolHookResult> + Send + 'a>> {
        Box::pin(async move {
            if self.turn_token_budget == 0 {
                return ToolHookResult::Allow;
            }

            let used = context.turn_usage.total_tokens();
            if used > self.turn_token_budget {
                debug!(
                    used,
                    budget = self.turn_token_budget,
                    tool_name,
                    "cost control: turn token budget exceeded, denying tool call"
                );
                return ToolHookResult::Deny {
                    reason: format!(
                        "turn token budget exceeded: {used} tokens used \
                         (budget: {})",
                        self.turn_token_budget
                    ),
                };
            }

            ToolHookResult::Allow
        })
    }
}
