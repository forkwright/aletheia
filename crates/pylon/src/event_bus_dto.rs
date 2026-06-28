//! Domain event wire shapes.

use serde::{Deserialize, Serialize};

/// Topic names in the canonical global SSE event contract.
///
/// WHY(#4613): Discovery, subscription defaults, and contract tests share a
/// single source of truth so first-party clients see one stable global event
/// schema across `/api/v1/events` and the compatibility `/events/subscribe`
/// alias.
pub(crate) const DISCOVERABLE_TOPICS: &[&str] = &[
    "background.progress",
    "fact.created",
    "nous.lifecycle",
    "session.archived",
    "session.created",
    "turn.complete",
    "turn.error",
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
