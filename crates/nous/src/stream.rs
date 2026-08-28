//! Real-time streaming events for the turn pipeline.

use hermeneus::anthropic::StreamEvent as LlmStreamEvent;
use koina::ulid::Ulid;

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
    pub turn_id: Ulid,
    /// Session that owns the turn.
    // kanon:ignore RUST/primitive-for-domain-id WHY: stream events cross a process boundary into pylon DTOs; both sides carry the session id as a plain string
    pub session_id: String,
    /// Canonical HTTP request ID from the gateway, when one exists (#4853).
    pub request_id: Option<String>,
}

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
