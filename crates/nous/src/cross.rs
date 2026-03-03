//! Cross-nous messaging — fire-and-forget, request-response, and delivery audit.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, RwLock};
use tracing::{instrument, warn};
use ulid::Ulid;

use crate::error::{self, AskTimeoutSnafu, DeliveryFailedSnafu, NousNotFoundSnafu, ReplyNotFoundSnafu};

const DEFAULT_REPLY_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_MAX_LOG_ENTRIES: usize = 1000;

/// Lifecycle state of a cross-nous message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum DeliveryState {
    Pending,
    Delivered,
    Acknowledged,
    Replied,
    Failed { reason: String },
    TimedOut,
}

/// A message from one nous to another.
#[derive(Debug, Clone)]
pub struct CrossNousMessage {
    pub id: Ulid,
    pub from: String,
    pub to: String,
    pub target_session: String,
    pub content: String,
    pub expects_reply: bool,
    pub reply_timeout: Option<Duration>,
    pub created_at: jiff::Timestamp,
    pub delivery: DeliveryState,
}

impl CrossNousMessage {
    #[must_use]
    pub fn new(
        from: impl Into<String>,
        to: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
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

    #[must_use]
    pub fn with_target_session(mut self, session: impl Into<String>) -> Self {
        self.target_session = session.into();
        self
    }

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
    pub in_reply_to: Ulid,
    pub from: String,
    pub content: String,
    pub created_at: jiff::Timestamp,
}

/// Envelope wrapping a message and optional reply channel.
pub struct CrossNousEnvelope {
    /// The cross-nous message.
    pub message: CrossNousMessage,
    #[expect(dead_code, reason = "reserved for future direct-reply integration")]
    pub(crate) reply_tx: Option<oneshot::Sender<CrossNousReply>>,
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
    #[must_use]
    pub fn new(max_log_entries: usize) -> Self {
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
            pending_replies: Arc::new(RwLock::new(HashMap::new())),
            delivery_log: Arc::new(RwLock::new(DeliveryLog::new(max_log_entries))),
        }
    }

    #[instrument(skip(self, sender))]
    pub async fn register(
        &self,
        nous_id: impl Into<String> + std::fmt::Debug,
        sender: mpsc::Sender<CrossNousEnvelope>,
    ) {
        let id = nous_id.into();
        self.routes.write().await.insert(id, sender);
    }

    #[instrument(skip(self))]
    pub async fn unregister(&self, nous_id: &str) {
        self.routes.write().await.remove(nous_id);
    }

    /// Fire-and-forget send. Returns `Delivered` on success.
    #[instrument(skip(self, message), fields(msg_id = %message.id, from = %message.from, to = %message.to))]
    pub async fn send(&self, message: CrossNousMessage) -> error::Result<DeliveryState> {
        let to = message.to.clone();

        let routes = self.routes.read().await;
        let Some(sender) = routes.get(&to).cloned() else {
            drop(routes);
            self.log_delivery(&message, &DeliveryState::Failed {
                reason: format!("nous '{to}' not registered"),
            })
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
                self.log_delivery_state(&to, DeliveryState::Delivered)
                    .await;
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
    #[instrument(skip(self, message), fields(msg_id = %message.id, from = %message.from, to = %message.to))]
    pub async fn ask(&self, mut message: CrossNousMessage) -> error::Result<CrossNousReply> {
        let to = message.to.clone();
        let timeout_dur = message.reply_timeout.unwrap_or(DEFAULT_REPLY_TIMEOUT);
        message.expects_reply = true;

        let routes = self.routes.read().await;
        let Some(sender) = routes.get(&to).cloned() else {
            drop(routes);
            self.log_delivery(&message, &DeliveryState::Failed {
                reason: format!("nous '{to}' not registered"),
            })
            .await;
            return NousNotFoundSnafu { nous_id: to }.fail();
        };
        drop(routes);

        let (reply_tx, reply_rx) = oneshot::channel();
        let msg_id = message.id;

        self.pending_replies
            .write()
            .await
            .insert(msg_id, reply_tx);

        let envelope = CrossNousEnvelope {
            message: message.clone(),
            reply_tx: None,
        };

        if sender.send(envelope).await.is_err() {
            self.pending_replies.write().await.remove(&msg_id);
            self.log_delivery(&message, &DeliveryState::Failed {
                reason: "inbox closed".to_owned(),
            })
            .await;
            return DeliveryFailedSnafu { nous_id: to }.fail();
        }

        self.log_delivery(&message, &DeliveryState::Delivered)
            .await;

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
                self.pending_replies.write().await.remove(&msg_id);
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
    pub message_id: Ulid,
    pub from: String,
    pub to: String,
    pub state: DeliveryState,
    pub timestamp: jiff::Timestamp,
}

/// Ring-buffer delivery audit log.
pub struct DeliveryLog {
    entries: VecDeque<DeliveryEntry>,
    max_entries: usize,
}

impl DeliveryLog {
    #[must_use]
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(max_entries.min(1024)),
            max_entries,
        }
    }

    pub fn record(&mut self, entry: DeliveryEntry) {
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }

    #[must_use]
    pub fn recent(&self, limit: usize) -> Vec<&DeliveryEntry> {
        self.entries.iter().rev().take(limit).collect()
    }

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
}
