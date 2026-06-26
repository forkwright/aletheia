//! In-memory turn event buffer for SSE client recovery.
//!
//! Each active turn gets a [`TurnBuffer`] that records every SSE event with a
//! monotonic sequence ID. When a client reconnects with `Last-Event-ID: N`,
//! the server replays events from `N+1` onward.
//!
//! Buffers are held in a [`TurnBufferRegistry`] keyed by `(session_id, turn_id)`.
//! Completed, failed, or aborted turns are retained for a configurable TTL
//! (default 5 minutes) so that late reconnects can still recover. A background
//! reaper task prunes expired entries.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::time::Instant;

use tokio::sync::futures::OwnedNotified;
use tokio::sync::{Mutex, Notify};
use tracing::{Instrument as _, debug, warn};

/// Default time-to-live for completed turn buffers before they are reaped.
const DEFAULT_COMPLETED_TTL: Duration = Duration::from_mins(5);

/// Maximum events retained per turn buffer to bound memory usage.
const DEFAULT_MAX_EVENTS_PER_TURN: usize = 10_000;

/// Retained marker emitted when a turn buffer reaches its capacity limit.
pub(crate) const REPLAY_GAP_EVENT_TYPE: &str = "replay_gap";

pub(crate) const REPLAY_GAP_REASON_BUFFER_CAPACITY: &str = "buffer_capacity";

/// Reason string for a turn aborted because the client disconnected.
pub(crate) const TURN_ABORT_REASON_CLIENT_DISCONNECT: &str = "client_disconnect";

/// Reason string for a turn aborted because the server is shutting down.
pub(crate) const TURN_ABORT_REASON_SERVER_SHUTDOWN: &str = "server_shutdown";

/// Reason string for a turn aborted because it exceeded its execution time limit.
pub(crate) const TURN_ABORT_REASON_TIMEOUT: &str = "timeout";

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

impl BufferedEvent {
    fn new(seq: u64, event_type: String, data: String) -> Self {
        Self {
            seq,
            event_type,
            data,
        }
    }
}

/// Result of recording an event into the replay buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecordOutcome {
    /// The caller's event was retained at this sequence number.
    Recorded { seq: u64 },
    /// The buffer retained a gap marker instead of the caller's event.
    ReplayGap {
        seq: u64,
        dropped_after_seq: u64,
        retained_limit: usize,
    },
    /// The buffer was already truncated; the caller's event was not retained.
    Dropped,
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
    /// Turn was aborted before completion (disconnect, shutdown, timeout, user cancel).
    Aborted { reason: String },
}

/// Per-turn event buffer.
///
/// Internally synchronized via `Mutex` so multiple producers (the streaming
/// task) and consumers (reconnecting clients) can access it concurrently.
pub(crate) struct TurnBuffer {
    events: Vec<BufferedEvent>,
    next_seq: u64,
    state: TurnState,
    max_events_per_turn: usize,
    truncated: bool,
    /// When the turn completed or failed (for TTL-based reaping).
    finished_at: Option<Instant>,
    /// Notified whenever a new event is appended. Reconnecting clients
    /// wait on this to receive live events after replaying the backlog.
    notify: Arc<Notify>,
}

impl TurnBuffer {
    fn new(max_events_per_turn: usize) -> Self {
        Self {
            events: Vec::new(),
            next_seq: 1,
            state: TurnState::Running,
            max_events_per_turn,
            truncated: false,
            finished_at: None,
            notify: Arc::new(Notify::new()),
        }
    }

    /// Append an event and return its assigned sequence number.
    fn push(&mut self, event_type: String, data: String) -> RecordOutcome {
        if self.truncated {
            return RecordOutcome::Dropped;
        }

        let seq = self.next_seq;

        if self.events.len() < self.max_events_per_turn {
            self.next_seq += 1;
            self.events.push(BufferedEvent::new(seq, event_type, data));
            self.notify.notify_waiters();
            return RecordOutcome::Recorded { seq };
        }

        let dropped_after_seq = self
            .events
            .last()
            .map_or(0, |event| event.seq.saturating_sub(1));
        let retained_limit = self.max_events_per_turn;
        let gap = BufferedEvent::new(
            seq,
            REPLAY_GAP_EVENT_TYPE.to_owned(),
            serde_json::json!({
                "type": REPLAY_GAP_EVENT_TYPE,
                "reason": REPLAY_GAP_REASON_BUFFER_CAPACITY,
                "dropped_after_seq": dropped_after_seq,
                "retained_limit": retained_limit,
            })
            .to_string(),
        );
        if !self.events.is_empty() {
            self.events.pop();
        }
        self.events.push(gap);
        self.next_seq += 1;
        self.truncated = true;

        warn!(
            seq,
            retained_limit, "turn buffer reached max capacity, emitting replay gap"
        );
        self.notify.notify_waiters();
        RecordOutcome::ReplayGap {
            seq,
            dropped_after_seq,
            retained_limit,
        }
    }

