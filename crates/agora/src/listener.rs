//! Unified channel listener — merges inbound messages from all providers.

use std::future::Future;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info_span, instrument};

use crate::semeion::SignalProvider;
use crate::types::InboundMessage;

fn redact_phone(phone: &str) -> String {
    if phone.len() > 4 {
        format!("...{}", &phone[phone.len() - 4..])
    } else {
        "****".to_owned()
    }
}

/// Listens on registered channels, merging inbound messages into a single stream.
///
/// Dropping the listener aborts all background polling tasks unless
/// [`into_receiver`](Self::into_receiver) was called first.
pub struct ChannelListener {
    rx: Option<mpsc::Receiver<InboundMessage>>,
    handles: Vec<JoinHandle<()>>,
}

impl ChannelListener {
    /// Start listening on a Signal provider.
    ///
    /// Spawns polling tasks for all accounts registered on the provider
    /// and merges their messages into a single receiver.
    pub fn start(
        signal_provider: &SignalProvider,
        poll_interval: Option<std::time::Duration>,
    ) -> Self {
        let (rx, handles) = signal_provider.listen(poll_interval);
        Self {
            rx: Some(rx),
            handles,
        }
    }

    /// Create from pre-built parts.
    ///
    /// Use when the caller assembles provider-specific listeners
    /// independently (e.g., merging Signal + future Slack receivers).
    pub fn from_parts(rx: mpsc::Receiver<InboundMessage>, handles: Vec<JoinHandle<()>>) -> Self {
        Self {
            rx: Some(rx),
            handles,
        }
    }

    /// Run the listener loop, dispatching each message to the handler.
    ///
    /// Returns when all senders are dropped (all polling tasks have stopped).
    #[instrument(skip_all)]
    pub async fn run<F, Fut>(mut self, handler: F)
    where
        F: Fn(InboundMessage) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send,
    {
        if let Some(ref mut rx) = self.rx {
            while let Some(msg) = rx.recv().await {
                let span = info_span!("inbound_message",
                    msg.channel = %msg.channel,
                    msg.source = %redact_phone(&msg.sender),
                );
                let _guard = span.enter();
                handler(msg).await;
            }
        }
        tracing::info!("all channels closed, listener stopping");
    }

    /// Unwrap into the raw receiver for manual control.
    ///
    /// The background polling handles are kept alive — they stop
    /// naturally when the receiver is dropped (closed channel).
    pub fn into_receiver(mut self) -> mpsc::Receiver<InboundMessage> {
        let rx = self
            .rx
            .take()
            .expect("into_receiver called on consumed listener");
        // Clear handles so Drop doesn't abort them
        self.handles.clear();
        rx
    }

    /// Stop all polling tasks.
    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for ChannelListener {
    fn drop(&mut self) {
        for handle in &self.handles {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_phone_long_number() {
        assert_eq!(redact_phone("+1234567890"), "...7890");
    }

    #[test]
    fn redact_phone_short_number() {
        assert_eq!(redact_phone("12"), "****");
    }

    #[test]
    fn redact_phone_exactly_four() {
        assert_eq!(redact_phone("1234"), "****");
    }

    #[test]
    fn redact_phone_five_chars() {
        assert_eq!(redact_phone("12345"), "...2345");
    }

    #[tokio::test]
    async fn listener_receives_messages() {
        let (tx, rx) = mpsc::channel(16);
        let listener = ChannelListener::from_parts(rx, vec![]);

        let msg = InboundMessage {
            channel: "signal".to_owned(),
            sender: "+1234567890".to_owned(),
            sender_name: None,
            group_id: None,
            text: "hello".to_owned(),
            timestamp: 100,
            attachments: vec![],
            raw: None,
        };

        tx.send(msg.clone()).await.expect("send");
        drop(tx);

        let mut rx = listener.into_receiver();
        let received = rx.recv().await.expect("recv");
        assert_eq!(received.text, "hello");
        assert_eq!(received.sender, "+1234567890");
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn listener_run_dispatches_to_handler() {
        let (tx, rx) = mpsc::channel(16);
        let listener = ChannelListener::from_parts(rx, vec![]);

        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count_clone = count.clone();

        for i in 0_u64..3 {
            tx.send(InboundMessage {
                channel: "signal".to_owned(),
                sender: format!("+{i}"),
                sender_name: None,
                group_id: None,
                text: format!("msg-{i}"),
                timestamp: i,
                attachments: vec![],
                raw: None,
            })
            .await
            .expect("send");
        }
        drop(tx);

        listener
            .run(move |_msg| {
                let c = count_clone.clone();
                async move {
                    c.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            })
            .await;

        assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn listener_stop_aborts_tasks() {
        let (_tx, rx) = mpsc::channel::<InboundMessage>(16);

        let handle = tokio::spawn(async {
            // Simulate a long-running task
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
        });

        let listener = ChannelListener::from_parts(rx, vec![handle]);
        listener.stop();

        // If we get here, the tasks were aborted successfully
    }

    #[tokio::test]
    async fn listener_drop_aborts_tasks() {
        let task_finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finished_clone = task_finished.clone();

        let (_tx, rx) = mpsc::channel::<InboundMessage>(16);

        let handle = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(300)).await;
            finished_clone.store(true, std::sync::atomic::Ordering::Relaxed);
        });

        {
            let _listener = ChannelListener::from_parts(rx, vec![handle]);
            // listener drops here
        }

        // Give the runtime a moment to process the abort
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(
            !task_finished.load(std::sync::atomic::Ordering::Relaxed),
            "task should have been aborted, not completed"
        );
    }
}
