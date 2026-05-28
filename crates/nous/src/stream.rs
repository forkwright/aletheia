//! Real-time streaming events for the turn pipeline.

use hermeneus::anthropic::StreamEvent as LlmStreamEvent;

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
        input: serde_json::Value,
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
    },
}
