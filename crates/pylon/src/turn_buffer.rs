//! In-memory turn event buffer for SSE client recovery.
//!
//! Each active turn gets a [`TurnBuffer`] that records every SSE event with a
//! monotonic sequence ID. When a client reconnects with `Last-Event-ID: N`,
//! the server replays events from `N+1` onward.
//!
//! Buffers are held in a [`TurnBufferRegistry`] keyed by `(session_id, turn_id)`.
//! Completed turns are retained for a configurable TTL (default 5 minutes) so
//! that late reconnects can still recover. A background reaper task prunes
//! expired entries.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{Mutex, Notify};
use tracing::{Instrument as _, debug, warn};

/// Default time-to-live for completed turn buffers before they are reaped.
const DEFAULT_COMPLETED_TTL: Duration = Duration::from_mins(5);

/// Maximum events retained per turn buffer to bound memory usage.
const MAX_EVENTS_PER_TURN: usize = 10_000;

/// A single buffered SSE event with its sequence ID.
#[derive(Debug, Clone)]
pub(crate) struct BufferedEvent {
    /// Monotonic sequence number (1-based).
    pub seq: u64,
    /// SSE event type name (e.g. `text_delta`, `message_complete`).
    pub event_type: String,
    /// Serialized JSON payload.
    pub data: String,
}

/// Turn completion state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TurnState {
    /// Turn is still executing; new events may arrive.
    Running,
    /// Turn completed successfully.
    Completed,
    /// Turn failed with an error.
    Failed,
}

/// Per-turn event buffer.
///
/// Internally synchronized via `Mutex` so multiple producers (the streaming
/// task) and consumers (reconnecting clients) can access it concurrently.
pub(crate) struct TurnBuffer {
    events: Vec<BufferedEvent>,
    next_seq: u64,
    state: TurnState,
    /// When the turn completed or failed (for TTL-based reaping).
    finished_at: Option<Instant>,
    /// Notified whenever a new event is appended. Reconnecting clients
    /// wait on this to receive live events after replaying the backlog.
    notify: Arc<Notify>,
}

impl TurnBuffer {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            next_seq: 1,
            state: TurnState::Running,
            finished_at: None,
            notify: Arc::new(Notify::new()),
        }
    }

    /// Append an event and return its assigned sequence number.
    fn push(&mut self, event_type: String, data: String) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;

        if self.events.len() < MAX_EVENTS_PER_TURN {
            self.events.push(BufferedEvent {
                seq,
                event_type,
                data,
            });
        } else if self.events.len() == MAX_EVENTS_PER_TURN {
            warn!(
                seq,
                "turn buffer reached max capacity ({MAX_EVENTS_PER_TURN}), dropping new events"
            );
        }

        self.notify.notify_waiters();
        seq
    }

    /// Mark the turn as completed or failed.
    fn finish(&mut self, state: TurnState) {
        self.state = state;
        self.finished_at = Some(Instant::now());
        self.notify.notify_waiters();
    }

    /// Get all events with sequence > `after_seq`.
    fn events_after(&self, after_seq: u64) -> Vec<BufferedEvent> {
        self.events
            .iter()
            .filter(|e| e.seq > after_seq)
            .cloned()
            .collect()
    }
}

/// Key for looking up a turn buffer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TurnKey {
    session_id: String,
    turn_id: String,
}

/// Registry of active and recently-completed turn buffers.
///
/// Held in `SessionsState` as `Arc<TurnBufferRegistry>`.
pub struct TurnBufferRegistry {
    buffers: Mutex<HashMap<TurnKey, Arc<Mutex<TurnBuffer>>>>,
    completed_ttl: Duration,
}

