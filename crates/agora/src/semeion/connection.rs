//! Connection state machine and outbound message buffering.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::client;

/// Connection states for a Signal account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    /// Signal-cli daemon is reachable.
    Connected,
    /// Attempting to reconnect after failure.
    Reconnecting { attempt: u32 },
}

/// Outbound message queued during disconnection.
pub(crate) struct BufferedMessage {
    pub params: client::SendParams,
    #[expect(dead_code, reason = "useful for future metrics/age-based eviction")]
    pub enqueued_at: Instant,
}

/// Per-account connection state and outbound buffer.
///
/// Tracks connection health and queues outbound messages during
/// disconnection, draining them automatically when the connection restores.
pub struct AccountState {
    /// Current connection state.
    pub state: ConnectionState,
    /// Messages waiting to be sent when connection is restored.
    buffer: VecDeque<BufferedMessage>,
    /// Maximum buffer size.
    capacity: usize,
    /// Total messages dropped due to buffer overflow.
    pub dropped_count: u64,
}

impl AccountState {
    /// Create a new account state starting as `Connected`.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            state: ConnectionState::Connected,
            buffer: VecDeque::new(),
            capacity,
            dropped_count: 0,
        }
    }

    /// Queue an outbound message. Drops the oldest if at capacity.
    pub fn enqueue(&mut self, params: client::SendParams) {
        if self.buffer.len() >= self.capacity {
            self.buffer.pop_front();
            self.dropped_count += 1;
            tracing::warn!(
                dropped_count = self.dropped_count,
                "outbound buffer full, dropping oldest message"
            );
        }
        self.buffer.push_back(BufferedMessage {
            params,
            enqueued_at: Instant::now(),
        });
    }

    /// Drain all buffered messages in FIFO order.
    pub fn drain_all(&mut self) -> Vec<client::SendParams> {
        self.buffer.drain(..).map(|bm| bm.params).collect()
    }

    /// Number of messages currently buffered.
    #[must_use]
    pub fn buffered_count(&self) -> usize {
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
    /// Total messages dropped due to overflow.
    pub dropped_count: u64,
}

/// Exponential backoff delay for reconnection attempts.
///
/// 1s, 2s, 4s, 8s, 16s, 32s, 60s (capped).
#[must_use]
pub fn reconnect_delay(attempt: u32) -> Duration {
    let secs = 1u64.checked_shl(attempt.min(6)).unwrap_or(64);
    Duration::from_secs(secs.min(60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_delay_values() {
        assert_eq!(reconnect_delay(0), Duration::from_secs(1));
        assert_eq!(reconnect_delay(1), Duration::from_secs(2));
        assert_eq!(reconnect_delay(2), Duration::from_secs(4));
        assert_eq!(reconnect_delay(3), Duration::from_secs(8));
        assert_eq!(reconnect_delay(4), Duration::from_secs(16));
        assert_eq!(reconnect_delay(5), Duration::from_secs(32));
        assert_eq!(reconnect_delay(6), Duration::from_secs(60));
        assert_eq!(reconnect_delay(7), Duration::from_secs(60));
        assert_eq!(reconnect_delay(100), Duration::from_secs(60));
    }

    #[test]
    fn account_state_starts_connected() {
        let state = AccountState::new(10);
        assert_eq!(state.state, ConnectionState::Connected);
        assert_eq!(state.buffered_count(), 0);
        assert_eq!(state.dropped_count, 0);
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
        state.enqueue(test_params("first"));
        state.enqueue(test_params("second"));
        state.enqueue(test_params("third"));

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
        state.enqueue(test_params("a"));
        state.enqueue(test_params("b"));
        state.enqueue(test_params("c"));
        state.enqueue(test_params("d"));
        state.enqueue(test_params("e"));

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
}
