//! Cloneable actor handle for sending messages to a [`NousActor`](crate::actor::NousActor).

use std::time::Duration;

use tokio::sync::{mpsc, oneshot};

use crate::error::{self, ActorRecvSnafu, ActorSendSnafu, InboxFullSnafu};
use crate::message::{NousMessage, NousStatus};
use crate::pipeline::TurnResult;
use crate::stream::TurnStreamEvent;

/// Default timeout for sending messages to an actor's inbox.
pub const DEFAULT_SEND_TIMEOUT: Duration = Duration::from_secs(30);

/// Cloneable handle for communicating with a nous actor.
///
/// All external interaction with a [`NousActor`](crate::actor::NousActor) goes through its handle.
/// The handle is `Clone + Send + Sync`: safe to share across tasks.
#[derive(Clone)]
pub struct NousHandle {
    id: String,
    sender: mpsc::Sender<NousMessage>,
}

impl NousHandle {
    /// Create a new handle from an actor id and sender.
    pub(crate) fn new(id: String, sender: mpsc::Sender<NousMessage>) -> Self {
        Self { id, sender }
    }

    /// Agent identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Send a turn message and await the result.
    ///
    /// Uses [`DEFAULT_SEND_TIMEOUT`] for the inbox send. If the inbox is full
    /// for longer than the timeout, returns [`InboxFull`](error::Error::InboxFull).
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. If cancelled after `mpsc::send` completes but before
    /// `oneshot::recv` returns, the message is consumed by the actor but the
    /// reply is lost. Callers should not use this in `select!` branches.
    pub async fn send_turn(
        &self,
        session_key: impl Into<String>,
        content: impl Into<String>,
    ) -> error::Result<TurnResult> {
        self.send_turn_with_session_id(session_key, None, content, DEFAULT_SEND_TIMEOUT)
            .await
    }

    /// Send a turn with an explicit database session ID.
    ///
    /// When `session_id` is `Some`, the actor adopts this ID for its in-memory
    /// `SessionState` instead of generating a new one. This prevents divergence
    /// between the HTTP-layer session ID and the actor's internal ID.
    pub async fn send_turn_with_session_id(
        &self,
        session_key: impl Into<String>,
        session_id: Option<String>,
        content: impl Into<String>,
        timeout: Duration,
    ) -> error::Result<TurnResult> {
        let (tx, rx) = oneshot::channel();
        let msg = NousMessage::Turn {
            session_key: session_key.into(),
            session_id,
            content: content.into(),
            span: tracing::Span::current(),
            reply: tx,
        };
        self.send_with_timeout(msg, timeout).await?;
        rx.await.map_err(|_send_err| {
            ActorRecvSnafu {
                message: format!("actor '{}' dropped reply", self.id),
            }
            .build()
        })?
    }

    /// Send a turn message with a configurable inbox timeout.
    pub async fn send_turn_with_timeout(
        &self,
        session_key: impl Into<String>,
        content: impl Into<String>,
        timeout: Duration,
    ) -> error::Result<TurnResult> {
        self.send_turn_with_session_id(session_key, None, content, timeout)
            .await
    }

    /// Send a turn message with real-time streaming and await the result.
    ///
    /// Events are sent to `stream_tx` as the LLM generates content and tools execute.
    /// The final `TurnResult` is returned when the turn completes.
    ///
    /// Uses [`DEFAULT_SEND_TIMEOUT`] for the inbox send.
    ///
    /// # Cancel safety
    ///
    /// Not cancel-safe. Same as [`send_turn`](Self::send_turn): if cancelled
    /// between the inbox send and reply receipt, the turn runs but the result
    /// is discarded.
    pub async fn send_turn_streaming(
        &self,
        session_key: impl Into<String>,
        content: impl Into<String>,
        stream_tx: mpsc::Sender<TurnStreamEvent>,
    ) -> error::Result<TurnResult> {
        self.send_turn_streaming_with_session_id(
            session_key,
            None,
            content,
            stream_tx,
            DEFAULT_SEND_TIMEOUT,
        )
        .await
    }

