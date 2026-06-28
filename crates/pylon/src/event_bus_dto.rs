//! Domain event wire shapes.

use serde::{Deserialize, Serialize};

/// Topic names that pylon currently publishes on its domain event bus.
///
/// WHY: Discovery and subscription tests share a single source of truth so the
/// advertised topic list cannot drift from the topics actually emitted by pylon
/// handlers.
pub(crate) const DISCOVERABLE_TOPICS: &[&str] = &[
    "fact.created",
    "turn.complete",
    "nous.lifecycle",
    "tool.approval.required",
    "tool.approval.resolved",
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
