//! Cross-nous messaging — fire-and-forget, request-response, and delivery audit.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{instrument, warn};
use ulid::Ulid;

use crate::error::{
    self, AskTimeoutSnafu, DeliveryFailedSnafu, NousNotFoundSnafu, ReplyNotFoundSnafu,
};

const DEFAULT_REPLY_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_LOG_ENTRIES: usize = 1000;

/// Lifecycle state of a cross-nous message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum DeliveryState {
    /// Message created but not yet sent.
    Pending,
    /// Message placed in the target actor's inbox.
    Delivered,
    /// Target acknowledged receipt (reserved for future use).
    Acknowledged,
    /// A reply was received for this message.
    Replied,
    /// Delivery failed with the given reason.
    Failed { reason: String },
    /// Reply was not received within the timeout window.
    TimedOut,
}

/// A message from one nous to another.
#[derive(Debug, Clone)]
pub struct CrossNousMessage {
    /// Unique message identifier.
    pub id: Ulid,
    /// Sender nous ID.
    pub from: String,
    /// Target nous ID.
    pub to: String,
    /// Session key on the target nous to inject the message into.
    pub target_session: String,
    /// Message text payload.
    pub content: String,
    /// Whether the sender expects a [`CrossNousReply`].
    pub expects_reply: bool,
    /// How long to wait for a reply before timing out.
    pub reply_timeout: Option<Duration>,
    /// When the message was created.
    pub created_at: jiff::Timestamp,
    /// Current delivery lifecycle state.
    pub delivery: DeliveryState,
}

impl CrossNousMessage {
    /// Create a fire-and-forget message targeting the default session.
    #[must_use]
    pub fn new(from: impl Into<String>, to: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: Ulid::new(),
            from: from.into(),
            to: to.into(),
            target_session: "main".to_owned(),
            content: content.into(),
            expects_reply: false,
            reply_timeout: None,
            created_at: jiff::Timestamp::now(),
            delivery: DeliveryState::Pending,
        }
    }

    /// Override the target session key (default is `"main"`).
    #[must_use]
    pub fn with_target_session(mut self, session: impl Into<String>) -> Self {
        self.target_session = session.into();
        self
    }

    /// Mark this message as expecting a reply within the given timeout.
    #[must_use]
    pub fn with_reply(mut self, timeout: Duration) -> Self {
        self.expects_reply = true;
        self.reply_timeout = Some(timeout);
        self
    }
}

/// Reply to a cross-nous message.
#[derive(Debug, Clone)]
pub struct CrossNousReply {
    /// ID of the original [`CrossNousMessage`] this replies to.
    pub in_reply_to: Ulid,
    /// Responding nous ID.
    pub from: String,
    /// Reply text payload.
    pub content: String,
    /// When the reply was created.
    pub created_at: jiff::Timestamp,
}

/// Envelope wrapping a message and optional reply channel.
pub struct CrossNousEnvelope {
    /// The cross-nous message.
    pub message: CrossNousMessage,
    #[expect(dead_code, reason = "reserved for future direct-reply integration")]
    pub(crate) reply_tx: Option<oneshot::Sender<CrossNousReply>>,
}

/// Drop guard that removes a `pending_replies` entry on all exit paths.
///
/// Uses `try_write()` for best-effort cleanup to avoid deadlocks when the
/// lock is already held by the current task.
struct PendingReplyGuard {
    map: Arc<RwLock<HashMap<Ulid, oneshot::Sender<CrossNousReply>>>>,
    id: Ulid,
}

impl Drop for PendingReplyGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = self.map.try_write() {
            map.remove(&self.id);
        }
    }
}

/// Routes cross-nous messages between registered actors.
pub struct CrossNousRouter {
    routes: Arc<RwLock<HashMap<String, mpsc::Sender<CrossNousEnvelope>>>>,
    pending_replies: Arc<RwLock<HashMap<Ulid, oneshot::Sender<CrossNousReply>>>>,
    delivery_log: Arc<RwLock<DeliveryLog>>,
}

impl Clone for CrossNousRouter {
    fn clone(&self) -> Self {
        Self {
            routes: Arc::clone(&self.routes),
            pending_replies: Arc::clone(&self.pending_replies),
            delivery_log: Arc::clone(&self.delivery_log),
        }
    }
}

impl Default for CrossNousRouter {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_LOG_ENTRIES)
    }
}

