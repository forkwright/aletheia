//! Cross-nous message router.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use koina::ulid::Ulid;
use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{instrument, warn};

use crate::error::{
    self, AddressRejectedSnafu, AskCycleDetectedSnafu, AskTimeoutSnafu, DeliveryFailedSnafu,
    NousNotFoundSnafu, ReplyNotFoundSnafu,
};

use super::{
    AddressMask, AskGraph, CrossNousEnvelope, CrossNousMessage, CrossNousReply,
    DEFAULT_MAX_LOG_ENTRIES, DEFAULT_REPLY_TIMEOUT, DeliveryEntry, DeliveryLog, DeliveryState,
};

pub(super) struct RouteEntry {
    sender: mpsc::Sender<CrossNousEnvelope>,
    address_mask: AddressMask,
}

/// RAII guard that removes the ask graph edge and pending reply entry when the
/// ask future is dropped without completing normally (e.g. task abort or
/// `select!` branch cancellation).
///
/// On normal completion the caller defuses the guard and performs synchronous
/// cleanup; the guard only fires on cancellation paths.
struct AskCleanupGuard {
    ask_graph: Arc<RwLock<AskGraph>>,
    pending_replies: Arc<RwLock<HashMap<Ulid, oneshot::Sender<CrossNousReply>>>>,
    from: Option<String>,
    to: Option<String>,
    msg_id: Ulid,
}

impl AskCleanupGuard {
    fn new(
        ask_graph: Arc<RwLock<AskGraph>>,
        pending_replies: Arc<RwLock<HashMap<Ulid, oneshot::Sender<CrossNousReply>>>>,
        from: String,
        to: String,
        msg_id: Ulid,
    ) -> Self {
        Self {
            ask_graph,
            pending_replies,
            from: Some(from),
            to: Some(to),
            msg_id,
        }
    }

    /// Disable cleanup so the guard does nothing when dropped.
    fn defuse(&mut self) {
        self.from = None;
        self.to = None;
    }
}

impl Drop for AskCleanupGuard {
    fn drop(&mut self) {
        let (Some(from), Some(to)) = (self.from.take(), self.to.take()) else {
            return;
        };
        let graph = Arc::clone(&self.ask_graph);
        let pending = Arc::clone(&self.pending_replies);
        let msg_id = self.msg_id;
        tokio::spawn(async move {
            graph.write().await.remove_edge(&from, &to);
            pending.write().await.remove(&msg_id);
        });
    }
}

/// Routes messages between nous actors using their IDs as keys.
pub struct CrossNousRouter {
    /// Maps nous id to its inbox sender and inbound address policy. Invariant:
    /// every spawned actor has exactly one entry; removed on unregister. Held
    /// briefly during send/register/unregister.
    pub(super) routes: Arc<RwLock<HashMap<String, RouteEntry>>>,
    /// Maps correlation id to the one-shot reply channel for an in-flight ask.
    /// Invariant: each ask inserts one entry; consumed exactly once on reply
    /// or removed on timeout, delivery failure, or cancellation.
    pending_replies: Arc<RwLock<HashMap<Ulid, oneshot::Sender<CrossNousReply>>>>,
    /// Append-only audit log of delivered messages. Invariant: entries are
    /// never modified after insertion; the log is read for diagnostics only.
    pub(super) delivery_log: Arc<RwLock<DeliveryLog>>,
    /// Directed graph of in-flight ask chains used for cycle detection.
    /// Invariant: an edge exists iff a pending ask is outstanding between
    /// the two nodes; removed when the reply arrives or the ask times out.
    pub(super) ask_graph: Arc<RwLock<AskGraph>>,
    /// Inbound address policy set before a route is registered.
    ///
    /// Missing masks use [`AddressMask::Public`]. Production manager
    /// registration passes an explicit mask derived from agent privacy; this
    /// fallback only applies to direct router registrations without policy.
    pending_address_masks: Arc<RwLock<HashMap<String, AddressMask>>>,
}