impl Default for TurnBufferRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TurnBufferRegistry {
    /// Create a new registry with the default completed-turn TTL.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffers: Mutex::new(HashMap::new()),
            completed_ttl: DEFAULT_COMPLETED_TTL,
        }
    }

    /// Create or retrieve the buffer for a turn. Returns a handle that the
    /// streaming handler uses to record events.
    pub(crate) async fn get_or_create(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Arc<Mutex<TurnBuffer>> {
        let key = TurnKey {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
        };
        let mut map = self.buffers.lock().await;
        Arc::clone(
            map.entry(key)
                .or_insert_with(|| Arc::new(Mutex::new(TurnBuffer::new()))),
        )
    }

    /// Look up an existing buffer (for reconnection). Returns `None` if
    /// the turn was never registered or has already been reaped.
    pub(crate) async fn get(
        &self,
        session_id: &str,
        turn_id: &str,
    ) -> Option<Arc<Mutex<TurnBuffer>>> {
        let key = TurnKey {
            session_id: session_id.to_owned(),
            turn_id: turn_id.to_owned(),
        };
        let map = self.buffers.lock().await;
        map.get(&key).cloned()
    }

    /// Remove expired completed/failed turn buffers.
    pub async fn reap_expired(&self) {
        let mut map = self.buffers.lock().await;
        let before = map.len();
        let ttl = self.completed_ttl;

        let mut to_remove = Vec::new();
        for (key, buffer) in map.iter() {
            let buf = buffer.lock().await;
            if let Some(finished_at) = buf.finished_at
                && finished_at.elapsed() > ttl
            {
                to_remove.push(key.clone());
            }
        }

        for key in &to_remove {
            map.remove(key);
        }

        let removed = to_remove.len();
        if removed > 0 {
            debug!(
                removed,
                remaining = before - removed,
                "reaped expired turn buffers"
            );
        }
    }
}

/// Handle for a streaming handler to record events into a turn buffer.
///
/// Wraps `Arc<Mutex<TurnBuffer>>` with convenience methods.
#[derive(Clone)]
pub(crate) struct TurnBufferHandle {
    inner: Arc<Mutex<TurnBuffer>>,
}

impl TurnBufferHandle {
    /// Create a handle from a buffer reference.
    pub fn new(buffer: Arc<Mutex<TurnBuffer>>) -> Self {
        Self { inner: buffer }
    }

    /// Record an SSE event. Returns the assigned sequence number.
    pub async fn record(&self, event_type: &str, data: &str) -> u64 {
        let mut buf = self.inner.lock().await;
        buf.push(event_type.to_owned(), data.to_owned())
    }

    /// Mark the turn as completed.
    pub async fn mark_completed(&self) {
        let mut buf = self.inner.lock().await;
        buf.finish(TurnState::Completed);
    }

    /// Mark the turn as failed.
    pub async fn mark_failed(&self) {
        let mut buf = self.inner.lock().await;
        buf.finish(TurnState::Failed);
    }

    /// Get the notify handle for live-streaming to reconnecting clients.
    pub async fn notify_handle(&self) -> Arc<Notify> {
        let buf = self.inner.lock().await;
        Arc::clone(&buf.notify)
    }

    /// Get events after a given sequence number, plus the current turn state.
    pub async fn events_after(&self, after_seq: u64) -> (Vec<BufferedEvent>, TurnState) {
        let buf = self.inner.lock().await;
        (buf.events_after(after_seq), buf.state.clone())
    }
}