impl CrossNousRouter {
    /// Create a router with the given delivery log capacity.
    #[must_use]
    pub fn new(max_log_entries: usize) -> Self {
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
            pending_replies: Arc::new(RwLock::new(HashMap::new())),
            delivery_log: Arc::new(RwLock::new(DeliveryLog::new(max_log_entries))),
        }
    }

    /// Register a nous actor's inbox so it can receive cross-nous messages.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Uses a single `RwLock::write` + `insert`.
    #[instrument(skip(self, sender))]
    pub async fn register(
        &self,
        nous_id: impl Into<String> + std::fmt::Debug,
        sender: mpsc::Sender<CrossNousEnvelope>,
    ) {
        let id = nous_id.into();
        self.routes.write().await.insert(id, sender);
    }

    /// Remove a nous actor's route, preventing further message delivery.
    #[instrument(skip(self))]
    pub async fn unregister(&self, nous_id: &str) {
        self.routes.write().await.remove(nous_id);
    }

    /// Fire-and-forget send. Returns `Delivered` on success.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled after `mpsc::send` succeeds, the message
    /// is delivered but the caller never sees the `Delivered` result.
    #[instrument(skip(self, message), fields(msg_id = %message.id, from = %message.from, to = %message.to))]
    pub async fn send(&self, message: CrossNousMessage) -> error::Result<DeliveryState> {
        let to = message.to.clone();

        let routes = self.routes.read().await;
        let Some(sender) = routes.get(&to).cloned() else {
            drop(routes);
            self.log_delivery(
                &message,
                &DeliveryState::Failed {
                    reason: format!("nous '{to}' not registered"),
                },
            )
            .await;
            return NousNotFoundSnafu { nous_id: to }.fail();
        };
        drop(routes);

        let envelope = CrossNousEnvelope {
            message,
            reply_tx: None,
        };

        match sender.send(envelope).await {
            Ok(()) => {
                self.log_delivery_state(&to, DeliveryState::Delivered).await;
                Ok(DeliveryState::Delivered)
            }
            Err(send_err) => {
                let state = DeliveryState::Failed {
                    reason: "inbox closed".to_owned(),
                };
                self.log_delivery(&send_err.0.message, &state).await;
                DeliveryFailedSnafu { nous_id: to }.fail()
            }
        }
    }

    /// Send and wait for reply. Returns the reply or a timeout error.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled after sending the message, a pending reply
    /// entry is leaked until timeout cleanup. Do not use in `select!` branches.
    #[instrument(skip(self, message), fields(msg_id = %message.id, from = %message.from, to = %message.to))]
    pub async fn ask(&self, mut message: CrossNousMessage) -> error::Result<CrossNousReply> {
        let to = message.to.clone();
        let timeout_dur = message.reply_timeout.unwrap_or(DEFAULT_REPLY_TIMEOUT);
        message.expects_reply = true;

        let routes = self.routes.read().await;
        let Some(sender) = routes.get(&to).cloned() else {
            drop(routes);
            self.log_delivery(
                &message,
                &DeliveryState::Failed {
                    reason: format!("nous '{to}' not registered"),
                },
            )
            .await;
            return NousNotFoundSnafu { nous_id: to }.fail();
        };
        drop(routes);

        let (reply_tx, reply_rx) = oneshot::channel();
        let msg_id = message.id;

        self.pending_replies.write().await.insert(msg_id, reply_tx);
        let _guard = PendingReplyGuard {
            map: Arc::clone(&self.pending_replies),
            id: msg_id,
        };

        let envelope = CrossNousEnvelope {
            message: message.clone(),
            reply_tx: None,
        };

        if sender.send(envelope).await.is_err() {
            self.log_delivery(
                &message,
                &DeliveryState::Failed {
                    reason: "inbox closed".to_owned(),
                },
            )
            .await;
            return DeliveryFailedSnafu { nous_id: to }.fail();
        }

        self.log_delivery(&message, &DeliveryState::Delivered).await;

        tokio::select! {
            result = reply_rx => {
                if let Ok(reply) = result {
                    self.log_delivery(&message, &DeliveryState::Replied).await;
                    Ok(reply)
                } else {
                    self.log_delivery(&message, &DeliveryState::Failed {
                        reason: "reply channel dropped".to_owned(),
                    }).await;
                    DeliveryFailedSnafu { nous_id: to }.fail()
                }
            }
            () = tokio::time::sleep(timeout_dur) => {
                self.log_delivery(&message, &DeliveryState::TimedOut).await;
                AskTimeoutSnafu {
                    nous_id: to,
                    timeout_secs: timeout_dur.as_secs(),
                }.fail()
            }
        }
    }

    /// Submit a reply for a pending ask.
    #[instrument(skip(self, reply), fields(in_reply_to = %reply.in_reply_to, from = %reply.from))]
    pub async fn reply(&self, reply: CrossNousReply) -> error::Result<()> {
        let msg_id = reply.in_reply_to;
        let Some(tx) = self.pending_replies.write().await.remove(&msg_id) else {
            warn!(message_id = %msg_id, "reply channel not found (expired or consumed)");
            return ReplyNotFoundSnafu {
                message_id: msg_id.to_string(),
            }
            .fail();
        };
        let _ = tx.send(reply);
        Ok(())
    }

    /// List all registered nous IDs.
    pub async fn registered(&self) -> Vec<String> {
        self.routes.read().await.keys().cloned().collect()
    }

    async fn log_delivery(&self, message: &CrossNousMessage, state: &DeliveryState) {
        self.delivery_log.write().await.record(DeliveryEntry {
            message_id: message.id,
            from: message.from.clone(),
            to: message.to.clone(),
            state: state.clone(),
            timestamp: jiff::Timestamp::now(),
        });
    }

    async fn log_delivery_state(&self, to: &str, state: DeliveryState) {
        self.delivery_log.write().await.record(DeliveryEntry {
            message_id: Ulid::new(),
            from: String::new(),
            to: to.to_owned(),
            state,
            timestamp: jiff::Timestamp::now(),
        });
    }
}

