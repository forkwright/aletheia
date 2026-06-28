//! Domain event wire shapes.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Topic names that pylon currently publishes on its domain event bus.
///
/// WHY: Discovery and subscription tests share a single source of truth so the
/// advertised topic list cannot drift from the topics actually emitted by pylon
/// handlers.
pub(crate) const DISCOVERABLE_TOPICS: &[&str] = &[
    "fact.created",
    "turn.complete",
    "nous.lifecycle",
    "credential.audit",
];

/// Event-discovery response for clients that need deliberate subscriptions.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EventDiscoveryResponse {
    /// Discoverable event topics with payload contracts.
    pub topics: Vec<EventTopicDescriptor>,
}

/// A discoverable event topic and its payload contract.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct EventTopicDescriptor {
    /// Topic name used in the `topics` subscribe query.
    pub name: String,
    /// Human-readable purpose of the topic.
    pub description: String,
    /// Visibility model for this event topic.
    pub visibility: String,
    /// JSON contract for the event payload.
    #[schema(value_type = Object)]
    pub payload_contract: serde_json::Value,
}

/// Build the current event-discovery response.
#[must_use]
pub(crate) fn discoverable_topic_descriptors() -> EventDiscoveryResponse {
    let topics = vec![
        fact_created_descriptor(),
        turn_complete_descriptor(),
        nous_lifecycle_descriptor(),
        credential_audit_descriptor(),
    ];
    debug_assert_eq!(
        topics
            .iter()
            .map(|topic| topic.name.as_str())
            .collect::<Vec<_>>(),
        DISCOVERABLE_TOPICS
    );
    EventDiscoveryResponse { topics }
}

fn fact_created_descriptor() -> EventTopicDescriptor {
    EventTopicDescriptor {
        name: "fact.created".to_owned(),
        description: "Knowledge fact creation events.".to_owned(),
        visibility: "scoped_by_nous_id".to_owned(),
        payload_contract: serde_json::json!({
            "type": "object",
            "required": ["fact_id", "nous_id", "content_preview"],
            "properties": {
                "fact_id": { "type": "string" },
                "nous_id": { "type": "string" },
                "content_preview": { "type": "string" }
            }
        }),
    }
}

fn turn_complete_descriptor() -> EventTopicDescriptor {
    EventTopicDescriptor {
        name: "turn.complete".to_owned(),
        description: "Agent turn completion and usage events.".to_owned(),
        visibility: "scoped_by_nous_id".to_owned(),
        payload_contract: serde_json::json!({
            "type": "object",
            "required": [
                "session_id",
                "nous_id",
                "turn_id",
                "input_tokens",
                "output_tokens",
                "stop_reason",
                "model"
            ],
            "properties": {
                "session_id": { "type": "string" },
                "nous_id": { "type": "string" },
                "turn_id": { "type": "string" },
                "input_tokens": { "type": "integer" },
                "output_tokens": { "type": "integer" },
                "cache_read_tokens": { "type": "integer" },
                "cache_write_tokens": { "type": "integer" },
                "stop_reason": { "type": "string" },
                "model": { "type": "string" }
            }
        }),
    }
}

fn nous_lifecycle_descriptor() -> EventTopicDescriptor {
    EventTopicDescriptor {
        name: "nous.lifecycle".to_owned(),
        description: "Agent lifecycle changes that may require operator action.".to_owned(),
        visibility: "operator".to_owned(),
        payload_contract: serde_json::json!({
            "type": "object",
            "required": ["nous_id", "event", "restart_required"],
            "properties": {
                "nous_id": { "type": "string" },
                "event": { "type": "string" },
                "restart_required": { "type": "boolean" }
            }
        }),
    }
}

fn credential_audit_descriptor() -> EventTopicDescriptor {
    EventTopicDescriptor {
        name: "credential.audit".to_owned(),
        description: "Redacted credential control-plane audit events.".to_owned(),
        visibility: "operator".to_owned(),
        payload_contract: serde_json::json!({
            "type": "object",
            "required": [
                "actor",
                "principal",
                "provider",
                "role",
                "action",
                "result",
                "request_id",
                "runtime_effect"
            ],
            "properties": {
                "actor": {
                    "type": "object",
                    "required": ["principal", "role"],
                    "properties": {
                        "principal": { "type": "string" },
                        "role": { "type": "string" },
                        "nous_id": { "type": ["string", "null"] }
                    }
                },
                "principal": { "type": "string" },
                "provider": { "type": ["string", "null"] },
                "role": { "type": ["string", "null"] },
                "action": { "type": "string", "enum": ["add", "validate", "rotate", "remove"] },
                "result": { "type": "string", "enum": ["success", "failure"] },
                "request_id": { "type": "string" },
                "runtime_effect": { "type": "string" },
                "error_code": { "type": ["string", "null"] }
            },
            "redaction": "Raw credential material is never present in this payload."
        }),
    }
}

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
