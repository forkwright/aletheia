//! Real-time streaming events for the turn pipeline.

use hermeneus::anthropic::StreamEvent as LlmStreamEvent;

/// Policy-minimized tool input carried only to the currently connected
/// approver.
///
/// This wrapper deliberately implements neither serialization nor a
/// value-bearing [`std::fmt::Debug`]. Durable/replay consumers receive the
/// separate `replay_input` value on [`TurnStreamEvent::ToolApprovalRequired`]
/// instead of being able to persist this evidence accidentally.
#[derive(Clone)]
pub struct LiveApprovalEvidence(serde_json::Value);

impl LiveApprovalEvidence {
    /// Seal a live-only approval payload at the dispatch boundary.
    #[must_use]
    pub(crate) fn new(input: serde_json::Value) -> Self {
        Self(input)
    }

    /// Borrow the live-only payload without transferring it to a durable
    /// surface.
    #[must_use]
    pub fn as_value(&self) -> &serde_json::Value {
        &self.0
    }

    /// Consume the wrapper at the live transport boundary.
    #[must_use]
    pub fn into_inner(self) -> serde_json::Value {
        self.0
    }
}

impl std::fmt::Debug for LiveApprovalEvidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LiveApprovalEvidence([REDACTED])")
    }
}

/// Events emitted during a streaming turn, bridging LLM deltas and tool lifecycle.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "variant fields (tool_id, tool_name, input, result, is_error, duration_ms) are self-documenting by name"
)]
// kanon:ignore RUST/non-exhaustive-enum — already #[non_exhaustive]; false positive from attribute ordering
pub enum TurnStreamEvent {
    /// LLM streaming delta forwarded from the provider.
    LlmDelta(LlmStreamEvent),
    /// Tool execution started.
    ToolStart {
        tool_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    /// Tool approval is required before execution.
    ToolApprovalRequired {
        turn_id: String,
        tool_id: String,
        tool_name: String,
        /// Minimum policy-redacted evidence sent only to the currently
        /// connected approver.
        input: LiveApprovalEvidence,
        /// Independently produced payload safe for replay/history buffers.
        /// Consumers must never reconstruct this from `input`.
        replay_input: serde_json::Value,
        risk: String,
        reason: String,
    },
    /// Tool approval was resolved.
    ToolApprovalResolved { tool_id: String, decision: String },
    /// Tool execution completed.
    ToolResult {
        tool_id: String,
        tool_name: String,
        result: String,
        is_error: bool,
        duration_ms: u64,
        /// Stable outcome classification (#4558) — `ToolCall::outcome_label()`
        /// for the same call: `"success"` / `"partial_success"` / `"error"`,
        /// or a denial-class string when the call never ran.
        outcome: String,
    },
}
