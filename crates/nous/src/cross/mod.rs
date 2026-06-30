// kanon:ignore RUST/file-too-long — cross-nous messaging module; knowledge sub-module extraction planned
//! Cross-nous messaging: fire-and-forget, request-response, and delivery audit.

use std::collections::HashMap;
use std::time::Duration;

use koina::ulid::Ulid;

pub mod knowledge;

pub(super) const DEFAULT_REPLY_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const DEFAULT_MAX_LOG_ENTRIES: usize = 1000;
const OPERATOR_SENDER_ID: &str = "operator";

/// Inbound address policy for a target nous.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AddressMask {
    /// Any sender may deliver to this target.
    #[default]
    Public,
    /// Only the operator sender may deliver to this target.
    OperatorOnly,
    /// Only the listed senders may deliver to this target.
    AllowList(Vec<String>),
}

impl AddressMask {
    /// Address policy derived from the effective agent privacy flag.
    ///
    /// Private nouses only accept operator-originated cross-nous messages.
    /// Public, shared, and team-scoped nouses remain broadly addressable until
    /// a future config model adds narrower inbound cohorts.
    #[must_use]
    pub fn for_agent_privacy(private: bool) -> Self {
        if private {
            Self::OperatorOnly
        } else {
            Self::Public
        }
    }

    /// Stable diagnostic label for this mask.
    #[must_use]
    pub fn diagnostic_kind(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::OperatorOnly => "operator_only",
            Self::AllowList(_) => "allow_list",
        }
    }

    /// Sender ids explicitly allowed by this mask.
    ///
    /// Empty for masks that do not use an allow-list.
    #[must_use]
    pub fn allowed_senders(&self) -> &[String] {
        match self {
            Self::AllowList(allowed) => allowed,
            Self::Public | Self::OperatorOnly => &[],
        }
    }

    fn permits(&self, sender: &str) -> bool {
        match self {
            Self::Public => true,
            Self::OperatorOnly => sender == OPERATOR_SENDER_ID,
            Self::AllowList(allowed) => allowed.iter().any(|id| id == sender),
        }
    }
}

