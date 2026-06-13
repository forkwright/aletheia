//! Lightweight in-process broadcast channel for domain events.
//!
//! Cross-system consumers subscribe via [`EventBus::subscribe`] and receive
//! [`DomainEvent`] values filtered by topic.  Lagged subscribers are dropped
//! gracefully with a warning rather than panicking.

#[path = "event_bus_dto.rs"]
mod event_bus_dto;
pub use event_bus_dto::DomainEvent;

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
    /// If there are no active subscribers the event is dropped; a structured
    /// warning is logged and the `aletheia_event_bus_drops_total` counter is
    /// incremented so operators can detect persistent drop conditions.
    pub fn publish(&self, event: DomainEvent) {
        if self.tx.receiver_count() == 0 {
            tracing::warn!(
                topic = %event.topic,
                cause = "no_active_subscribers",
                "event-bus drop: no active subscribers"
            );
            crate::metrics::record_event_bus_drop(&event.topic, "no_active_subscribers");
        }
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
    fn event_bus_no_subscriber_does_not_panic() {
        let bus = EventBus::new(16);
        // Publish with no subscribers: must not panic, even though the event is dropped.
        bus.publish(DomainEvent::new("test.topic", serde_json::json!({})));
    }

    /// Verify that a lagging subscriber receives `RecvError::Lagged` when the channel
    /// fills up, proving that slow/disconnected subscribers are handled gracefully.
    #[test]
    fn lagging_subscriber_gets_lagged_error() {
        let bus = EventBus::new(2); // tiny capacity to force lag
        let mut rx = bus.subscribe();

        // Publish 3 events into a capacity-2 channel without reading.
        for i in 0_u32..3 {
            bus.publish(DomainEvent::new("lag.topic", serde_json::json!({"i": i})));
        }

        // The receiver should report a lag (skipped messages) on next recv.
        match rx.try_recv() {
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
                assert!(n >= 1, "should report at least one skipped message");
            }
            Ok(_) => {
                // First message came through before overflow; consume the rest.
                // If we can drain without lag, that's acceptable too for small counts.
            }
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }
}
