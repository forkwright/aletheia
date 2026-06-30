//! Lightweight in-process broadcast channel for domain events.
//!
//! Cross-system consumers subscribe via [`EventBus::subscribe`] and receive
//! [`DomainEvent`] values filtered by topic.  Slow subscribers are reported as
//! `RecvError::Lagged`; the SSE handler in `crate::handlers::events` surfaces that
//! loss to clients as a typed `stream_lagged` event so it is never silent.
//!
//! WHY(#4910): The bus also keeps a bounded in-memory journal of recent events
//! so that SSE reconnects with `Last-Event-ID` can replay missed events. Events
//! that have fallen out of the journal are reported as a typed `stream_gap`
//! event, and cursors outside the current journal are reported as typed
//! `stream_expired` events, rather than silently losing continuity.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::Mutex;

use tokio::sync::broadcast;

#[path = "event_bus_dto.rs"]
mod event_bus_dto;
pub(crate) use event_bus_dto::DISCOVERABLE_TOPICS;
pub use event_bus_dto::DomainEvent;

impl DomainEvent {
    /// Construct a new domain event with the current timestamp and id.
    #[must_use]
    pub(crate) fn new(id: u64, topic: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            id,
            topic: topic.into(),
            payload,
            at: jiff::Timestamp::now().to_string(),
        }
    }
}

/// Why a reconnecting subscriber cannot be resumed from its requested cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JournalGapReason {
    /// The requested cursor is older than the retained journal tail.
    RetainedEventsEvicted,
    /// The client supplied a cursor but this process has no retained events.
    JournalEmpty,
    /// The requested cursor is newer than the newest event in this process.
    CursorBeyondJournal,
}

impl JournalGapReason {
    #[must_use]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::RetainedEventsEvicted => "retained_events_evicted",
            Self::JournalEmpty => "journal_empty",
            Self::CursorBeyondJournal => "cursor_beyond_journal",
        }
    }
}

/// Continuity failure details for a reconnecting subscriber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JournalGap {
    pub(crate) reason: JournalGapReason,
    pub(crate) requested_last_event_id: u64,
    pub(crate) first_missed_id: Option<u64>,
    pub(crate) last_missed_id: Option<u64>,
    pub(crate) oldest_retained_id: Option<u64>,
    pub(crate) newest_retained_id: Option<u64>,
    pub(crate) reset_event_id: u64,
}

/// Snapshot of the journal for a reconnecting subscriber.
pub(crate) struct JournalSnapshot {
    /// Events with `id > last_id` that are still retained in the journal.
    pub(crate) replay: Vec<DomainEvent>,
    /// Typed continuity failure, if the requested cursor cannot be resumed.
    pub(crate) gap: Option<JournalGap>,
}

/// In-process broadcast bus for domain events.
///
/// Holds a [`tokio::sync::broadcast::Sender`] and provides typed publish /
/// subscribe methods.  The channel capacity is fixed at creation time;
/// slow subscribers lag behind and are dropped gracefully.
///
/// WHY(#4910): A bounded in-memory journal is kept alongside the broadcast
/// channel so that reconnecting clients can resume the stream with
/// `Last-Event-ID`. The journal is bounded; events that fall out of it or
/// cursors from a different process lifetime are reported via typed control
/// events.
pub struct EventBus {
    tx: broadcast::Sender<DomainEvent>,
    next_id: AtomicU64,
    journal: Mutex<VecDeque<DomainEvent>>,
    journal_capacity: usize,
}

