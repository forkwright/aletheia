//! Cross-nous message router.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{RwLock, mpsc, oneshot};
use tracing::{instrument, warn};
use ulid::Ulid;

use crate::error::{
    self, AskCycleDetectedSnafu, AskTimeoutSnafu, DeliveryFailedSnafu, NousNotFoundSnafu,
    ReplyNotFoundSnafu,
};

use super::{
    AskGraph, CrossNousEnvelope, CrossNousMessage, CrossNousReply, DEFAULT_MAX_LOG_ENTRIES,
    DEFAULT_REPLY_TIMEOUT, DeliveryEntry, DeliveryLog, DeliveryState,
};

pub struct CrossNousRouter {
    /// Maps nous id to its inbox sender. Invariant: every spawned actor has
    /// exactly one entry; removed on unregister. Held briefly during
    /// send/register/unregister.
    pub(super) routes: Arc<RwLock<HashMap<String, mpsc::Sender<CrossNousEnvelope>>>>,
    /// Maps correlation id to the one-shot reply channel for an in-flight ask.
    /// Invariant: each ask inserts one entry; consumed exactly once on reply
    /// or removed on timeout.
    pending_replies: Arc<RwLock<HashMap<Ulid, oneshot::Sender<CrossNousReply>>>>,
    /// Append-only audit log of delivered messages. Invariant: entries are
    /// never modified after insertion; the log is read for diagnostics only.
    pub(super) delivery_log: Arc<RwLock<DeliveryLog>>,
    /// Directed graph of in-flight ask chains used for cycle detection.
    /// Invariant: an edge exists iff a pending ask is outstanding between
    /// the two nodes; removed when the reply arrives or the ask times out.
    pub(super) ask_graph: Arc<RwLock<AskGraph>>,
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
