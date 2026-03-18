//! Cross-nous messaging: fire-and-forget, request-response, and delivery audit.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{instrument, warn};
use ulid::Ulid;

use crate::error::{
    self, AskCycleDetectedSnafu, AskTimeoutSnafu, DeliveryFailedSnafu, NousNotFoundSnafu,
    ReplyNotFoundSnafu,
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
}

/// Tracks in-flight ask edges for cycle detection.
///
/// Each entry maps `from_nous -> to_nous` for a pending ask. Before adding
/// a new edge, [`AskGraph::check_cycle`] walks the graph to detect whether
/// the new edge would close a cycle (direct or indirect).
pub(crate) struct AskGraph {
    /// Adjacency list: `from -> set of to`.
    edges: HashMap<String, Vec<String>>,
}

impl AskGraph {
    fn new() -> Self {
        Self {
            edges: HashMap::new(),
        }
    }

    /// Add an ask edge. Returns `Ok(())` if no cycle, or `Err(chain)` with
    /// the full cycle path if adding this edge would create a cycle.
    fn add_edge(&mut self, from: &str, to: &str) -> std::result::Result<(), Vec<String>> {
        // WHY: Walk the graph from `to` to see if we can reach `from`.
        // If so, adding `from -> to` closes a cycle.
        let mut chain = vec![from.to_owned(), to.to_owned()];
        let mut current = to;

        loop {
            let Some(targets) = self.edges.get(current) else {
                break;
            };

            for next in targets {
                if next == from {
                    chain.push(next.clone());
                    return Err(chain);
                }
            }

            // NOTE: For simplicity, follow the first outgoing edge. In practice,
            // an actor has at most one outstanding ask at a time (single-threaded
            // actor loop), so each node has at most one outgoing edge.
            if let Some(next) = targets.first() {
                chain.push(next.clone());
                current = next;
            } else {
                break;
            }
        }

        self.edges
            .entry(from.to_owned())
            .or_default()
            .push(to.to_owned());
        Ok(())
    }

    /// Remove a previously added ask edge.
    fn remove_edge(&mut self, from: &str, to: &str) {
        if let Some(targets) = self.edges.get_mut(from) {
            if let Some(pos) = targets.iter().position(|t| t == to) {
                targets.swap_remove(pos);
            }
            if targets.is_empty() {
                self.edges.remove(from);
            }
        }
    }

    /// Number of edges currently tracked.
    #[cfg(test)]
    fn edge_count(&self) -> usize {
        self.edges.values().map(Vec::len).sum()
    }
}

/// Routes cross-nous messages between registered actors.
pub struct CrossNousRouter {
    /// Maps nous id to its inbox sender. Invariant: every spawned actor has
    /// exactly one entry; removed on unregister. Held briefly during
    /// send/register/unregister.
    routes: Arc<RwLock<HashMap<String, mpsc::Sender<CrossNousEnvelope>>>>,
    /// Maps correlation id to the one-shot reply channel for an in-flight ask.
    /// Invariant: each ask inserts one entry; consumed exactly once on reply
    /// or removed on timeout.
    pending_replies: Arc<RwLock<HashMap<Ulid, oneshot::Sender<CrossNousReply>>>>,
    /// Append-only audit log of delivered messages. Invariant: entries are
    /// never modified after insertion; the log is read for diagnostics only.
    delivery_log: Arc<RwLock<DeliveryLog>>,
    /// Directed graph of in-flight ask chains used for cycle detection.
    /// Invariant: an edge exists iff a pending ask is outstanding between
    /// the two nodes; removed when the reply arrives or the ask times out.
    ask_graph: Arc<RwLock<AskGraph>>,
}