    /// Mark the turn as completed or failed.
    fn finish(&mut self, state: TurnState) {
        self.state = state;
        self.finished_at = Some(Instant::now());
        self.notify.notify_waiters();
    }

    /// Mark the turn as aborted with a client-visible reason.
    fn abort(&mut self, reason: String) {
        self.state = TurnState::Aborted { reason };
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

/// Replay snapshot with a waiter already registered for the next change.
pub(crate) struct TurnBufferSnapshot {
    pub events: Vec<BufferedEvent>,
    pub state: TurnState,
    pub notified: Pin<Box<OwnedNotified>>,
}

impl TurnBufferSnapshot {
    fn new(
        events: Vec<BufferedEvent>,
        state: TurnState,
        notified: Pin<Box<OwnedNotified>>,
    ) -> Self {
        Self {
            events,
            state,
            notified,
        }
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
    max_events_per_turn: usize,
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
        Self::with_limits(DEFAULT_COMPLETED_TTL, DEFAULT_MAX_EVENTS_PER_TURN)
    }

    /// Create a registry with explicit replay retention limits.
    pub(crate) fn with_limits(completed_ttl: Duration, max_events_per_turn: usize) -> Self {
        Self {
            buffers: Mutex::new(HashMap::new()),
            completed_ttl,
            max_events_per_turn: max_events_per_turn.max(1),
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
                .or_insert_with(|| Arc::new(Mutex::new(TurnBuffer::new(self.max_events_per_turn)))),
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

    /// Remove expired completed, failed, or aborted turn buffers.
    ///
    /// INVARIANT: the outer `buffers` lock is never held across an inner
    /// `Mutex::lock().await`. We snapshot the candidate entries under a brief
    /// outer critical section, release the outer lock, inspect each buffer, and
    /// only re-acquire the outer lock to remove confirmed-expired keys. This
    /// prevents concurrent `get_or_create`/`get` callers from stalling for the
    /// entire reaper sweep and eliminates the latent async deadlock surface from
    /// inconsistent outer→inner vs. inner-only lock ordering.
    pub async fn reap_expired(&self) {
        let ttl = self.completed_ttl;

        // WHY: clone the candidate set while holding the outer lock so the
        // subsequent per-buffer lock awaits cannot block registry-wide lookups.
        let candidates: Vec<(TurnKey, Arc<Mutex<TurnBuffer>>)> = {
            let map = self.buffers.lock().await;
            map.iter()
                .map(|(key, buffer)| (key.clone(), Arc::clone(buffer)))
                .collect()
        };

        let mut to_remove = Vec::new();
        for (key, buffer) in candidates {
            let buf = buffer.lock().await;
            if let Some(finished_at) = buf.finished_at
                && finished_at.elapsed() > ttl
            {
                to_remove.push(key);
            }
        }

        if to_remove.is_empty() {
            return;
        }

        let mut map = self.buffers.lock().await;
        let before = map.len();
        for key in &to_remove {
            map.remove(key);
        }
        let removed = to_remove.len();
        debug!(
            removed,
            remaining = before - removed,
            "reaped expired turn buffers"
        );
    }
}

/// Handle for a streaming handler to record events into a turn buffer.
///
/// Wraps `Arc<Mutex<TurnBuffer>>` with convenience methods. The embedded
/// `terminal` flag ensures that only the first terminal transition
/// (completed/failed/aborted) wins, even when both the turn task and the
/// disconnect Drop guard race to mark the buffer (#4794).
#[derive(Clone)]
pub(crate) struct TurnBufferHandle {
    inner: Arc<Mutex<TurnBuffer>>,
    terminal: Arc<AtomicBool>,
}

impl TurnBufferHandle {
    /// Create a handle from a buffer reference.
    pub(crate) fn new(buffer: Arc<Mutex<TurnBuffer>>) -> Self {
        Self {
            inner: buffer,
            terminal: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Record an SSE event. Returns the retained record outcome.
    pub(crate) async fn record(&self, event_type: &str, data: &str) -> RecordOutcome {
        let mut buf = self.inner.lock().await;
        buf.push(event_type.to_owned(), data.to_owned())
    }

    /// Mark the turn as completed if it is not already terminal.
    pub(crate) async fn mark_completed(&self) {
        if self
            .terminal
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let mut buf = self.inner.lock().await;
        buf.finish(TurnState::Completed);
    }

    /// Mark the turn as failed if it is not already terminal.
    pub(crate) async fn mark_failed(&self) {
        if self
            .terminal
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let mut buf = self.inner.lock().await;
        buf.finish(TurnState::Failed);
    }

    /// Mark the turn as aborted with a client-visible reason if it is not
    /// already terminal.
    pub(crate) async fn mark_aborted(&self, reason: &str) {
        if self
            .terminal
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let mut buf = self.inner.lock().await;
        buf.abort(reason.to_owned());
    }

    /// Get events after a given sequence number, plus the current turn state.
    #[cfg(test)]
    pub(crate) async fn events_after(&self, after_seq: u64) -> (Vec<BufferedEvent>, TurnState) {
        let buf = self.inner.lock().await;
        (buf.events_after(after_seq), buf.state.clone())
    }

    /// Get a replay snapshot with a waiter armed before the lock is released.
    pub(crate) async fn snapshot_after(&self, after_seq: u64) -> TurnBufferSnapshot {
        let buf = self.inner.lock().await;
        let mut notified = Box::pin(Arc::clone(&buf.notify).notified_owned());
        notified.as_mut().enable();
        TurnBufferSnapshot::new(buf.events_after(after_seq), buf.state.clone(), notified)
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

        assert_eq!(seq1, RecordOutcome::Recorded { seq: 1 });
        assert_eq!(seq2, RecordOutcome::Recorded { seq: 2 });
        assert_eq!(seq3, RecordOutcome::Recorded { seq: 3 });
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
    async fn reap_does_not_hold_outer_lock_across_inner_await() {
        // WHY: if reap_expired kept the outer registry lock while awaiting each
        // inner buffer lock, holding one inner lock would stall every concurrent
        // get_or_create/get call. This test demonstrates the outer lock is
        // released before the inner lock is acquired.
        let registry = Arc::new(TurnBufferRegistry::new());

        let buf = registry.get_or_create("ses-1", "slow-turn").await;
        let inner_guard = buf.lock().await;

        let registry_for_reap = Arc::clone(&registry);
        let reap_task = tokio::spawn(async move {
            registry_for_reap.reap_expired().await;
        });

        // Give the reaper a chance to reach the blocked inner lock await.
        tokio::task::yield_now().await;

        let result = tokio::time::timeout(
            Duration::from_secs(1),
            registry.get_or_create("ses-1", "other-turn"),
        )
        .await;
        assert!(
            result.is_ok(),
            "get_or_create must not be blocked while reaper awaits an inner lock"
        );

        drop(inner_guard);
        assert!(reap_task.await.is_ok(), "reaper task panicked");
    }

    #[tokio::test]
    async fn reap_removes_expired_buffers() {
        tokio::time::pause();
        let registry =
            TurnBufferRegistry::with_limits(Duration::from_secs(0), DEFAULT_MAX_EVENTS_PER_TURN);

        let buf = registry.get_or_create("ses-1", "turn-1").await;
        let handle = TurnBufferHandle::new(buf);
        handle.mark_completed().await;

        tokio::time::advance(Duration::from_millis(1)).await;

        registry.reap_expired().await;
        assert!(registry.get("ses-1", "turn-1").await.is_none());
    }

    #[tokio::test]
    async fn reap_keeps_running_buffers() {
        let registry =
            TurnBufferRegistry::with_limits(Duration::from_secs(0), DEFAULT_MAX_EVENTS_PER_TURN);

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
        for i in 0..(DEFAULT_MAX_EVENTS_PER_TURN + 10) {
            handle
                .record("text_delta", &format!(r#"{{"text":"event-{i}"}}"#))
                .await;
        }

        let (events, _) = handle.events_after(0).await;
        assert_eq!(
            events.len(),
            DEFAULT_MAX_EVENTS_PER_TURN,
            "buffer must cap at DEFAULT_MAX_EVENTS_PER_TURN"
        );
    }

    #[tokio::test]
    async fn buffer_emits_replay_gap_at_capacity() {
        let registry = TurnBufferRegistry::with_limits(DEFAULT_COMPLETED_TTL, 3);
        let buf = registry.get_or_create("ses-1", "turn-1").await;
        let handle = TurnBufferHandle::new(buf);

        assert_eq!(
            handle.record("text_delta", r#"{"text":"a"}"#).await,
            RecordOutcome::Recorded { seq: 1 }
        );
        assert_eq!(
            handle.record("text_delta", r#"{"text":"b"}"#).await,
            RecordOutcome::Recorded { seq: 2 }
        );
        assert_eq!(
            handle.record("text_delta", r#"{"text":"c"}"#).await,
            RecordOutcome::Recorded { seq: 3 }
        );

        assert_eq!(
            handle.record("text_delta", r#"{"text":"d"}"#).await,
            RecordOutcome::ReplayGap {
                seq: 4,
                dropped_after_seq: 2,
                retained_limit: 3,
            }
        );
        assert_eq!(
            handle.record("text_delta", r#"{"text":"e"}"#).await,
            RecordOutcome::Dropped
        );

        let (events, _) = handle.events_after(0).await;
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[1].seq, 2);
        assert_eq!(events[2].seq, 4);
        assert_eq!(events[2].event_type, REPLAY_GAP_EVENT_TYPE);
        let Ok(gap) = serde_json::from_str::<serde_json::Value>(&events[2].data) else {
            panic!("gap json");
        };
        assert_eq!(gap["type"], REPLAY_GAP_EVENT_TYPE);
        assert_eq!(gap["reason"], REPLAY_GAP_REASON_BUFFER_CAPACITY);
        assert_eq!(gap["dropped_after_seq"], 2);
        assert_eq!(gap["retained_limit"], 3);
    }

    #[tokio::test]
    async fn mark_aborted_sets_terminal_state_with_reason() {
        let registry = TurnBufferRegistry::new();
        let buf = registry.get_or_create("ses-1", "turn-1").await;
        let handle = TurnBufferHandle::new(buf);

        handle
            .mark_aborted(TURN_ABORT_REASON_CLIENT_DISCONNECT)
            .await;

        let (_, state) = handle.events_after(0).await;
        assert_eq!(
            state,
            TurnState::Aborted {
                reason: TURN_ABORT_REASON_CLIENT_DISCONNECT.to_owned(),
            }
        );
    }

    #[tokio::test]
    async fn reap_removes_expired_aborted_buffers() {
        tokio::time::pause();
        let registry =
            TurnBufferRegistry::with_limits(Duration::from_secs(0), DEFAULT_MAX_EVENTS_PER_TURN);

        let buf = registry.get_or_create("ses-1", "turn-1").await;
        let handle = TurnBufferHandle::new(buf);
        handle.mark_aborted(TURN_ABORT_REASON_SERVER_SHUTDOWN).await;

        tokio::time::advance(Duration::from_millis(1)).await;

        registry.reap_expired().await;
        assert!(registry.get("ses-1", "turn-1").await.is_none());
    }

    #[tokio::test]
    async fn aborted_buffer_retains_events_before_reap() {
        let registry = TurnBufferRegistry::new();
        let buf = registry.get_or_create("ses-1", "turn-1").await;
        let handle = TurnBufferHandle::new(buf);

        handle.record("text_delta", r#"{"text":"partial"}"#).await;
        handle.mark_aborted(TURN_ABORT_REASON_TIMEOUT).await;

        let (events, state) = handle.events_after(0).await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "text_delta");
        assert_eq!(
            state,
            TurnState::Aborted {
                reason: TURN_ABORT_REASON_TIMEOUT.to_owned(),
            }
        );
    }
}
