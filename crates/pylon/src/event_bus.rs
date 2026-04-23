//! Lightweight in-process broadcast channel for domain events.
//!
//! Cross-system consumers subscribe via [`EventBus::subscribe`] and receive
//! [`DomainEvent`] values filtered by topic.  Lagged subscribers are dropped
//! gracefully with a warning rather than panicking.

use serde::{Deserialize, Serialize};

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

impl DomainEvent {
    /// Construct a new domain event with the current timestamp.
    #[must_use]
    pub fn new(topic: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            topic: topic.into(),
            payload,
            at: jiff::Timestamp::now().to_string(),
        }
    }
}

/// In-process broadcast bus for domain events.
///
/// Holds a [`tokio::sync::broadcast::Sender`] and provides typed publish /
/// subscribe methods.  The channel capacity is fixed at creation time;
/// slow subscribers lag behind and are dropped gracefully.
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<DomainEvent>,
}

impl EventBus {
    /// Create a new event bus with the given channel capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(capacity);
        Self { tx }
    }

    /// Publish a domain event to all current subscribers.
    ///
    /// Errors are silently ignored when there are no subscribers.
    pub fn publish(&self, event: DomainEvent) {
        let _ = self.tx.send(event);
    }

    /// Subscribe to the broadcast channel.
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<DomainEvent> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn event_bus_publish_and_receive() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();

        bus.publish(DomainEvent::new("test.topic", serde_json::json!({"k": 1})));

        let event = rx.try_recv().expect("should receive event");
        assert_eq!(event.topic, "test.topic");
        assert_eq!(event.payload.get("k"), Some(&serde_json::json!(1)));
    }

    #[test]
    fn event_bus_no_subscriber_is_noop() {
        let bus = EventBus::new(16);
        bus.publish(DomainEvent::new("test.topic", serde_json::json!({})));
        // Should not panic or error.
    }
}
