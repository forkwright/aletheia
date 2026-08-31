//! Domain event wire shapes.

use serde::{Deserialize, Serialize};

/// Topic names that pylon currently publishes on its domain event bus.
///
/// WHY: Discovery and subscription tests share a single source of truth so the
/// advertised topic list cannot drift from the topics actually emitted by pylon
/// handlers.
///
/// WHY(#4557): `turn.start`/`turn.complete`/`turn.failed`/`turn.cancelled` are
/// the complete turn lifecycle — every turn publishes exactly `turn.start`
/// followed by exactly one of the other three, never zero and never more than
/// one terminal event (enforced by `TurnBufferHandle`'s terminal-state CAS,
/// the same guard that already serializes the per-turn SSE buffer's
/// Completed/Failed/Aborted transition). `turn.cancelled` covers every
/// non-error terminal abort — client disconnect, server shutdown, and
/// pipeline/ask timeout — mirroring `TurnState::Aborted`'s existing grouping
/// (WHY(#4794) at `turn_buffer.rs`: timeouts are terminal aborts, not generic
/// turn failures).
pub(crate) const DISCOVERABLE_TOPICS: &[&str] = &[
    "fact.created",
    "turn.start",
    "turn.complete",
    "turn.failed",
    "turn.cancelled",
    "nous.lifecycle",
    // WHY(#4878): credential add/rotate/remove (state-changing) and validate
    // (read-only provider probe) are kept as separate topics so a security
    // dashboard can subscribe to mutations without also receiving routine
    // validation-click traffic, or vice versa. Payload contract documented
    // on `crate::handlers::credentials::CredentialAuditEvent`.
    "credential.mutation",
    "credential.validation",
    // WHY(#6813): the per-turn `tool_approval_required`/`tool_approval_resolved`
    // stream events reach only the one client holding that turn's SSE stream
    // open; a tool call blocked on approval in any other agent/session was
    // invisible on the always-open domain-bus connection. These topics mirror
    // that per-turn pair one-to-one — published from the same bridge that
    // translates the nous stream events (the single point every gated
    // approval flows through) — carrying routing identity only, never tool
    // input. Payload builders: `tool_approval_required_event_payload` /
    // `tool_approval_resolved_event_payload` in
    // `crate::handlers::sessions::streaming`.
    "tool.approval_required",
    "tool.approval_resolved",
];

/// A domain event with a stable topic name, monotonic id, JSON payload, and
/// emission timestamp.
///
/// WHY(#4910): The `id` field is a durable sequence number within a pylon
/// process. It enables `Last-Event-ID` reconnect replay and lets clients detect
/// unrecoverable gaps when the requested id has fallen out of the in-memory
/// journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DomainEvent {
    /// Monotonic durable event id (1-based).
    pub id: u64,
    /// Event topic (e.g. `fact.created`, `turn.complete`).
    pub topic: String,
    /// Structured event payload.
    pub payload: serde_json::Value,
    /// ISO-8601 timestamp of emission.
    pub at: String,
}
