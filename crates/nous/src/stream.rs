//! Real-time streaming events for the turn pipeline.

use aletheia_hermeneus::anthropic::StreamEvent as LlmStreamEvent;

/// Events emitted during a streaming turn, bridging LLM deltas and tool lifecycle.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "variant fields (tool_id, tool_name, input, result, is_error, duration_ms) are self-documenting by name"
)]
pub enum TurnStreamEvent {
    /// LLM streaming delta forwarded from the provider.
    LlmDelta(LlmStreamEvent),
    /// Tool execution started.
    ToolStart {
        tool_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    /// Tool execution completed.
    ToolResult {
        tool_id: String,
        tool_name: String,
        result: String,
        is_error: bool,
        duration_ms: u64,
    },
}