impl Clone for CrossNousRouter {
    fn clone(&self) -> Self {
        Self {
            routes: Arc::clone(&self.routes),
            pending_replies: Arc::clone(&self.pending_replies),
            delivery_log: Arc::clone(&self.delivery_log),
            ask_graph: Arc::clone(&self.ask_graph),
            pending_address_masks: Arc::clone(&self.pending_address_masks),
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
            pending_address_masks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a nous actor's inbox so it can receive cross-nous messages.
    ///
    /// Direct registrations use any pending mask set by
    /// [`Self::set_address_mask`] and otherwise default to
    /// [`AddressMask::Public`]. Manager-owned actors should call
    /// [`Self::register_with_address_mask`] so private-agent routes are never
    /// left to the public fallback.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Uses short `RwLock::write` sections with no partial
    /// message delivery.
    #[instrument(skip(self, sender))]
    pub async fn register(
        &self,
        nous_id: impl Into<String> + std::fmt::Debug,
        sender: mpsc::Sender<CrossNousEnvelope>,
    ) {
        let id = nous_id.into();
        let address_mask = self
            .pending_address_masks
            .write()
            .await
            .remove(&id)
            .unwrap_or_default();
        self.register_route(id, sender, address_mask).await;
    }

    /// Register a nous actor's inbox with an explicit inbound address policy.
    ///
    /// Use this for production actors whose route policy comes from effective
    /// agent configuration.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Uses short `RwLock::write` sections with no partial
    /// message delivery.
    #[instrument(skip(self, sender))]
    pub async fn register_with_address_mask(
        &self,
        nous_id: impl Into<String> + std::fmt::Debug,
        sender: mpsc::Sender<CrossNousEnvelope>,
        address_mask: AddressMask,
    ) {
        let id = nous_id.into();
        self.pending_address_masks.write().await.remove(&id);
        self.register_route(id, sender, address_mask).await;
    }

    async fn register_route(
        &self,
        id: String,
        sender: mpsc::Sender<CrossNousEnvelope>,
        address_mask: AddressMask,
    ) {
        self.routes.write().await.insert(
            id,
            RouteEntry {
                sender,
                address_mask,
            },
        );
    }

    /// Remove a nous actor's route, preventing further message delivery.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Uses a single `RwLock::write` + `remove`. No partial
    /// state on cancellation.
    #[instrument(skip(self))]
    pub async fn unregister(&self, nous_id: &str) {
        self.routes.write().await.remove(nous_id);
    }

    /// Set or update the inbound address policy for a target nous.
    ///
    /// If the route is not registered yet, the mask is stored and applied to a
    /// later direct [`Self::register`] call. Missing entries are treated as
    /// [`AddressMask::Public`].
    #[instrument(skip(self))]
    pub async fn set_address_mask(
        &self,
        nous_id: impl Into<String> + std::fmt::Debug,
        mask: AddressMask,
    ) {
        let id = nous_id.into();
        {
            let mut routes = self.routes.write().await;
            if let Some(entry) = routes.get_mut(&id) {
                entry.address_mask = mask;
                return;
            }
        }
        self.pending_address_masks.write().await.insert(id, mask);
    }

    /// Return the current effective address policy for a nous id.
    ///
    /// Registered routes return their live mask. Unregistered ids return a
    /// pending mask when one exists, otherwise [`AddressMask::Public`].
    pub async fn address_mask(&self, nous_id: &str) -> AddressMask {
        if let Some(mask) = self
            .routes
            .read()
            .await
            .get(nous_id)
            .map(|entry| entry.address_mask.clone())
        {
            return mask;
        }

        self.pending_address_masks
            .read()
            .await
            .get(nous_id)
            .cloned()
            .unwrap_or_default()
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
        let Some(route) = routes.get(&to) else {
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
        let sender = route.sender.clone();
        let address_mask = route.address_mask.clone();
        drop(routes);

        self.enforce_address_mask(&message, &address_mask).await?;

        let message_for_log = message.clone();
        let envelope = CrossNousEnvelope { message };

        match sender.send(envelope).await {
            Ok(()) => {
                self.log_delivery(&message_for_log, &DeliveryState::Delivered)
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
    ///
    /// Checks for cycles in the ask dependency graph before dispatching.
    /// If Actor A is already waiting on Actor B and B tries to ask A, the
    /// cycle is detected and an error is returned immediately.
    ///
    /// # Cancel safety
    ///
    /// Cancellation of the ask future (e.g. task abort or `select!`) cleans up
    /// the ask graph edge and pending reply entry. Note that the target may
    /// still process the message after the asker stops waiting.
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

        {
            let mut graph = self.ask_graph.write().await;
            if let Err(chain) = graph.add_edge(&from, &to) {
                let chain_str = chain.join(" -> ");
                warn!(chain = %chain_str, "ask cycle detected, returning error to prevent deadlock");
                return AskCycleDetectedSnafu { chain: chain_str }.fail();
            }
        }

        let (reply_tx, reply_rx) = oneshot::channel();
        let msg_id = message.id;
        self.pending_replies.write().await.insert(msg_id, reply_tx);

        // INVARIANT: from this point, the edge and pending reply entry must be
        // removed on every exit path. The guard handles cancellation; normal
        // paths clean up explicitly before defusing the guard.
        let mut guard = AskCleanupGuard::new(
            Arc::clone(&self.ask_graph),
            Arc::clone(&self.pending_replies),
            from.clone(),
            to.clone(),
            msg_id,
        );

        let result = self.ask_inner(&message, &to, timeout_dur, reply_rx).await;

        self.ask_graph.write().await.remove_edge(&from, &to);
        guard.defuse();

        result
    }

    /// Inner ask logic. The caller owns edge and pending-reply cleanup on
    /// normal completion; this function only removes the pending reply entry
    /// on the error paths it returns directly.
    async fn ask_inner(
        &self,
        message: &CrossNousMessage,
        to: &str,
        timeout_dur: Duration,
        reply_rx: oneshot::Receiver<CrossNousReply>,
    ) -> error::Result<CrossNousReply> {
        let routes = self.routes.read().await;
        let Some(route) = routes.get(to) else {
            drop(routes);
            self.pending_replies.write().await.remove(&message.id);
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
        let sender = route.sender.clone();
        let address_mask = route.address_mask.clone();
        drop(routes);

        if let Err(e) = self.enforce_address_mask(message, &address_mask).await {
            self.pending_replies.write().await.remove(&message.id);
            return Err(e);
        }

        let envelope = CrossNousEnvelope {
            message: message.clone(),
        };

        if sender.send(envelope).await.is_err() {
            self.pending_replies.write().await.remove(&message.id);
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
                    self.pending_replies.write().await.remove(&message.id);
                    self.log_delivery(message, &DeliveryState::Failed {
                        reason: "reply channel dropped".to_owned(),
                    }).await;
                    DeliveryFailedSnafu { nous_id: to.to_owned() }.fail()
                }
            }
            () = tokio::time::sleep(timeout_dur) => {
                self.pending_replies.write().await.remove(&message.id);
                self.log_delivery(message, &DeliveryState::TimedOut).await;
                AskTimeoutSnafu {
                    nous_id: to.to_owned(),
                    timeout_secs: timeout_dur.as_secs(),
                }.fail()
            }
        }
    }

    /// Submit a reply for a pending ask.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled after removing the pending reply entry
    /// but before sending the reply, the asker will timeout waiting for a
    /// reply that was consumed. Do not use in `select!` branches.
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
        if tx.send(reply).is_err() {
            warn!(message_id = %msg_id, "reply receiver dropped before reply delivery");
        }
        Ok(())
    }

    /// List all registered nous IDs.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Uses a single `RwLock::read`. No partial state on cancellation.
    pub async fn registered(&self) -> Vec<String> {
        self.routes.read().await.keys().cloned().collect()
    }

    #[cfg(test)]
    pub(super) async fn pending_reply_count(&self) -> usize {
        self.pending_replies.read().await.len()
    }

    async fn enforce_address_mask(
        &self,
        message: &CrossNousMessage,
        mask: &AddressMask,
    ) -> error::Result<()> {
        if mask.permits(&message.from) {
            return Ok(());
        }

        let state = DeliveryState::Failed {
            reason: format!(
                "address rejected by target '{}' for sender '{}'",
                message.to, message.from
            ),
        };
        self.log_delivery(message, &state).await;
        AddressRejectedSnafu {
            from: message.from.clone(),
            to: message.to.clone(),
        }
        .fail()
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
}