/// A single delivery audit record.
#[derive(Debug, Clone)]
pub struct DeliveryEntry {
    /// ID of the delivered message.
    pub message_id: Ulid,
    /// Sender nous ID.
    pub from: String,
    /// Target nous ID.
    pub to: String,
    /// Delivery outcome at the time of recording.
    pub state: DeliveryState,
    /// When this delivery event was recorded.
    pub timestamp: jiff::Timestamp,
}

/// Ring-buffer delivery audit log.
pub struct DeliveryLog {
    entries: VecDeque<DeliveryEntry>,
    max_entries: usize,
}

impl DeliveryLog {
    /// Create a delivery log with the given maximum capacity.
    #[must_use]
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries.min(1024)),
            max_entries,
        }
    }

    /// Append an entry, evicting the oldest if at capacity.
    pub fn record(&mut self, entry: DeliveryEntry) {
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    /// Most recent entries, newest first, up to `limit`.
    #[must_use]
    pub fn recent(&self, limit: usize) -> Vec<&DeliveryEntry> {
        self.entries.iter().rev().take(limit).collect()
    }

    /// Recent entries involving the given nous (as sender or receiver), newest first.
    #[must_use]
    pub fn for_nous(&self, nous_id: &str, limit: usize) -> Vec<&DeliveryEntry> {
        self.entries
            .iter()
            .rev()
            .filter(|e| e.from == nous_id || e.to == nous_id)
            .take(limit)
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    async fn setup_router() -> (CrossNousRouter, mpsc::Receiver<CrossNousEnvelope>) {
        let router = CrossNousRouter::default();
        let (tx, rx) = mpsc::channel(32);
        router.register("target", tx).await;
        (router, rx)
    }

    #[test]
    fn send_sync_assertions() {
        static_assertions::assert_impl_all!(CrossNousRouter: Send, Sync);
        static_assertions::assert_impl_all!(CrossNousMessage: Send, Sync);
        static_assertions::assert_impl_all!(CrossNousReply: Send, Sync);
        static_assertions::assert_impl_all!(DeliveryState: Send, Sync);
        static_assertions::assert_impl_all!(DeliveryEntry: Send, Sync);
        static_assertions::assert_impl_all!(DeliveryLog: Send);
    }

    #[tokio::test]
    async fn register_makes_nous_routable() {
        let router = CrossNousRouter::default();
        let (tx1, _rx1) = mpsc::channel(32);
        let (tx2, _rx2) = mpsc::channel(32);
        let (tx3, _rx3) = mpsc::channel(32);
        router.register("alpha", tx1).await;
        router.register("beta", tx2).await;
        router.register("gamma", tx3).await;

        let mut registered = router.registered().await;
        registered.sort();
        assert_eq!(registered, vec!["alpha", "beta", "gamma"]);
    }

    #[tokio::test]
    async fn unregister_removes_route() {
        let (router, _rx) = setup_router().await;
        router.unregister("target").await;
        assert!(router.registered().await.is_empty());
    }

    #[tokio::test]
    async fn send_delivers_message() {
        let (router, mut rx) = setup_router().await;
        let msg = CrossNousMessage::new("sender", "target", "hello");
        let state = router.send(msg).await.unwrap();
        assert_eq!(state, DeliveryState::Delivered);

        let envelope = rx.recv().await.unwrap();
        assert_eq!(envelope.message.content, "hello");
        assert_eq!(envelope.message.from, "sender");
    }

    #[tokio::test]
    async fn send_to_unknown_returns_error() {
        let router = CrossNousRouter::default();
        let msg = CrossNousMessage::new("sender", "ghost", "hello");
        let err = router.send(msg).await.unwrap_err();
        assert!(err.to_string().contains("ghost"));
    }

    #[tokio::test]
    async fn ask_receives_reply() {
        let (router, mut rx) = setup_router().await;
        let router_for_reply = router.clone();

        let msg = CrossNousMessage::new("sender", "target", "question")
            .with_reply(Duration::from_secs(5));

        let ask_handle = tokio::spawn(async move { router_for_reply.ask(msg).await });

        let envelope = rx.recv().await.unwrap();
        let reply = CrossNousReply {
            in_reply_to: envelope.message.id,
            from: "target".to_owned(),
            content: "answer".to_owned(),
            created_at: jiff::Timestamp::now(),
        };
        router.reply(reply).await.unwrap();

        let result = ask_handle.await.unwrap().unwrap();
        assert_eq!(result.content, "answer");
        assert_eq!(result.from, "target");
    }

    #[tokio::test]
    async fn ask_times_out() {
        let (router, _rx) = setup_router().await;
        let msg = CrossNousMessage::new("sender", "target", "question")
            .with_reply(Duration::from_millis(10));
        let err = router.ask(msg).await.unwrap_err();
        assert!(err.to_string().contains("timed out"));
    }

    #[tokio::test]
    async fn reply_to_expired_ask_returns_error() {
        let router = CrossNousRouter::default();
        let reply = CrossNousReply {
            in_reply_to: Ulid::new(),
            from: "target".to_owned(),
            content: "late".to_owned(),
            created_at: jiff::Timestamp::now(),
        };
        let err = router.reply(reply).await.unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[tokio::test]
    async fn delivery_log_records_entries() {
        let (router, _rx) = setup_router().await;
        let msg = CrossNousMessage::new("sender", "target", "hello");
        router.send(msg).await.unwrap();

        let log = router.delivery_log.read().await;
        let entries = log.recent(10);
        assert!(!entries.is_empty());
        assert_eq!(entries[0].to, "target");
    }

    #[tokio::test]
    async fn delivery_log_evicts_oldest() {
        let mut log = DeliveryLog::new(3);
        for i in 0..5 {
            log.record(DeliveryEntry {
                message_id: Ulid::new(),
                from: "a".to_owned(),
                to: format!("target-{i}"),
                state: DeliveryState::Delivered,
                timestamp: jiff::Timestamp::now(),
            });
        }
        assert_eq!(log.entries.len(), 3);
        let recent = log.recent(10);
        assert_eq!(recent[0].to, "target-4");
        assert_eq!(recent[2].to, "target-2");
    }

    #[tokio::test]
    async fn delivery_log_for_nous_filters() {
        let mut log = DeliveryLog::new(100);
        log.record(DeliveryEntry {
            message_id: Ulid::new(),
            from: "a".to_owned(),
            to: "b".to_owned(),
            state: DeliveryState::Delivered,
            timestamp: jiff::Timestamp::now(),
        });
        log.record(DeliveryEntry {
            message_id: Ulid::new(),
            from: "c".to_owned(),
            to: "d".to_owned(),
            state: DeliveryState::Delivered,
            timestamp: jiff::Timestamp::now(),
        });
        log.record(DeliveryEntry {
            message_id: Ulid::new(),
            from: "b".to_owned(),
            to: "e".to_owned(),
            state: DeliveryState::Delivered,
            timestamp: jiff::Timestamp::now(),
        });

        let entries = log.for_nous("b", 10);
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn concurrent_sends_all_deliver() {
        let (router, mut rx) = setup_router().await;

        let mut handles = Vec::new();
        for i in 0..10 {
            let r = router.clone();
            handles.push(tokio::spawn(async move {
                let msg =
                    CrossNousMessage::new(format!("sender-{i}"), "target", format!("msg-{i}"));
                r.send(msg).await
            }));
        }

        let mut successes = 0;
        for h in handles {
            if h.await.unwrap().is_ok() {
                successes += 1;
            }
        }
        assert_eq!(successes, 10);

        let mut received = 0;
        while rx.try_recv().is_ok() {
            received += 1;
        }
        assert_eq!(received, 10);
    }

    // --- Edge cases ---

    #[test]
    fn message_new_defaults() {
        let msg = CrossNousMessage::new("a", "b", "hello");
        assert_eq!(msg.from, "a");
        assert_eq!(msg.to, "b");
        assert_eq!(msg.content, "hello");
        assert_eq!(msg.target_session, "main");
        assert!(!msg.expects_reply);
        assert!(msg.reply_timeout.is_none());
        assert_eq!(msg.delivery, DeliveryState::Pending);
    }

    #[test]
    fn message_with_target_session() {
        let msg = CrossNousMessage::new("a", "b", "hello").with_target_session("custom-session");
        assert_eq!(msg.target_session, "custom-session");
    }

    #[test]
    fn message_with_reply() {
        let msg = CrossNousMessage::new("a", "b", "question").with_reply(Duration::from_secs(10));
        assert!(msg.expects_reply);
        assert_eq!(msg.reply_timeout, Some(Duration::from_secs(10)));
    }

    #[tokio::test]
    async fn send_to_closed_inbox_fails() {
        let router = CrossNousRouter::default();
        let (tx, rx) = mpsc::channel(1);
        router.register("closed", tx).await;
        drop(rx); // Close the receiver side

        let msg = CrossNousMessage::new("sender", "closed", "hello");
        let err = router.send(msg).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn ask_to_unknown_target_fails() {
        let router = CrossNousRouter::default();
        let msg = CrossNousMessage::new("sender", "ghost", "question")
            .with_reply(Duration::from_millis(10));
        let err = router.ask(msg).await;
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("ghost"));
    }

    #[tokio::test]
    async fn register_overwrites_previous() {
        let router = CrossNousRouter::default();
        let (tx1, _rx1) = mpsc::channel(32);
        let (tx2, _rx2) = mpsc::channel(32);
        router.register("alpha", tx1).await;
        router.register("alpha", tx2).await;

        let registered = router.registered().await;
        assert_eq!(registered.len(), 1);
    }

    #[test]
    fn delivery_log_recent_limit() {
        let mut log = DeliveryLog::new(100);
        for i in 0..10 {
            log.record(DeliveryEntry {
                message_id: Ulid::new(),
                from: "a".to_owned(),
                to: format!("b-{i}"),
                state: DeliveryState::Delivered,
                timestamp: jiff::Timestamp::now(),
            });
        }
        let recent = log.recent(3);
        assert_eq!(recent.len(), 3);
    }

    #[test]
    fn delivery_log_empty() {
        let log = DeliveryLog::new(10);
        assert!(log.recent(10).is_empty());
        assert!(log.for_nous("a", 10).is_empty());
    }

    #[test]
    fn delivery_state_serde_roundtrip() {
        let states = [
            DeliveryState::Pending,
            DeliveryState::Delivered,
            DeliveryState::Acknowledged,
            DeliveryState::Replied,
            DeliveryState::Failed {
                reason: "test".to_owned(),
            },
            DeliveryState::TimedOut,
        ];
        for state in &states {
            let json = serde_json::to_string(state).unwrap();
            let back: DeliveryState = serde_json::from_str(&json).unwrap();
            assert_eq!(*state, back);
        }
    }

    #[test]
    fn router_default_creates_with_default_capacity() {
        let router = CrossNousRouter::default();
        // Just verify it doesn't panic
        assert!(router.routes.try_read().is_ok());
    }

    #[tokio::test]
    async fn ask_cleans_up_pending_replies_on_success() {
        let (router, mut rx) = setup_router().await;
        let router_for_reply = router.clone();

        let msg = CrossNousMessage::new("sender", "target", "question")
            .with_reply(Duration::from_secs(5));

        let ask_handle = tokio::spawn(async move { router_for_reply.ask(msg).await });

        let envelope = rx.recv().await.unwrap();
        let reply = CrossNousReply {
            in_reply_to: envelope.message.id,
            from: "target".to_owned(),
            content: "answer".to_owned(),
            created_at: jiff::Timestamp::now(),
        };
        router.reply(reply).await.unwrap();

        let result = ask_handle.await.unwrap().unwrap();
        assert_eq!(result.content, "answer");

        assert!(
            router.pending_replies.read().await.is_empty(),
            "pending_replies should be empty after successful reply"
        );
    }

    #[tokio::test]
    async fn ask_cleans_up_pending_replies_on_timeout() {
        let (router, _rx) = setup_router().await;
        let msg = CrossNousMessage::new("sender", "target", "question")
            .with_reply(Duration::from_millis(10));
        let err = router.ask(msg).await;
        assert!(err.is_err());

        assert!(
            router.pending_replies.read().await.is_empty(),
            "pending_replies should be empty after timeout"
        );
    }
}
