//! Connection state machine and outbound message buffering.

use std::collections::VecDeque;
use std::time::Instant;

use super::client;

/// Exponential backoff delay for reconnection attempts.
///
/// Re-exported from [`crate::connection_utils`].
pub(crate) use crate::connection_utils::reconnect_delay;

/// Connection states for a Signal account.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectionState {
    /// Signal-cli daemon is reachable.
    Connected,
    /// Attempting to reconnect after failure.
    Reconnecting {
        /// Number of reconnection attempts made so far.
        attempt: u32,
    },
    /// Circuit breaker tripped after too many consecutive failures.
    /// Polling has stopped; only periodic health checks run.
    Halted {
        /// Total consecutive failures when the circuit breaker tripped.
        total_failures: u32,
    },
}

/// Outbound message queued during disconnection.
pub(crate) struct BufferedMessage {
    pub params: client::SendParams,
    #[expect(
        dead_code,
        reason = "captured at enqueue time for age-based eviction and metrics"
    )]
    pub enqueued_at: Instant,
}

/// Whether the newly submitted message entered the recovery queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnqueueDisposition {
    /// The new message is queued (an older message may have been evicted).
    Queued,
    /// Buffering is disabled, so the new message was dropped.
    DroppedDisabled,
}

/// Per-account connection state and outbound buffer.
///
/// Tracks connection health and queues outbound messages during
/// disconnection, draining them automatically when the connection restores.
pub(crate) struct AccountState {
    /// Current connection state.
    pub state: ConnectionState,
    /// Messages waiting to be sent when connection is restored.
    buffer: VecDeque<BufferedMessage>,
    /// Maximum buffer size.
    capacity: usize,
    /// Total queued messages dropped due to overflow or a permanent send failure.
    pub dropped_count: u64,
    /// Sends whose response was lost after the daemon may have accepted them.
    pub ambiguous_delivery_count: u64,
    /// Sends that reached only a subset of their intended recipients.
    pub partial_delivery_count: u64,
    /// Destructively received envelopes or parts that could not be forwarded.
    pub receive_loss_count: u64,
}

impl AccountState {
    /// Create a new account state starting as `Connected`.
    #[must_use]
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            state: ConnectionState::Connected,
            buffer: VecDeque::new(),
            capacity,
            dropped_count: 0,
            ambiguous_delivery_count: 0,
            partial_delivery_count: 0,
            receive_loss_count: 0,
        }
    }

    /// Queue an outbound message. Drops the oldest if at capacity.
    #[must_use]
    pub(crate) fn enqueue(&mut self, params: client::SendParams) -> EnqueueDisposition {
        if self.capacity == 0 {
            self.dropped_count = self.dropped_count.saturating_add(1);
            tracing::warn!(
                dropped_count = self.dropped_count,
                "outbound buffer disabled, dropping message"
            );
            return EnqueueDisposition::DroppedDisabled;
        }
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front();
            self.dropped_count = self.dropped_count.saturating_add(1);
            tracing::warn!(
                dropped_count = self.dropped_count,
                "outbound buffer full, dropping oldest message"
            );
        }
        self.buffer.push_back(BufferedMessage {
            params,
            enqueued_at: Instant::now(),
        });
        EnqueueDisposition::Queued
    }

    /// Drain all buffered messages in FIFO order.
    #[cfg(test)]
    pub(crate) fn drain_all(&mut self) -> Vec<client::SendParams> {
        self.buffer.drain(..).map(|bm| bm.params).collect()
    }

    /// Remove and return the oldest queued message before beginning delivery.
    pub(crate) fn take_front(&mut self) -> Option<client::SendParams> {
        self.buffer.pop_front().map(|message| message.params)
    }

    /// Restore a proven-unsent message to the head of the FIFO.
    pub(crate) fn push_front(&mut self, params: client::SendParams) {
        self.buffer.push_front(BufferedMessage {
            params,
            enqueued_at: Instant::now(),
        });
    }

    /// Record a send whose visibility is unknown and therefore cannot be retried.
    pub(crate) fn record_ambiguous_delivery(&mut self) {
        self.ambiguous_delivery_count = self.ambiguous_delivery_count.saturating_add(1);
    }

    /// Record a send that reached only some intended recipients.
    pub(crate) fn record_partial_delivery(&mut self) {
        self.partial_delivery_count = self.partial_delivery_count.saturating_add(1);
    }

    /// Record a queued message that failed permanently and cannot block the FIFO.
    pub(crate) fn record_failed_delivery(&mut self) {
        self.dropped_count = self.dropped_count.saturating_add(1);
    }

    /// Record messages or message parts consumed but not forwarded.
    pub(crate) fn record_receive_loss(&mut self, count: u64) {
        self.receive_loss_count = self.receive_loss_count.saturating_add(count);
    }

    /// Number of messages currently buffered.
    #[must_use]
    pub(crate) fn buffered_count(&self) -> usize {
        self.buffer.len()
    }
}

