//! Real-time streaming events for the turn pipeline.

use hermeneus::anthropic::StreamEvent as LlmStreamEvent;

/// Authoritative identity of the turn emitting a tool-lifecycle event (#5016).
///
/// WHY: the approval event previously carried the session-local turn *number*
/// in a field named `turn_id`, and the Pylon bridge silently substituted its
/// own stream ULID — the same event had two different identities depending on
/// where it was observed. Every tool-lifecycle event now carries the canonical
/// turn ULID (`SessionState::turn_id`), the owning session id, and the gateway
/// request id when the turn originated from an HTTP request (#4853).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnEventIdentity {
    /// Canonical turn identifier (ULID), stable across actor restarts.
    // kanon:ignore RUST/primitive-for-domain-id WHY: stream-event wire identity; the ULID is minted on SessionState and only stringified here
    pub turn_id: String,
    /// Session that owns the turn.
    // kanon:ignore RUST/primitive-for-domain-id WHY: stream events cross a process boundary into pylon DTOs; both sides carry the session id as a plain string
    pub session_id: String,
    /// Canonical HTTP request ID from the gateway, when one exists (#4853).
    pub request_id: Option<String>,
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
        identity: TurnEventIdentity,
        tool_id: String,
        tool_name: String,
        input: serde_json::Value,
    },
    /// Tool approval is required before execution.
    ToolApprovalRequired {
        identity: TurnEventIdentity,
        tool_id: String,
        tool_name: String,
        input: serde_json::Value,
        risk: String,
        reason: String,
    },
    /// Tool approval was resolved.
    ToolApprovalResolved {
        identity: TurnEventIdentity,
        tool_id: String,
        decision: String,
    },
    /// Tool execution completed.
    ToolResult {
        identity: TurnEventIdentity,
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