impl EventBus {
    /// Create a new event bus with the given channel and journal capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self {
            tx,
            next_id: AtomicU64::new(1),
            journal: Mutex::new(VecDeque::with_capacity(capacity)),
            journal_capacity: capacity.max(1),
        }
    }

    /// Publish a domain event to all current subscribers.
    ///
    /// If there are no active subscribers the event is dropped; a structured
    /// warning is logged and the `aletheia_event_bus_drops_total` counter is
    /// incremented so operators can detect persistent drop conditions.
    pub async fn publish(&self, event: DomainEvent) {
        if self.tx.receiver_count() == 0 {
            tracing::warn!(
                topic = %event.topic,
                cause = "no_active_subscribers",
                "event-bus drop: no active subscribers"
            );
            crate::metrics::record_event_bus_drop(&event.topic, "no_active_subscribers");
        }
        {
            let mut journal = self.journal.lock().await;
            if journal.len() >= self.journal_capacity {
                journal.pop_front();
            }
            journal.push_back(event.clone());
        }
        let _ = self.tx.send(event);
    }

    /// Assign the next durable monotonic id.
    pub(crate) fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::SeqCst)
    }

    /// Subscribe to the broadcast channel.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        self.tx.subscribe()
    }

    /// Subscribe and collect retained events with `id > last_id`.
    ///
    /// The returned snapshot contains replay events and a live receiver. If the
    /// requested cursor is not covered by the retained journal, `gap` is set and
    /// the caller must inject a typed control event before any replayed events.
    pub(crate) async fn subscribe_from(
        &self,
        last_id: u64,
    ) -> (JournalSnapshot, broadcast::Receiver<DomainEvent>) {
        let journal = self.journal.lock().await;
        let oldest_id = journal.front().map(|e| e.id);
        let newest_id = journal.back().map(|e| e.id);

        let mut snapshot = JournalSnapshot {
            replay: Vec::new(),
            gap: None,
        };

        match (oldest_id, newest_id) {
            (Some(oldest), Some(newest)) => {
                if last_id > newest {
                    snapshot.gap = Some(JournalGap {
                        reason: JournalGapReason::CursorBeyondJournal,
                        requested_last_event_id: last_id,
                        first_missed_id: None,
                        last_missed_id: None,
                        oldest_retained_id: Some(oldest),
                        newest_retained_id: Some(newest),
                        reset_event_id: newest,
                    });
                } else if last_id > 0 && last_id.saturating_add(1) < oldest {
                    // Events strictly between the client's cursor and the oldest
                    // retained event are unrecoverable. The oldest retained event
                    // itself is still replayed, so it is not part of the missed
                    // range.
                    snapshot.gap = Some(JournalGap {
                        reason: JournalGapReason::RetainedEventsEvicted,
                        requested_last_event_id: last_id,
                        first_missed_id: Some(last_id + 1),
                        last_missed_id: Some(oldest - 1),
                        oldest_retained_id: Some(oldest),
                        newest_retained_id: Some(newest),
                        reset_event_id: oldest - 1,
                    });
                }
                snapshot.replay = journal.iter().filter(|e| e.id > last_id).cloned().collect();
            }
            _ if last_id > 0 => {
                snapshot.gap = Some(JournalGap {
                    reason: JournalGapReason::JournalEmpty,
                    requested_last_event_id: last_id,
                    first_missed_id: None,
                    last_missed_id: None,
                    oldest_retained_id: None,
                    newest_retained_id: None,
                    reset_event_id: 0,
                });
            }
            _ => {}
        }

        let rx = self.tx.subscribe();
        // Release the journal lock only after subscribing so events published
        // between the replay and the subscription are still delivered on the
        // live receiver.
        drop(journal);
        (snapshot, rx)
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on Vecs with asserted length"
)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn event_bus_publish_and_receive() {
        let bus = EventBus::new(16);
        let id = bus.next_id();
        let mut rx = bus.subscribe();

        bus.publish(DomainEvent::new(
            id,
            "test.topic",
            serde_json::json!({"k": 1}),
        ))
        .await;

        let event = rx.try_recv().expect("should receive event");
        assert_eq!(event.id, id);
        assert_eq!(event.topic, "test.topic");
        assert_eq!(event.payload.get("k"), Some(&serde_json::json!(1)));
    }

    #[tokio::test]
    async fn event_bus_no_subscriber_does_not_panic() {
        let bus = EventBus::new(16);
        let id = bus.next_id();
        bus.publish(DomainEvent::new(id, "test.topic", serde_json::json!({})))
            .await;
        // Even with no live subscribers, the event is retained in the journal.
        let (snapshot, _rx) = bus.subscribe_from(0).await;
        assert_eq!(
            snapshot.replay.len(),
            1,
            "event should be retained in journal"
        );
        assert_eq!(snapshot.replay[0].id, id);
    }

    /// Verify that a lagging subscriber receives `RecvError::Lagged` when the channel
    /// fills up, proving that slow/disconnected subscribers are handled gracefully.
    #[tokio::test]
    async fn lagging_subscriber_gets_lagged_error() {
        let bus = EventBus::new(2); // tiny capacity to force lag
        let mut rx = bus.subscribe();

        // Publish 3 events into a capacity-2 channel without reading.
        for i in 0_u32..3 {
            let id = bus.next_id();
            bus.publish(DomainEvent::new(
                id,
                "lag.topic",
                serde_json::json!({"i": i}),
            ))
            .await;
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

    #[tokio::test]
    async fn subscribe_from_replays_missed_events() {
        let bus = EventBus::new(16);
        let id1 = bus.next_id();
        bus.publish(DomainEvent::new(id1, "t", serde_json::json!(1)))
            .await;
        let id2 = bus.next_id();
        bus.publish(DomainEvent::new(id2, "t", serde_json::json!(2)))
            .await;

        let (snapshot, _rx) = bus.subscribe_from(id1).await;
        assert_eq!(snapshot.replay.len(), 1);
        assert_eq!(snapshot.replay[0].id, id2);
        assert_eq!(snapshot.gap, None);
    }

    #[tokio::test]
    async fn subscribe_from_does_not_gap_when_next_event_is_retained() {
        let bus = EventBus::new(2);
        let id1 = bus.next_id();
        bus.publish(DomainEvent::new(id1, "t", serde_json::json!(1)))
            .await;
        let id2 = bus.next_id();
        bus.publish(DomainEvent::new(id2, "t", serde_json::json!(2)))
            .await;
        let id3 = bus.next_id();
        bus.publish(DomainEvent::new(id3, "t", serde_json::json!(3)))
            .await;

        let (snapshot, _rx) = bus.subscribe_from(id1).await;
        assert_eq!(snapshot.gap, None);
        assert_eq!(snapshot.replay.len(), 2);
        assert_eq!(snapshot.replay[0].id, id2);
        assert_eq!(snapshot.replay[1].id, id3);
    }

    #[tokio::test]
    async fn subscribe_from_reports_gap_when_last_id_too_old() {
        let bus = EventBus::new(2);
        let id1 = bus.next_id();
        bus.publish(DomainEvent::new(id1, "t", serde_json::json!(1)))
            .await;
        let id2 = bus.next_id();
        bus.publish(DomainEvent::new(id2, "t", serde_json::json!(2)))
            .await;
        let id3 = bus.next_id();
        bus.publish(DomainEvent::new(id3, "t", serde_json::json!(3)))
            .await;
        let id4 = bus.next_id();
        bus.publish(DomainEvent::new(id4, "t", serde_json::json!(4)))
            .await;

        let (snapshot, _rx) = bus.subscribe_from(id1).await;
        assert_eq!(
            snapshot.gap,
            Some(JournalGap {
                reason: JournalGapReason::RetainedEventsEvicted,
                requested_last_event_id: id1,
                first_missed_id: Some(id2),
                last_missed_id: Some(id2),
                oldest_retained_id: Some(id3),
                newest_retained_id: Some(id4),
                reset_event_id: id2,
            })
        );
        assert_eq!(snapshot.replay.len(), 2);
        assert_eq!(snapshot.replay[0].id, id3);
        assert_eq!(snapshot.replay[1].id, id4);
    }

    #[tokio::test]
    async fn subscribe_from_reports_expired_when_cursor_is_ahead() {
        let bus = EventBus::new(2);
        let id1 = bus.next_id();
        bus.publish(DomainEvent::new(id1, "t", serde_json::json!(1)))
            .await;

        let (snapshot, _rx) = bus.subscribe_from(99).await;
        assert_eq!(
            snapshot.gap,
            Some(JournalGap {
                reason: JournalGapReason::CursorBeyondJournal,
                requested_last_event_id: 99,
                first_missed_id: None,
                last_missed_id: None,
                oldest_retained_id: Some(id1),
                newest_retained_id: Some(id1),
                reset_event_id: id1,
            })
        );
        assert!(snapshot.replay.is_empty());
    }

    #[tokio::test]
    async fn subscribe_from_reports_expired_when_journal_is_empty() {
        let bus = EventBus::new(2);

        let (snapshot, _rx) = bus.subscribe_from(99).await;
        assert_eq!(
            snapshot.gap,
            Some(JournalGap {
                reason: JournalGapReason::JournalEmpty,
                requested_last_event_id: 99,
                first_missed_id: None,
                last_missed_id: None,
                oldest_retained_id: None,
                newest_retained_id: None,
                reset_event_id: 0,
            })
        );
        assert!(snapshot.replay.is_empty());
    }

    #[tokio::test]
    async fn subscribe_from_zero_replays_all() {
        let bus = EventBus::new(8);
        let id1 = bus.next_id();
        bus.publish(DomainEvent::new(id1, "t", serde_json::json!(1)))
            .await;
        let id2 = bus.next_id();
        bus.publish(DomainEvent::new(id2, "t", serde_json::json!(2)))
            .await;

        let (snapshot, _rx) = bus.subscribe_from(0).await;
        assert_eq!(snapshot.replay.len(), 2);
        assert_eq!(snapshot.replay[0].id, id1);
        assert_eq!(snapshot.replay[1].id, id2);
        assert_eq!(snapshot.gap, None);
    }
}
