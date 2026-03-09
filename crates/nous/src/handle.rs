//! Cloneable actor handle for sending messages to a [`NousActor`](crate::actor::NousActor).

use tokio::sync::{mpsc, oneshot};

use crate::error::{self, ActorRecvSnafu, ActorSendSnafu};
use crate::message::{NousMessage, NousStatus};
use crate::pipeline::TurnResult;
use crate::stream::TurnStreamEvent;

/// Cloneable handle for communicating with a nous actor.
///
/// All external interaction with a [`NousActor`](crate::actor::NousActor) goes through its handle.
/// The handle is `Clone + Send + Sync` — safe to share across tasks.
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
    pub async fn send_turn(
        &self,
        session_key: impl Into<String>,
        content: impl Into<String>,
    ) -> error::Result<TurnResult> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(NousMessage::Turn {
                session_key: session_key.into(),
                content: content.into(),
                reply: tx,
            })
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
        })?
    }

    /// Send a turn message with real-time streaming and await the result.
    ///
    /// Events are sent to `stream_tx` as the LLM generates content and tools execute.
    /// The final `TurnResult` is returned when the turn completes.
    pub async fn send_turn_streaming(
        &self,
        session_key: impl Into<String>,
        content: impl Into<String>,
        stream_tx: mpsc::Sender<TurnStreamEvent>,
    ) -> error::Result<TurnResult> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(NousMessage::StreamingTurn {
                session_key: session_key.into(),
                content: content.into(),
                stream_tx,
                reply: tx,
            })
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
        })?
    }

    /// Query the actor's current status.
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
mod tests {
    use super::*;

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
        tokio::spawn(async move {
            if let Some(NousMessage::Turn { reply, .. }) = rx.recv().await {
                drop(reply);
            }
        });

        let err = handle.send_turn("main", "Hello").await;
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("dropped reply"), "got: {msg}");
    }

    #[test]
    fn handle_send_sync() {
        static_assertions::assert_impl_all!(NousHandle: Send, Sync, Clone);
    }
}