impl Clone for CrossNousRouter {
    fn clone(&self) -> Self {
        Self {
            routes: Arc::clone(&self.routes),
            pending_replies: Arc::clone(&self.pending_replies),
            delivery_log: Arc::clone(&self.delivery_log),
            ask_graph: Arc::clone(&self.ask_graph),
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
            ask_graph: Arc::new(RwLock::new(AskGraph::new())),
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

        let envelope = CrossNousEnvelope { message };

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
    /// Checks for cycles in the ask dependency graph before dispatching.
    /// If Actor A is already waiting on Actor B and B tries to ask A, the
    /// cycle is detected and an error is returned immediately.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled after sending the message, a pending reply
    /// entry is leaked until timeout cleanup. Do not use in `select!` branches.
    ///
    /// # Errors
    ///
    /// Returns [`error::Error::AskCycleDetected`] if the ask would create a cycle.
    /// Returns [`error::Error::NousNotFound`] if the target is not registered.
    /// Returns [`error::Error::DeliveryFailed`] if the target inbox is closed.
    /// Returns [`error::Error::AskTimeout`] if no reply arrives within the timeout.
    #[instrument(skip(self, message), fields(msg_id = %message.id, from = %message.from, to = %message.to))]
    pub async fn ask(&self, mut message: CrossNousMessage) -> error::Result<CrossNousReply> {
        let from = message.from.clone();
        let to = message.to.clone();
        let timeout_dur = message.reply_timeout.unwrap_or(DEFAULT_REPLY_TIMEOUT);
        message.expects_reply = true;

        // Cycle detection: check before we block.
        {
            let mut graph = self.ask_graph.write().await;
            if let Err(chain) = graph.add_edge(&from, &to) {
                let chain_str = chain.join(" -> ");
                warn!(chain = %chain_str, "ask cycle detected, returning error to prevent deadlock");
                return AskCycleDetectedSnafu { chain: chain_str }.fail();
            }
        }

        // INVARIANT: from this point, the edge is in the graph and must be
        // removed on every exit path (success, failure, timeout).
        let result = self.ask_inner(&mut message, &to, timeout_dur).await;

        self.ask_graph.write().await.remove_edge(&from, &to);

        result
    }

    /// Inner ask logic, factored out so the caller can guarantee edge cleanup.
    async fn ask_inner(
        &self,
        message: &mut CrossNousMessage,
        to: &str,
        timeout_dur: Duration,
    ) -> error::Result<CrossNousReply> {
        let routes = self.routes.read().await;
        let Some(sender) = routes.get(to).cloned() else {
            drop(routes);
            self.log_delivery(
                message,
                &DeliveryState::Failed {
                    reason: format!("nous '{to}' not registered"),
                },
            )
            .await;
            return NousNotFoundSnafu {
                nous_id: to.to_owned(),
            }
            .fail();
        };
        drop(routes);

        let (reply_tx, reply_rx) = oneshot::channel();
        let msg_id = message.id;

        self.pending_replies.write().await.insert(msg_id, reply_tx);

        let envelope = CrossNousEnvelope {
            message: message.clone(),
        };

        if sender.send(envelope).await.is_err() {
            self.pending_replies.write().await.remove(&msg_id);
            self.log_delivery(
                message,
                &DeliveryState::Failed {
                    reason: "inbox closed".to_owned(),
                },
            )
            .await;
            return DeliveryFailedSnafu {
                nous_id: to.to_owned(),
            }
            .fail();
        }

        self.log_delivery(message, &DeliveryState::Delivered).await;

        tokio::select! {
            result = reply_rx => {
                if let Ok(reply) = result {
                    self.log_delivery(message, &DeliveryState::Replied).await;
                    Ok(reply)
                } else {
                    self.log_delivery(message, &DeliveryState::Failed {
                        reason: "reply channel dropped".to_owned(),
                    }).await;
                    DeliveryFailedSnafu { nous_id: to.to_owned() }.fail()
                }
            }
            () = tokio::time::sleep(timeout_dur) => {
                self.pending_replies.write().await.remove(&msg_id);
                self.log_delivery(message, &DeliveryState::TimedOut).await;
                AskTimeoutSnafu {
                    nous_id: to.to_owned(),
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
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;
    use tracing::Instrument;

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

        let ask_handle = tokio::spawn(
            async move { router_for_reply.ask(msg).await }
                .instrument(tracing::info_span!("test_ask")),
        );

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
            handles.push(tokio::spawn(
                async move {
                    let msg =
                        CrossNousMessage::new(format!("sender-{i}"), "target", format!("msg-{i}"));
                    r.send(msg).await
                }
                .instrument(tracing::info_span!("test_send", index = i)),
            ));
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

    // --- Cycle detection tests ---

    #[test]
    fn ask_graph_direct_cycle_detected() {
        let mut graph = AskGraph::new();
        graph.add_edge("a", "b").unwrap();
        let err = graph.add_edge("b", "a").unwrap_err();
        assert_eq!(err, vec!["b", "a", "b"]);
    }

    #[test]
    fn ask_graph_indirect_cycle_detected() {
        let mut graph = AskGraph::new();
        graph.add_edge("a", "b").unwrap();
        graph.add_edge("b", "c").unwrap();
        let err = graph.add_edge("c", "a").unwrap_err();
        assert_eq!(err, vec!["c", "a", "b", "c"]);
    }

    #[test]
    fn ask_graph_no_false_positive_on_shared_target() {
        let mut graph = AskGraph::new();
        graph.add_edge("a", "b").unwrap();
        // c asking b is fine: no cycle.
        graph.add_edge("c", "b").unwrap();
        assert_eq!(graph.edge_count(), 2);
    }

    #[test]
    fn ask_graph_edge_removed_after_completion() {
        let mut graph = AskGraph::new();
        graph.add_edge("a", "b").unwrap();
        assert_eq!(graph.edge_count(), 1);
        graph.remove_edge("a", "b");
        assert_eq!(graph.edge_count(), 0);
        // Now b -> a should succeed since a -> b was removed.
        graph.add_edge("b", "a").unwrap();
        assert_eq!(graph.edge_count(), 1);
    }

    #[test]
    fn ask_graph_remove_nonexistent_edge_is_noop() {
        let mut graph = AskGraph::new();
        graph.remove_edge("x", "y");
        assert_eq!(graph.edge_count(), 0);
    }

    #[tokio::test]
    async fn ask_detects_direct_cycle() {
        let router = CrossNousRouter::default();
        let (tx_a, _rx_a) = mpsc::channel(32);
        let (tx_b, _rx_b) = mpsc::channel(32);
        router.register("a", tx_a).await;
        router.register("b", tx_b).await;

        // Simulate a -> b already in flight by adding the edge directly.
        router.ask_graph.write().await.add_edge("a", "b").unwrap();

        // Now b tries to ask a: should detect cycle.
        let msg = CrossNousMessage::new("b", "a", "question").with_reply(Duration::from_secs(1));
        let err = router.ask(msg).await.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("cycle detected"),
            "expected cycle error, got: {err_msg}"
        );
        assert!(
            err_msg.contains("b -> a -> b"),
            "expected chain in error, got: {err_msg}"
        );
    }

    #[tokio::test]
    async fn ask_detects_indirect_cycle() {
        let router = CrossNousRouter::default();
        let (tx_a, _rx_a) = mpsc::channel(32);
        let (tx_b, _rx_b) = mpsc::channel(32);
        let (tx_c, _rx_c) = mpsc::channel(32);
        router.register("a", tx_a).await;
        router.register("b", tx_b).await;
        router.register("c", tx_c).await;

        // a -> b and b -> c already in flight.
        {
            let mut graph = router.ask_graph.write().await;
            graph.add_edge("a", "b").unwrap();
            graph.add_edge("b", "c").unwrap();
        }

        // c tries to ask a: should detect indirect cycle.
        let msg = CrossNousMessage::new("c", "a", "question").with_reply(Duration::from_secs(1));
        let err = router.ask(msg).await.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("cycle detected"),
            "expected cycle error, got: {err_msg}"
        );
        assert!(
            err_msg.contains("c -> a -> b -> c"),
            "expected full chain, got: {err_msg}"
        );
    }

    #[tokio::test]
    async fn ask_no_false_positive_concurrent_asks() {
        let (router, mut rx) = setup_router().await;
        let (tx_c, _rx_c) = mpsc::channel(32);
        router.register("sender_c", tx_c).await;
        let router_reply = router.clone();

        // Two independent actors ask the same target: no cycle.
        let msg =
            CrossNousMessage::new("sender_c", "target", "hello").with_reply(Duration::from_secs(5));

        let ask_handle = tokio::spawn({
            let r = router.clone();
            async move { r.ask(msg).await }
        });

        let envelope = rx.recv().await.unwrap();
        let reply = CrossNousReply {
            in_reply_to: envelope.message.id,
            from: "target".to_owned(),
            content: "ok".to_owned(),
            created_at: jiff::Timestamp::now(),
        };
        router_reply.reply(reply).await.unwrap();

        let result = ask_handle.await.unwrap();
        assert!(result.is_ok(), "expected success, got: {result:?}");

        // Graph should be clean after ask completes.
        assert_eq!(router.ask_graph.read().await.edge_count(), 0);
    }

    #[tokio::test]
    async fn ask_graph_cleaned_up_on_timeout() {
        let (router, _rx) = setup_router().await;
        let msg = CrossNousMessage::new("sender", "target", "question")
            .with_reply(Duration::from_millis(10));

        let router_check = router.clone();

        let err = router.ask(msg).await.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("timed out"),
            "expected timeout, got: {err_msg}"
        );

        // Graph must be clean after timeout.
        assert_eq!(router_check.ask_graph.read().await.edge_count(), 0);
    }

    #[tokio::test]
    async fn ask_graph_cleaned_up_on_delivery_failure() {
        let router = CrossNousRouter::default();
        let (tx, rx) = mpsc::channel(1);
        router.register("target", tx).await;
        drop(rx);

        let msg =
            CrossNousMessage::new("sender", "target", "hello").with_reply(Duration::from_secs(1));
        let err = router.ask(msg).await;
        assert!(err.is_err());

        // Graph must be clean after delivery failure.
        assert_eq!(router.ask_graph.read().await.edge_count(), 0);
    }
}