/// Spawn a background task that periodically reaps expired turn buffers.
///
/// Runs every 60 seconds until the shutdown token is cancelled.
pub(crate) fn spawn_reaper(
    registry: Arc<TurnBufferRegistry>,
    shutdown: tokio_util::sync::CancellationToken,
) {
    tokio::spawn(
        async move {
            let mut interval = tokio::time::interval(Duration::from_mins(1));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        registry.reap_expired().await;
                    }
                    () = shutdown.cancelled() => {
                        debug!("turn buffer reaper shutting down");
                        return;
                    }
                }
            }
        }
        .instrument(tracing::info_span!("pylon.turn_buffer_reaper")),
    );
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test: indices validated by assertions"
)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn buffer_records_events_with_monotonic_seq() {
        let registry = TurnBufferRegistry::new();
        let buf = registry.get_or_create("ses-1", "turn-1").await;
        let handle = TurnBufferHandle::new(buf);

        let seq1 = handle.record("text_delta", r#"{"text":"hello"}"#).await;
        let seq2 = handle.record("text_delta", r#"{"text":" world"}"#).await;
        let seq3 = handle
            .record("message_complete", r#"{"stop_reason":"end_turn"}"#)
            .await;

        assert_eq!(seq1, 1);
        assert_eq!(seq2, 2);
        assert_eq!(seq3, 3);
    }

    #[tokio::test]
    async fn events_after_returns_subset() {
        let registry = TurnBufferRegistry::new();
        let buf = registry.get_or_create("ses-1", "turn-1").await;
        let handle = TurnBufferHandle::new(buf);

        handle.record("text_delta", r#"{"text":"a"}"#).await;
        handle.record("text_delta", r#"{"text":"b"}"#).await;
        handle.record("text_delta", r#"{"text":"c"}"#).await;

        let (events, state) = handle.events_after(1).await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 2);
        assert_eq!(events[1].seq, 3);
        assert_eq!(state, TurnState::Running);
    }

    #[tokio::test]
    async fn events_after_zero_returns_all() {
        let registry = TurnBufferRegistry::new();
        let buf = registry.get_or_create("ses-1", "turn-1").await;
        let handle = TurnBufferHandle::new(buf);

        handle.record("text_delta", r#"{"text":"a"}"#).await;
        handle.record("text_delta", r#"{"text":"b"}"#).await;

        let (events, _) = handle.events_after(0).await;
        assert_eq!(events.len(), 2);
    }

    #[tokio::test]
    async fn mark_completed_sets_state() {
        let registry = TurnBufferRegistry::new();
        let buf = registry.get_or_create("ses-1", "turn-1").await;
        let handle = TurnBufferHandle::new(buf);

        handle.record("message_complete", "{}").await;
        handle.mark_completed().await;

        let (_, state) = handle.events_after(0).await;
        assert_eq!(state, TurnState::Completed);
    }

    #[tokio::test]
    async fn mark_failed_sets_state() {
        let registry = TurnBufferRegistry::new();
        let buf = registry.get_or_create("ses-1", "turn-1").await;
        let handle = TurnBufferHandle::new(buf);

        handle.mark_failed().await;

        let (_, state) = handle.events_after(0).await;
        assert_eq!(state, TurnState::Failed);
    }

    #[tokio::test]
    async fn get_returns_none_for_unknown_turn() {
        let registry = TurnBufferRegistry::new();
        let result = registry.get("ses-1", "unknown").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn get_returns_existing_buffer() {
        let registry = TurnBufferRegistry::new();
        let _ = registry.get_or_create("ses-1", "turn-1").await;
        let result = registry.get("ses-1", "turn-1").await;
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn reap_removes_expired_buffers() {
        let registry = TurnBufferRegistry {
            buffers: Mutex::new(HashMap::new()),
            // WHY: use zero TTL so the buffer expires immediately for testing
            completed_ttl: Duration::from_secs(0),
        };

        let buf = registry.get_or_create("ses-1", "turn-1").await;
        let handle = TurnBufferHandle::new(buf);
        handle.mark_completed().await;

        // Small delay so elapsed > 0
        tokio::time::sleep(Duration::from_millis(10)).await;

        registry.reap_expired().await;
        assert!(registry.get("ses-1", "turn-1").await.is_none());
    }

    #[tokio::test]
    async fn reap_keeps_running_buffers() {
        let registry = TurnBufferRegistry {
            buffers: Mutex::new(HashMap::new()),
            completed_ttl: Duration::from_secs(0),
        };

        let _ = registry.get_or_create("ses-1", "turn-1").await;
        // Buffer is still Running (not completed)

        registry.reap_expired().await;
        assert!(
            registry.get("ses-1", "turn-1").await.is_some(),
            "running buffers must not be reaped"
        );
    }

    #[tokio::test]
    async fn buffer_caps_at_max_events() {
        let registry = TurnBufferRegistry::new();
        let buf = registry.get_or_create("ses-1", "turn-1").await;
        let handle = TurnBufferHandle::new(buf);

        // Push MAX + 10 events
        for i in 0..(MAX_EVENTS_PER_TURN + 10) {
            handle
                .record("text_delta", &format!(r#"{{"text":"event-{i}"}}"#))
                .await;
        }

        let (events, _) = handle.events_after(0).await;
        assert_eq!(
            events.len(),
            MAX_EVENTS_PER_TURN,
            "buffer must cap at MAX_EVENTS_PER_TURN"
        );
    }
}
