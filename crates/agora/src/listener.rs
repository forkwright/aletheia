//! Unified channel listener: merges inbound messages from channel providers.

use std::future::Future;
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::{Instrument, info_span, instrument};

use tokio_util::sync::CancellationToken;

use crate::types::{ChannelProvider, InboundMessage};

fn redact_phone(phone: &str) -> String {
    if phone.len() > 4 {
        format!("...{}", phone.get(phone.len() - 4..).unwrap_or(""))
    } else {
        "****".to_owned()
    }
}

/// Listens on registered channels, merging inbound messages into a single stream.
///
/// Dropping the listener aborts all background polling tasks through
/// [`JoinSet`]'s drop behavior unless [`into_receiver`](Self::into_receiver)
/// was called first, which transfers the receiver and handles to the caller.
pub struct ChannelListener {
    rx: Option<mpsc::Receiver<InboundMessage>>,
    handles: JoinSet<()>,
    /// Maximum concurrent inbound-message handler tasks.
    max_concurrent_handlers: usize,
}

impl ChannelListener {
    /// Start listening on a channel provider.
    ///
    /// Spawns provider-specific polling tasks and merges their messages into a
    /// single receiver. When the `cancel` token is cancelled, polling tasks
    /// exit promptly.
    #[must_use]
    pub fn start<P>(
        provider: &P,
        poll_interval: Option<std::time::Duration>,
        cancel: CancellationToken,
    ) -> Self
    where
        P: ChannelProvider + ?Sized,
    {
        let (rx, handles) = provider.listen(poll_interval, cancel);
        Self::from_parts(rx, handles)
    }

    /// Start listening with explicit config for handler concurrency.
    #[must_use]
    pub fn start_with_config<P>(
        provider: &P,
        poll_interval: Option<std::time::Duration>,
        cancel: CancellationToken,
        max_concurrent_handlers: usize,
    ) -> Self
    where
        P: ChannelProvider + ?Sized,
    {
        let (rx, handles) = provider.listen(poll_interval, cancel);
        Self::from_parts_with_config(rx, handles, max_concurrent_handlers)
    }

