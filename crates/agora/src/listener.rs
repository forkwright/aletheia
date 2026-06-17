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
    handles: Option<JoinSet<()>>,
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
        let count = i64::try_from(handles.len()).unwrap_or(0);
        crate::metrics::set_active_subscriptions(count);
        tracing::info!(
            subscriptions = count,
            max_concurrent_handlers,
            "channel listener started"
        );
        Self {
            rx: Some(rx),
            handles: Some(handles),
            max_concurrent_handlers,
        }
    }

    /// Decrement the active-subscription gauge when the listener is dropped
    /// while it still owns the receiver and background tasks.
    ///
    /// [`into_receiver`](Self::into_receiver) transfers ownership of the
    /// receiver and handles to the caller; in that case the subscriptions are
    /// still active and the gauge must not be cleared.
    fn decrement_on_drop(&mut self) {
        if self.rx.is_some() {
            crate::metrics::set_active_subscriptions(0);
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
        let mut handler_set = JoinSet::new();

        if let Some(ref mut rx) = self.rx {
            while let Some(msg) = rx.recv().await {
                // WHY: cap concurrent handler tasks to prevent unbounded growth
                // when messages arrive faster than handlers complete.
                while handler_set.len() >= self.max_concurrent_handlers {
                    // Each handler task records its own failure, so we only need
                    // to await a slot here.
                    let _ = handler_set.join_next().await;
                }

                let span = info_span!(
                    "inbound_message",
                    msg.channel = %msg.channel,
                    msg.source = %redact_phone(&msg.sender),
                );
                let h = Arc::clone(&handler);
                // WHY: run handler future directly in handler_set so JoinSet owns all
                // handler futures; when run() is cancelled JoinSet::drop aborts them
                // atomically — eliminates the orphaned-task risk of a nested
                // tokio::spawn whose JoinHandle is dropped (detaches) on cancellation.
                handler_set.spawn(h(msg).instrument(span));
            }
        }

        // WHY: wait for all in-flight handler tasks to complete before shutdown;
        // join_next returns Err(JoinError) when a handler panics — record the failure.
        while let Some(result) = handler_set.join_next().await {
            if let Err(e) = result {
                tracing::warn!(error = %e, "handler task panicked");
                crate::metrics::record_handler_failure("_unknown");
            }
        }

        // Drain provider/forwarding handles so provider failures are surfaced.
        #[expect(
            clippy::expect_used,
            reason = "run consumes self and handles are only taken here"
        )]
        let mut forwarding_handles = self.handles.take().expect("handles already consumed");
        while let Some(result) = forwarding_handles.join_next().await {
            if let Err(e) = result {
                tracing::warn!(error = %e, "listener forwarding task failed");
                crate::metrics::record_handler_failure("_forwarder");
            }
        }

        self.decrement_on_drop();
        tracing::info!("channel listener stopped");
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
        #[expect(
            clippy::expect_used,
            reason = "handles is None only if into_receiver was already called; calling it twice is a programming error and panic is appropriate"
        )]
        let handles = self
            .handles
            .take()
            .expect("into_receiver called on consumed listener");
        (rx, handles)
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
            let channel_id = provider.id().to_owned();
            let (mut provider_rx, mut provider_handles) =
                provider.listen(poll_interval, cancel.clone());
            let tx = merged_tx.clone();
            merged_handles.spawn(async move {
                while let Some(message) = provider_rx.recv().await {
                    if tx.send(message).await.is_err() {
                        break;
                    }
                }

                while let Some(result) = provider_handles.join_next().await {
                    if let Err(e) = result {
                        tracing::warn!(
                            error = %e,
                            channel_id = %channel_id,
                            "provider task failed"
                        );
                        crate::metrics::record_provider_failure(&channel_id);
                    }
                }
            });
        }

        drop(merged_tx);
        (merged_rx, merged_handles)
    }
}

impl Drop for ChannelListener {
    fn drop(&mut self) {
        self.decrement_on_drop();
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
        fn id(&self) -> &'static str {
            self.channel
        }