/// Health report for a Signal account connection.
#[derive(Debug, Clone)]
pub struct ConnectionHealthReport {
    /// Current connection state.
    pub state: ConnectionState,
    /// Messages waiting in the outbound buffer.
    pub buffered_messages: usize,
    /// Total queued messages dropped due to overflow or permanent send failure.
    pub dropped_count: u64,
    /// Total sends with an ambiguous provider-visible outcome.
    pub ambiguous_delivery_count: u64,
    /// Total sends that reached only a subset of intended recipients.
    pub partial_delivery_count: u64,
    /// Total destructively received envelopes or parts that were lost.
    pub receive_loss_count: u64,
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test: JSON key indexing on known-present keys"
)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn account_state_starts_connected() {
        let state = AccountState::new(10);
        assert_eq!(state.state, ConnectionState::Connected);
        assert_eq!(state.buffered_count(), 0);
        assert_eq!(state.dropped_count, 0);
        assert_eq!(state.ambiguous_delivery_count, 0);
        assert_eq!(state.partial_delivery_count, 0);
        assert_eq!(state.receive_loss_count, 0);
    }

    fn test_params(msg: &str) -> client::SendParams {
        client::SendParams {
            message: Some(msg.to_owned()),
            recipient: Some("+1234567890".to_owned()),
            group_id: None,
            account: None,
            attachments: None,
        }
    }

    #[test]
    fn enqueue_and_drain_fifo() {
        let mut state = AccountState::new(10);
        assert_eq!(
            state.enqueue(test_params("first")),
            EnqueueDisposition::Queued
        );
        assert_eq!(
            state.enqueue(test_params("second")),
            EnqueueDisposition::Queued
        );
        assert_eq!(
            state.enqueue(test_params("third")),
            EnqueueDisposition::Queued
        );

        assert_eq!(state.buffered_count(), 3);

        let drained = state.drain_all();
        assert_eq!(drained.len(), 3);
        assert_eq!(drained[0].message.as_deref(), Some("first"));
        assert_eq!(drained[1].message.as_deref(), Some("second"));
        assert_eq!(drained[2].message.as_deref(), Some("third"));
        assert_eq!(state.buffered_count(), 0);
    }

    #[test]
    fn enqueue_drops_oldest_at_capacity() {
        let mut state = AccountState::new(3);
        for message in ["a", "b", "c", "d", "e"] {
            assert_eq!(
                state.enqueue(test_params(message)),
                EnqueueDisposition::Queued
            );
        }

        assert_eq!(state.buffered_count(), 3);
        assert_eq!(state.dropped_count, 2);

        let drained = state.drain_all();
        assert_eq!(drained[0].message.as_deref(), Some("c"));
        assert_eq!(drained[1].message.as_deref(), Some("d"));
        assert_eq!(drained[2].message.as_deref(), Some("e"));
    }

    #[test]
    fn drain_empty_buffer() {
        let mut state = AccountState::new(10);
        let drained = state.drain_all();
        assert!(drained.is_empty());
    }

    #[test]
    fn reconnecting_state_tracks_attempt_count() {
        let mut state = AccountState::new(10);
        assert_eq!(state.state, ConnectionState::Connected);
        state.state = ConnectionState::Reconnecting { attempt: 1 };
        assert_eq!(state.state, ConnectionState::Reconnecting { attempt: 1 });
        state.state = ConnectionState::Reconnecting { attempt: 5 };
        if let ConnectionState::Reconnecting { attempt } = state.state {
            assert_eq!(attempt, 5);
        } else {
            panic!("expected Reconnecting state");
        }
    }

    #[test]
    fn buffer_capacity_one_drops_oldest_on_second_enqueue() {
        let mut state = AccountState::new(1);
        assert_eq!(
            state.enqueue(test_params("first")),
            EnqueueDisposition::Queued
        );
        assert_eq!(state.buffered_count(), 1);
        assert_eq!(state.dropped_count, 0);

        assert_eq!(
            state.enqueue(test_params("second")),
            EnqueueDisposition::Queued
        );
        assert_eq!(state.buffered_count(), 1);
        assert_eq!(state.dropped_count, 1);

        let drained = state.drain_all();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].message.as_deref(), Some("second"));
    }

    #[test]
    fn zero_capacity_drops_instead_of_growing_an_unbounded_queue() {
        let mut state = AccountState::new(0);
        assert_eq!(
            state.enqueue(test_params("never queued")),
            EnqueueDisposition::DroppedDisabled
        );
        assert_eq!(state.buffered_count(), 0);
        assert_eq!(state.dropped_count, 1);
    }

    #[test]
    fn drain_resets_buffer_then_accepts_new_messages() {
        let mut state = AccountState::new(10);
        assert_eq!(state.enqueue(test_params("a")), EnqueueDisposition::Queued);
        assert_eq!(state.enqueue(test_params("b")), EnqueueDisposition::Queued);
        assert_eq!(state.buffered_count(), 2);

        drop(state.drain_all());
        assert_eq!(state.buffered_count(), 0);

        assert_eq!(state.enqueue(test_params("c")), EnqueueDisposition::Queued);
        assert_eq!(state.buffered_count(), 1);
    }

    #[test]
    fn buffering_during_reconnect_then_draining_on_restore() {
        let mut state = AccountState::new(10);
        state.state = ConnectionState::Reconnecting { attempt: 1 };
        assert_eq!(
            state.enqueue(test_params("buffered-during-reconnect")),
            EnqueueDisposition::Queued
        );
        assert_eq!(state.buffered_count(), 1);

        state.state = ConnectionState::Connected;
        let drained = state.drain_all();
        assert_eq!(drained.len(), 1);
        assert_eq!(
            drained[0].message.as_deref(),
            Some("buffered-during-reconnect")
        );
    }

    #[test]
    fn reconnect_delay_u32_max_caps_at_sixty_seconds() {
        assert_eq!(reconnect_delay(u32::MAX), Duration::from_mins(1));
    }
}