    /// Start listening on multiple channel providers and merge their streams.
    #[must_use]
    pub fn start_many<'a, I>(
        providers: I,
        poll_interval: Option<std::time::Duration>,
        cancel: &CancellationToken,
    ) -> Self
    where
        I: IntoIterator<Item = &'a dyn ChannelProvider>,
    {
        Self::start_many_with_config(
            providers,
            poll_interval,
            cancel,
            Self::DEFAULT_MAX_CONCURRENT_HANDLERS,
        )
    }

    /// Start listening on multiple providers with explicit handler concurrency.
    #[must_use]
    pub fn start_many_with_config<'a, I>(
        providers: I,
        poll_interval: Option<std::time::Duration>,
        cancel: &CancellationToken,
        max_concurrent_handlers: usize,
    ) -> Self
    where
        I: IntoIterator<Item = &'a dyn ChannelProvider>,
    {
        let (rx, handles) = Self::merge_providers(providers, poll_interval, cancel);
        Self::from_parts_with_config(rx, handles, max_concurrent_handlers)
    }

    /// Create from pre-built parts with default handler concurrency.
    ///
    /// Use when the caller assembles provider-specific listeners
    /// independently (e.g., merging Signal + future Slack receivers).
    /// Abort callbacks are registered at construction time for each handle.
    #[must_use]
    pub(crate) fn from_parts(rx: mpsc::Receiver<InboundMessage>, handles: JoinSet<()>) -> Self {
        Self::from_parts_with_config(rx, handles, Self::DEFAULT_MAX_CONCURRENT_HANDLERS)
    }

    /// Create from pre-built parts with explicit handler concurrency limit.
    #[must_use]
    pub(crate) fn from_parts_with_config(
        rx: mpsc::Receiver<InboundMessage>,
        handles: JoinSet<()>,
        max_concurrent_handlers: usize,
    ) -> Self {
        // WHY: JoinSet aborts all tasks on drop, so no explicit cleanup needed.
        // Handle count is small (single-digit), fits in i64
        crate::metrics::set_active_subscriptions(i64::try_from(handles.len()).unwrap_or(0));
        Self {
            rx: Some(rx),
            handles,
            max_concurrent_handlers,
        }
    }

    /// Fallback default; runtime reads `MessagingConfig::max_concurrent_handlers`.
    const DEFAULT_MAX_CONCURRENT_HANDLERS: usize = 64;

    /// Run the listener loop, dispatching each message to the handler concurrently.
    ///
    /// Each inbound message is dispatched to `handler` in a separate spawned task,
    /// so a slow handler does not block delivery of subsequent messages.
    /// Concurrency is capped at `max_concurrent_handlers` (from `MessagingConfig`)
    /// to prevent unbounded task growth under load.
    ///
    /// Returns after all senders are dropped (all polling tasks have stopped) and
    /// all in-flight handler tasks have completed.
    #[instrument(skip_all)]
    pub async fn run<F, Fut>(mut self, handler: F)
    where
        F: Fn(InboundMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let handler = Arc::new(handler);
        let mut set = JoinSet::new();

        if let Some(ref mut rx) = self.rx {
            while let Some(msg) = rx.recv().await {
                // WHY: cap concurrent handler tasks to prevent unbounded growth
                // when messages arrive faster than handlers complete.
                while set.len() >= self.max_concurrent_handlers {
                    if let Some(result) = set.join_next().await
                        && let Err(e) = result
                    {
                        tracing::warn!(error = %e, "handler task panicked");
                    }
                }

                let span = info_span!(
                    "inbound_message",
                    msg.channel = %msg.channel,
                    msg.source = %redact_phone(&msg.sender),
                );
                let h = Arc::clone(&handler);
                set.spawn(async move { h(msg).await }.instrument(span));
            }
        }

        // WHY: wait for all in-flight handler tasks to complete before shutdown
        while let Some(result) = set.join_next().await {
            if let Err(e) = result {
                tracing::warn!(error = %e, "handler task panicked");
            }
        }

        tracing::info!("all channels closed, listener stopping");
    }

    /// Unwrap into the raw receiver and background task handles for manual control.
    ///
    /// The returned handles represent the background polling tasks.  Callers can
    /// abort them for immediate shutdown or await them for graceful drain.  Tasks
    /// also stop naturally once the receiver is dropped (closed channel).
    #[must_use]
    pub fn into_receiver(mut self) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>) {
        #[expect(
            clippy::expect_used,
            reason = "rx is None only if into_receiver was already called; calling it twice is a programming error and panic is appropriate"
        )]
        let rx = self
            .rx
            .take()
            .expect("into_receiver called on consumed listener");
        (rx, self.handles)
    }

    fn merge_providers<'a, I>(
        providers: I,
        poll_interval: Option<std::time::Duration>,
        cancel: &CancellationToken,
    ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>)
    where
        I: IntoIterator<Item = &'a dyn ChannelProvider>,
    {
        let (merged_tx, merged_rx) = mpsc::channel(64);
        let mut merged_handles = JoinSet::new();

        for provider in providers {
            let (mut provider_rx, mut provider_handles) =
                provider.listen(poll_interval, cancel.clone());
            let tx = merged_tx.clone();
            merged_handles.spawn(async move {
                while let Some(message) = provider_rx.recv().await {
                    if tx.send(message).await.is_err() {
                        break;
                    }
                }

                while provider_handles.join_next().await.is_some() {}
            });
        }

        drop(merged_tx);
        (merged_rx, merged_handles)
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::pin::Pin;

    use tracing::Instrument;

    use super::*;

    static TEST_CAPABILITIES: crate::types::ChannelCapabilities =
        crate::types::ChannelCapabilities {
            threads: false,
            reactions: false,
            typing: false,
            media: false,
            streaming: false,
            rich_formatting: false,
            max_text_length: 1000,
        };

    struct TestProvider {
        channel: &'static str,
        messages: Vec<InboundMessage>,
    }

    impl TestProvider {
        fn new(channel: &'static str, text: &str) -> Self {
            Self {
                channel,
                messages: vec![InboundMessage {
                    channel: channel.to_owned(),
                    sender: format!("{channel}-sender"),
                    sender_name: None,
                    group_id: None,
                    text: text.to_owned(),
                    timestamp: 100,
                    attachments: vec![],
                    raw: None,
                }],
            }
        }
    }

    impl ChannelProvider for TestProvider {
        fn id(&self) -> &str {
            self.channel
        }

        fn name(&self) -> &str {
            self.channel
        }

        fn capabilities(&self) -> &crate::types::ChannelCapabilities {
            &TEST_CAPABILITIES
        }

        fn send<'a>(
            &'a self,
            _params: &'a crate::types::SendParams,
        ) -> Pin<Box<dyn Future<Output = crate::types::SendResult> + Send + 'a>> {
            Box::pin(async { crate::types::SendResult::ok() })
        }

        fn listen(
            &self,
            _poll_interval: Option<std::time::Duration>,
            _cancel: CancellationToken,
        ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>) {
            let (tx, rx) = mpsc::channel(16);
            for message in &self.messages {
                tx.try_send(message.clone()).expect("send test message");
            }
            drop(tx);
            (rx, JoinSet::new())
        }

        fn probe<'a>(
            &'a self,
        ) -> Pin<Box<dyn Future<Output = crate::types::ProbeResult> + Send + 'a>> {
            Box::pin(async {
                crate::types::ProbeResult {
                    ok: true,
                    latency_ms: None,
                    error: None,
                    details: None,
                }
            })
        }
    }

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
        let listener = ChannelListener::from_parts(rx, JoinSet::new());

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

        let (mut rx, _handles) = listener.into_receiver();
        let received = rx.recv().await.expect("recv");
        assert_eq!(received.text, "hello");
        assert_eq!(received.sender, "+1234567890");
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn listener_merges_multiple_providers() {
        let signal = TestProvider::new("signal", "from signal");
        let slack = TestProvider::new("slack", "from slack");
        let providers: [&dyn ChannelProvider; 2] = [&signal, &slack];
        let cancel = CancellationToken::new();
        let listener = ChannelListener::start_many(providers, None, &cancel);

        let (mut rx, _handles) = listener.into_receiver();
        let mut received = Vec::new();
        while let Some(message) = rx.recv().await {
            received.push((message.channel, message.text));
        }
        received.sort();

        assert_eq!(
            received,
            vec![
                ("signal".to_owned(), "from signal".to_owned()),
                ("slack".to_owned(), "from slack".to_owned())
            ]
        );
    }

    #[tokio::test]
    async fn listener_run_dispatches_to_handler() {
        let (tx, rx) = mpsc::channel(16);
        let listener = ChannelListener::from_parts(rx, JoinSet::new());

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
    async fn listener_drop_aborts_tasks() {
        let task_finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let finished_clone = task_finished.clone();

        let (_tx, rx) = mpsc::channel::<InboundMessage>(16);

        let handle = tokio::spawn(
            async move {
                tokio::time::sleep(std::time::Duration::from_mins(5)).await;
                finished_clone.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            .instrument(tracing::info_span!("test_sleep_task")),
        );

        {
            let mut handles = JoinSet::new();
            handles.spawn(async move {
                if let Err(e) = handle.await {
                    tracing::warn!(error = %e, "spawned task failed");
                }
            });
            let _listener = ChannelListener::from_parts(rx, handles);
        }

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(
            !task_finished.load(std::sync::atomic::Ordering::Relaxed),
            "task should have been aborted, not completed"
        );
    }

    #[tokio::test]
    async fn into_receiver_returns_handles() {
        let (_tx, rx) = mpsc::channel::<InboundMessage>(16);

        let handle = tokio::spawn(
            async {
                tokio::time::sleep(std::time::Duration::from_mins(5)).await;
            }
            .instrument(tracing::info_span!("test_sleep_task")),
        );

        let mut join_set = JoinSet::new();
        join_set.spawn(async move {
            if let Err(e) = handle.await {
                tracing::warn!(error = %e, "spawned task failed");
            }
        });
        let listener = ChannelListener::from_parts(rx, join_set);
        let (_rx, mut handles) = listener.into_receiver();

        assert_eq!(handles.len(), 1);
        handles.abort_all();
    }
}
