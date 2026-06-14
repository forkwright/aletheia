//! Domain event wire shapes.

use serde::{Deserialize, Serialize};

/// Topic names that pylon currently publishes on its domain event bus.
///
/// WHY: Discovery and subscription tests share a single source of truth so the
/// advertised topic list cannot drift from the topics actually emitted by pylon
/// handlers.
pub const DISCOVERABLE_TOPICS: &[&str] = &["fact.created", "turn.complete", "nous.lifecycle"];

/// A domain event with a stable topic name and JSON payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DomainEvent {
    /// Event topic (e.g. `fact.created`, `turn.complete`).
    pub topic: String,
    /// Structured event payload.
    pub payload: serde_json::Value,
    /// ISO-8601 timestamp of emission.
    pub at: String,
}