        fn name(&self) -> &'static str {
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
    #[expect(
        clippy::await_holding_lock,
        reason = "current_thread executor; no deadlock risk — GAUGE_TEST_LOCK is never acquired inside run()"
    )]
    async fn listener_drop_aborts_tasks() {
        let _guard = crate::metrics::GAUGE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let _guard = crate::metrics::GAUGE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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

    // ── Lifecycle/metrics tests ──

    struct PanicProvider;

    impl ChannelProvider for PanicProvider {
        fn id(&self) -> &'static str {
            "panic-provider"
        }

        fn name(&self) -> &'static str {
            "panic-provider"
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
            drop(tx);
            let mut handles = JoinSet::new();
            handles.spawn(async move { panic!("provider polling task failed") });
            (rx, handles)
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

    fn fresh_registry() -> koina::metrics::MetricsRegistry {
        let r = koina::metrics::MetricsRegistry::new();
        r.with_registry(crate::metrics::register);
        r
    }

    fn encode_metrics(r: &koina::metrics::MetricsRegistry) -> String {
        let mut buf = String::new();
        #[expect(clippy::unwrap_used, reason = "encoding into String is infallible")]
        r.encode(&mut buf).unwrap();
        buf
    }

    fn counter_value_for(encoded: &str, metric: &str, labels: &str) -> Option<u64> {
        let needle = format!("{metric}{{{labels}}} ");
        encoded.lines().find_map(|line| {
            line.strip_prefix(&needle)
                .and_then(|rest| rest.split_whitespace().next())
                .and_then(|v| v.parse::<u64>().ok())
        })
    }

    #[tokio::test]
    async fn drop_decrements_active_subscriptions() {
        let _guard = crate::metrics::GAUGE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_tx, rx) = mpsc::channel::<InboundMessage>(16);
        let handle = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_mins(5)).await;
        });
        let mut handles = JoinSet::new();
        handles.spawn(async move {
            if let Err(e) = handle.await {
                tracing::warn!(error = %e, "spawned task failed");
            }
        });

        let r = fresh_registry();
        {
            let _listener = ChannelListener::from_parts(rx, handles);
            let during = encode_metrics(&r);
            assert!(
                during.contains("aletheia_active_subscriptions 1"),
                "got: {during}"
            );
        }

        let after = encode_metrics(&r);
        assert!(
            after.contains("aletheia_active_subscriptions 0"),
            "got: {after}"
        );
    }

    #[tokio::test]
    #[expect(
        clippy::await_holding_lock,
        reason = "current_thread executor; no deadlock risk — GAUGE_TEST_LOCK is never acquired inside run()"
    )]
    async fn provider_task_failure_is_counted() {
        let _guard = crate::metrics::GAUGE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let provider = PanicProvider;
        let providers: [&dyn ChannelProvider; 1] = [&provider];
        let cancel = CancellationToken::new();
        let listener = ChannelListener::start_many(providers, None, &cancel);

        let r = fresh_registry();
        listener.run(|_msg| async {}).await;

        let out = encode_metrics(&r);
        let count = counter_value_for(
            &out,
            "aletheia_provider_failures_total",
            "channel_id=\"panic-provider\"",
        );
        assert_eq!(
            count,
            Some(1),
            "provider failure should be counted once; got: {out}"
        );
    }

    #[tokio::test]
    async fn handler_task_failure_is_counted() {
        let (tx, rx) = mpsc::channel(16);
        let listener = ChannelListener::from_parts(rx, JoinSet::new());

        tx.send(InboundMessage {
            channel: "signal".to_owned(),
            sender: "+1".to_owned(),
            sender_name: None,
            group_id: None,
            text: "boom".to_owned(),
            timestamp: 1,
            attachments: vec![],
            raw: None,
        })
        .await
        .expect("send");
        drop(tx);

        let r = fresh_registry();
        listener
            .run(|_msg| async move { panic!("handler task failed") })
            .await;

        let out = encode_metrics(&r);
        // NOTE: after removing the inner tokio::spawn, handler panics propagate
        // as JoinError from JoinSet::join_next in the drain loop; the channel_id is
        // no longer available at that point so failures are recorded as "_unknown".
        let count = counter_value_for(
            &out,
            "aletheia_handler_failures_total",
            "channel_id=\"_unknown\"",
        );
        assert_eq!(
            count,
            Some(1),
            "handler failure should be counted once; got: {out}"
        );
    }
}
