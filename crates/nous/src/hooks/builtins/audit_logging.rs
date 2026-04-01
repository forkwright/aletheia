//! Audit logging hook: records turn events for observability.

use std::future::Future;
use std::pin::Pin;

use tracing::{debug, info};

use crate::hooks::{HookResult, QueryContext, TurnContext, TurnHook};

/// Hook that records turn events using existing nous metrics and tracing.
///
/// Fires on `before_query` (logs turn start) and `on_turn_complete`
/// (logs turn summary with token usage and tool call count).
pub(crate) struct AuditLoggingHook;

impl TurnHook for AuditLoggingHook {
    fn name(&self) -> &'static str {
        "audit_logging"
    }

    fn before_query<'a>(
        &'a self,
        context: &'a mut QueryContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(async move {
            debug!(
                nous_id = context.nous_id,
                message_len = context.user_message.len(),
                has_system_prompt = context.pipeline.system_prompt.is_some(),
                tool_count = context.pipeline.tools.len(),
                "audit: turn starting"
            );
            HookResult::Continue
        })
    }

    fn on_turn_complete<'a>(
        &'a self,
        context: &'a TurnContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + Send + 'a>> {
        Box::pin(async move {
            info!(
                nous_id = context.nous_id,
                input_tokens = context.result.usage.input_tokens,
                output_tokens = context.result.usage.output_tokens,
                tool_calls = context.result.tool_calls.len(),
                session_tokens = context.session_tokens,
                stop_reason = context.result.stop_reason.as_str(),
                "audit: turn completed"
            );

            // WHY: record turn metrics through the existing metrics system
            crate::metrics::record_turn(context.nous_id);

            HookResult::Continue
        })
    }
}