    /// Send a streaming turn with an explicit database session ID.
    pub async fn send_turn_streaming_with_session_id(
        &self,
        session_key: impl Into<String>,
        session_id: Option<String>,
        content: impl Into<String>,
        stream_tx: mpsc::Sender<TurnStreamEvent>,
        timeout: Duration,
    ) -> error::Result<TurnResult> {
        let (tx, rx) = oneshot::channel();
        let msg = NousMessage::StreamingTurn {
            session_key: session_key.into(),
            session_id,
            content: content.into(),
            stream_tx,
            span: tracing::Span::current(),
            reply: tx,
        };
        self.send_with_timeout(msg, timeout).await?;
        rx.await.map_err(|_send_err| {
            ActorRecvSnafu {
                message: format!("actor '{}' dropped reply", self.id),
            }
            .build()
        })?
    }

    /// Send a streaming turn with a configurable inbox timeout.
    pub async fn send_turn_streaming_with_timeout(
        &self,
        session_key: impl Into<String>,
        content: impl Into<String>,
        stream_tx: mpsc::Sender<TurnStreamEvent>,
        timeout: Duration,
    ) -> error::Result<TurnResult> {
        self.send_turn_streaming_with_session_id(session_key, None, content, stream_tx, timeout)
            .await
    }

    /// Send a ping to the actor and wait for a reply.
    ///
    /// Returns `Ok(())` if the actor responds within `timeout`, or an error
    /// if the actor is unresponsive.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Both sides are cancel-safe and a lost ping has no side effects.
    pub async fn ping(&self, timeout: Duration) -> error::Result<()> {
        let (tx, rx) = oneshot::channel();
        self.send_with_timeout(NousMessage::Ping { reply: tx }, timeout)
            .await?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(ActorRecvSnafu {
                message: format!("actor '{}' dropped ping reply", self.id),
            }
            .build()),
            Err(_) => Err(ActorRecvSnafu {
                message: format!("actor '{}' ping timed out", self.id),
            }
            .build()),
        }
    }

    /// Send a message to the actor's inbox with a timeout.
    async fn send_with_timeout(&self, msg: NousMessage, timeout: Duration) -> error::Result<()> {
        match tokio::time::timeout(timeout, self.sender.send(msg)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(ActorSendSnafu {
                message: format!("actor '{}' inbox closed", self.id),
            }
            .build()),
            Err(_) => Err(InboxFullSnafu {
                nous_id: self.id.clone(),
                timeout_secs: timeout.as_secs(),
            }
            .build()),
        }
    }

    /// Query the actor's current status.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Both `mpsc::send` and `oneshot::recv` are cancel-safe.
    /// A lost status query has no side effects.
    pub async fn status(&self) -> error::Result<NousStatus> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(NousMessage::Status { reply: tx })
            .await
            .map_err(|_send_err| {
                ActorSendSnafu {
                    message: format!("actor '{}' inbox closed", self.id),
                }
                .build()
            })?;
        rx.await.map_err(|_send_err| {
            ActorRecvSnafu {
                message: format!("actor '{}' dropped reply", self.id),
            }
            .build()
        })
    }

    /// Transition the actor to dormant state.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Uses a single `mpsc::send` with no follow-up await.
    pub async fn sleep(&self) -> error::Result<()> {
        self.sender
            .send(NousMessage::Sleep)
            .await
            .map_err(|_send_err| {
                ActorSendSnafu {
                    message: format!("actor '{}' inbox closed", self.id),
                }
                .build()
            })
    }

    /// Wake the actor from dormant state.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Uses a single `mpsc::send` with no follow-up await.
    pub async fn wake(&self) -> error::Result<()> {
        self.sender
            .send(NousMessage::Wake)
            .await
            .map_err(|_send_err| {
                ActorSendSnafu {
                    message: format!("actor '{}' inbox closed", self.id),
                }
                .build()
            })
    }

    /// Request graceful shutdown.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe. Uses a single `mpsc::send` with no follow-up await.
    pub async fn shutdown(&self) -> error::Result<()> {
        self.sender
            .send(NousMessage::Shutdown)
            .await
            .map_err(|_send_err| {
                ActorSendSnafu {
                    message: format!("actor '{}' inbox closed", self.id),
                }
                .build()
            })
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
#[expect(clippy::expect_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;
    use tracing::Instrument;

    #[test]
    fn handle_id_returns_correct_value() {
        let (tx, _rx) = mpsc::channel(1);
        let handle = NousHandle::new("syn".to_owned(), tx);
        assert_eq!(handle.id(), "syn");
    }

    #[test]
    fn handle_clone_preserves_id() {
        let (tx, _rx) = mpsc::channel(1);
        let handle = NousHandle::new("syn".to_owned(), tx);
        let cloned = handle.clone();
        assert_eq!(cloned.id(), "syn");
    }

    #[tokio::test]
    async fn send_turn_to_closed_channel_returns_error() {
        let (tx, rx) = mpsc::channel(1);
        let handle = NousHandle::new("syn".to_owned(), tx);
        drop(rx);

        let err = handle.send_turn("main", "Hello").await;
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("inbox closed"), "got: {msg}");
    }

    #[tokio::test]
    async fn status_to_closed_channel_returns_error() {
        let (tx, rx) = mpsc::channel(1);
        let handle = NousHandle::new("syn".to_owned(), tx);
        drop(rx);

        let err = handle.status().await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn sleep_to_closed_channel_returns_error() {
        let (tx, rx) = mpsc::channel(1);
        let handle = NousHandle::new("syn".to_owned(), tx);
        drop(rx);

        let err = handle.sleep().await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn wake_to_closed_channel_returns_error() {
        let (tx, rx) = mpsc::channel(1);
        let handle = NousHandle::new("syn".to_owned(), tx);
        drop(rx);

        let err = handle.wake().await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn shutdown_to_closed_channel_returns_error() {
        let (tx, rx) = mpsc::channel(1);
        let handle = NousHandle::new("syn".to_owned(), tx);
        drop(rx);

        let err = handle.shutdown().await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn send_turn_dropped_reply_returns_error() {
        let (tx, mut rx) = mpsc::channel(1);
        let handle = NousHandle::new("syn".to_owned(), tx);

        // Spawn a task that receives the message but drops the reply channel
        tokio::spawn(
            async move {
                if let Some(NousMessage::Turn { reply, .. }) = rx.recv().await {
                    drop(reply);
                }
            }
            .instrument(tracing::info_span!("test_drop_reply")),
        );

        let err = handle.send_turn("main", "Hello").await;
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("dropped reply"), "got: {msg}");
    }

    /// Verify the actor pattern: when the oneshot reply sender fires into a
    /// dropped receiver, it does not panic: the `let _ = reply.send(result)`
    /// pattern silently discards the error.
    #[tokio::test]
    async fn actor_continues_after_reply_channel_dropped() {
        let (tx, mut rx) = mpsc::channel::<NousMessage>(4);
        let handle = NousHandle::new("syn".to_owned(), tx);

        // Simulate actor loop: receive messages and reply
        let actor = tokio::spawn(
            async move {
                let mut received = 0u32;
                while let Some(msg) = rx.recv().await {
                    match msg {
                        NousMessage::Turn { reply, .. }
                        | NousMessage::StreamingTurn { reply, .. } => {
                            // Actor sends result: receiver may already be dropped.
                            // This must not panic.
                            let _ = reply.send(Err(crate::error::PipelineStageSnafu {
                                stage: "test",
                                message: "simulated",
                            }
                            .build()));
                            received += 1;
                        }
                        NousMessage::Shutdown => break,
                        _ => {}
                    }
                }
                received
            }
            .instrument(tracing::info_span!("test_actor_loop")),
        );

        // Send a turn, then drop the handle's receiver (simulating client disconnect)
        let send_result = {
            let (reply_tx, _reply_rx) = tokio::sync::oneshot::channel();
            handle
                .sender
                .send(NousMessage::Turn {
                    session_key: "main".to_owned(),
                    session_id: None,
                    content: "hello".to_owned(),
                    span: tracing::Span::current(),
                    reply: reply_tx,
                })
                .await
        };
        assert!(send_result.is_ok());

        // Actor should still be alive: send shutdown and verify it processed the turn
        let _ = handle.shutdown().await;
        let received = actor.await.expect("actor should not panic");
        assert_eq!(received, 1, "actor should have processed the turn");
    }

    #[test]
    fn handle_send_sync() {
        static_assertions::assert_impl_all!(NousHandle: Send, Sync, Clone);
    }
}