/// Lifecycle state of a cross-nous message.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "variant fields (reason) are self-documenting by name"
)]
// kanon:ignore RUST/non-exhaustive-enum — already #[non_exhaustive]; false positive from attribute ordering
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
    /// Optional typed payload — see [`knowledge::KnowledgePayload`].
    pub payload: Option<knowledge::KnowledgePayload>,
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
            payload: None,
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

    /// Attach a typed [`knowledge::KnowledgePayload`].
    #[must_use]
    pub fn with_payload(mut self, payload: knowledge::KnowledgePayload) -> Self {
        self.payload = Some(payload);
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

        while let Some(targets) = self.edges.get(current) {
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

mod delivery;
/// Routes cross-nous messages between registered actors.
mod router;

pub(crate) use delivery::{DeliveryEntry, DeliveryLog};
pub use router::CrossNousRouter;

#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices are valid after asserting len"
)]
#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use tokio::sync::mpsc;
    use tracing::Instrument;

    use super::*;

    async fn setup_router() -> (CrossNousRouter, mpsc::Receiver<CrossNousEnvelope>) {
        let router = CrossNousRouter::default();
        let (tx, rx) = mpsc::channel(32);
        router.register("target", tx).await;
        (router, rx)
    }

    #[test]
    fn send_sync_assertions() {
        const _: fn() = || {
            fn assert_send_sync<T: Send + Sync>() {}
            fn assert_send<T: Send>() {}
            assert_send_sync::<CrossNousRouter>();
            assert_send_sync::<CrossNousMessage>();
            assert_send_sync::<CrossNousReply>();
            assert_send_sync::<DeliveryState>();
            assert_send_sync::<DeliveryEntry>();
            assert_send::<DeliveryLog>();
        };
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
    async fn send_success_log_preserves_message_attribution() {
        let (router, _rx) = setup_router().await;
        let msg = CrossNousMessage::new("sender", "target", "hello");
        let msg_id = msg.id;
        let state = router.send(msg).await.unwrap();
        assert_eq!(state, DeliveryState::Delivered);

        let log = router.delivery_log.read().await;
        let entries = log.recent(10);
        let delivered = entries
            .iter()
            .find(|e| matches!(e.state, DeliveryState::Delivered))
            .unwrap();
        assert_eq!(delivered.message_id, msg_id);
        assert_eq!(delivered.from, "sender");
        assert_eq!(delivered.to, "target");
        assert_eq!(delivered.state, DeliveryState::Delivered);
    }

    #[tokio::test]
    async fn address_mask_default_public_permits_send() {
        let (router, mut rx) = setup_router().await;
        let msg = CrossNousMessage::new("sender", "target", "hello");
        let state = router.send(msg).await.unwrap();
        assert_eq!(state, DeliveryState::Delivered);

        let envelope = rx.recv().await.unwrap();
        assert_eq!(envelope.message.from, "sender");
        assert_eq!(envelope.message.to, "target");
    }

    #[tokio::test]
    async fn register_with_address_mask_exposes_and_enforces_policy() {
        let router = CrossNousRouter::default();
        let (tx, mut rx) = mpsc::channel(32);
        router
            .register_with_address_mask("target", tx, AddressMask::OperatorOnly)
            .await;

        assert_eq!(
            router.address_mask("target").await,
            AddressMask::OperatorOnly
        );

        let rejected = CrossNousMessage::new("sender", "target", "hello");
        let err = router.send(rejected).await.unwrap_err();
        assert!(err.to_string().contains("address rejected"));
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));

        let allowed = CrossNousMessage::new("operator", "target", "hello");
        let state = router.send(allowed).await.unwrap();
        assert_eq!(state, DeliveryState::Delivered);
        assert_eq!(rx.recv().await.unwrap().message.from, "operator");
    }

    #[tokio::test]
    async fn set_address_mask_before_register_applies_pending_policy() {
        let router = CrossNousRouter::default();
        router
            .set_address_mask("target", AddressMask::AllowList(vec!["alice".to_owned()]))
            .await;

        assert_eq!(
            router.address_mask("target").await,
            AddressMask::AllowList(vec!["alice".to_owned()])
        );

        let (tx, mut rx) = mpsc::channel(32);
        router.register("target", tx).await;
        let rejected = CrossNousMessage::new("bob", "target", "hello");
        let err = router.send(rejected).await.unwrap_err();
        assert!(err.to_string().contains("address rejected"));

        let allowed = CrossNousMessage::new("alice", "target", "hello");
        let state = router.send(allowed).await.unwrap();
        assert_eq!(state, DeliveryState::Delivered);
        assert_eq!(rx.recv().await.unwrap().message.from, "alice");
    }

    #[tokio::test]
    async fn address_mask_operator_only_rejects_non_operator_and_permits_operator() {
        let (router, mut rx) = setup_router().await;
        router
            .set_address_mask("target", AddressMask::OperatorOnly)
            .await;

        let rejected = CrossNousMessage::new("sender", "target", "hello");
        let err = router.send(rejected).await.unwrap_err();
        assert!(err.to_string().contains("address rejected"));
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));

        let log = router.delivery_log.read().await;
        let entries = log.recent(1);
        assert!(matches!(
            entries[0].state,
            DeliveryState::Failed { ref reason } if reason.contains("address rejected")
        ));
        drop(log);

        let allowed = CrossNousMessage::new("operator", "target", "hello");
        let state = router.send(allowed).await.unwrap();
        assert_eq!(state, DeliveryState::Delivered);

        let envelope = rx.recv().await.unwrap();
        assert_eq!(envelope.message.from, "operator");
    }

    #[tokio::test]
    async fn address_mask_allow_list_permits_listed_sender_and_rejects_unlisted_sender() {
        let (router, mut rx) = setup_router().await;
        router
            .set_address_mask("target", AddressMask::AllowList(vec!["alice".to_owned()]))
            .await;

        let allowed = CrossNousMessage::new("alice", "target", "hello");
        let state = router.send(allowed).await.unwrap();
        assert_eq!(state, DeliveryState::Delivered);

        let envelope = rx.recv().await.unwrap();
        assert_eq!(envelope.message.from, "alice");

        let rejected = CrossNousMessage::new("bob", "target", "hello");
        let err = router.send(rejected).await.unwrap_err();
        assert!(err.to_string().contains("address rejected"));
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
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
    async fn address_mask_ask_rejection_cleans_state_and_does_not_deliver() {
        let (router, mut rx) = setup_router().await;
        router
            .set_address_mask("target", AddressMask::OperatorOnly)
            .await;

        let msg = CrossNousMessage::new("sender", "target", "question")
            .with_reply(Duration::from_secs(1));
        let err = router.ask(msg).await.unwrap_err();
        assert!(err.to_string().contains("address rejected"));

        assert_eq!(router.ask_graph.read().await.edge_count(), 0);
        assert_eq!(router.pending_reply_count().await, 0);
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));

        let log = router.delivery_log.read().await;
        let entries = log.recent(1);
        assert!(matches!(
            entries[0].state,
            DeliveryState::Failed { ref reason } if reason.contains("address rejected")
        ));
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
        assert!(router.routes.try_read().is_ok());
    }

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

        // NOTE: Simulate a -> b already in flight by adding the edge directly.
        router.ask_graph.write().await.add_edge("a", "b").unwrap();

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

        {
            let mut graph = router.ask_graph.write().await;
            graph.add_edge("a", "b").unwrap();
            graph.add_edge("b", "c").unwrap();
        }

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

        let msg =
            CrossNousMessage::new("sender_c", "target", "hello").with_reply(Duration::from_secs(5));

        let ask_handle = tokio::spawn(
            {
                let r = router.clone();
                async move { r.ask(msg).await }
            }
            .instrument(tracing::info_span!("test_ask_multi_receiver")),
        );

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

        assert_eq!(router_check.ask_graph.read().await.edge_count(), 0);
    }

    #[tokio::test]
    async fn ask_graph_cleaned_up_on_cancellation() {
        let router = CrossNousRouter::default();
        let (tx, _rx) = mpsc::channel(1);
        // NOTE: Pre-filling the inbox makes `ask_inner` suspend at
        // `sender.send(envelope).await`.
        // INVARIANT: the edge is inserted before that point, so aborting the
        // task must not leave stale graph state.
        tx.try_send(CrossNousEnvelope {
            message: CrossNousMessage::new("filler", "target", "filler"),
        })
        .unwrap();
        router.register("target", tx).await;

        let msg = CrossNousMessage::new("sender", "target", "question")
            .with_reply(Duration::from_mins(1));

        let spawned_router = router.clone();
        let handle = tokio::spawn(async move { spawned_router.ask(msg).await });
        tokio::task::yield_now().await;

        handle.abort();
        let _ = handle.await;

        // NOTE: give any spawned async cleanup a chance to run.
        tokio::task::yield_now().await;

        assert_eq!(router.ask_graph.read().await.edge_count(), 0);
    }

    #[tokio::test]
    async fn pending_reply_cleaned_up_on_cancellation() {
        let (router, mut rx) = setup_router().await;
        let msg = CrossNousMessage::new("sender", "target", "question")
            .with_reply(Duration::from_mins(1));

        let spawned_router = router.clone();
        let handle = tokio::spawn(async move { spawned_router.ask(msg).await });

        // INVARIANT: receiving the envelope proves the pending-reply entry was
        // inserted before the ask future is aborted.
        let _envelope = rx.recv().await.unwrap();

        handle.abort();
        let _ = handle.await;

        // NOTE: give any spawned async cleanup a chance to run.
        tokio::task::yield_now().await;

        assert_eq!(router.pending_reply_count().await, 0);
        assert_eq!(router.ask_graph.read().await.edge_count(), 0);
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

        assert_eq!(router.ask_graph.read().await.edge_count(), 0);
    }

    #[tokio::test]
    async fn ask_cleanup_guard_removes_state_when_cancelled_while_waiting_for_reply() {
        let (router, mut rx) = setup_router().await;
        let msg = CrossNousMessage::new("sender", "target", "question")
            .with_reply(Duration::from_mins(1));

        let ask_handle = tokio::spawn({
            let r = router.clone();
            async move { r.ask(msg).await }
        });

        // Wait until the ask has been delivered and is suspended waiting for a reply.
        let envelope = rx.recv().await.unwrap();
        assert_eq!(envelope.message.content, "question");

        // Cancel the in-flight ask.
        ask_handle.abort();
        let result = ask_handle.await;
        assert!(result.is_err(), "expected cancelled task, got: {result:?}");

        // Yield so the guard's spawned cleanup task runs.
        tokio::task::yield_now().await;

        assert_eq!(router.ask_graph.read().await.edge_count(), 0);
        assert_eq!(router.pending_reply_count().await, 0);
    }

    #[tokio::test]
    async fn ask_cleanup_guard_removes_state_when_cancelled_before_delivery() {
        let (router, mut rx) = setup_router().await;
        let msg = CrossNousMessage::new("sender", "target", "question")
            .with_reply(Duration::from_mins(1));

        let ask_handle = tokio::spawn({
            let r = router.clone();
            async move { r.ask(msg).await }
        });

        // Cancel immediately, before the receiver has necessarily pulled the message.
        ask_handle.abort();
        let result = ask_handle.await;
        assert!(result.is_err(), "expected cancelled task, got: {result:?}");

        tokio::task::yield_now().await;

        // Drain any message that made it into the inbox before cancellation.
        while rx.try_recv().is_ok() {}

        assert_eq!(router.ask_graph.read().await.edge_count(), 0);
        assert_eq!(router.pending_reply_count().await, 0);
    }
}

#[cfg(test)]
mod knowledge_tests;
