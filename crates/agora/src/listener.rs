//! Unified channel listener: merges inbound messages from channel providers.

use std::future::Future;
use std::sync::Arc;

use futures::FutureExt;

use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tracing::{Instrument, info_span, instrument};

use tokio_util::sync::CancellationToken;

use crate::types::{ChannelProvider, InboundMessage};

async fn invoke_handler<F, Fut>(
    handler: Arc<F>,
    message: InboundMessage,
    channel_id: String,
    _in_flight: crate::metrics::InboundHandlerGuard,
) where
    F: Fn(InboundMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    // Calling the handler constructs its future and can itself panic. Catch
    // that synchronous boundary separately so it retains channel attribution.
    let future = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| handler(message))) {
        Ok(future) => future,
        Err(_panic) => {
            tracing::warn!(
                channel_id = %channel_id,
                phase = "construction",
                "handler task panicked"
            );
            crate::metrics::record_handler_failure(&channel_id);
            return;
        }
    };

    if std::panic::AssertUnwindSafe(future)
        .catch_unwind()
        .await
        .is_err()
    {
        tracing::warn!(
            channel_id = %channel_id,
            phase = "poll",
            "handler task panicked"
        );
        crate::metrics::record_handler_failure(&channel_id);
    }
}

fn record_provider_completion(result: Result<(), tokio::task::JoinError>, channel_id: &str) {
    let Err(error) = result else {
        return;
    };

    let failure = if error.is_cancelled() {
        "unexpected cancellation"
    } else {
        "panic"
    };
    tracing::warn!(channel_id = %channel_id, failure, "provider task failed");
    crate::metrics::record_provider_failure(channel_id);
}

fn record_forwarder_completion(result: Result<(), tokio::task::JoinError>) {
    let Err(error) = result else {
        return;
    };

    let failure = if error.is_cancelled() {
        "unexpected cancellation"
    } else {
        "panic"
    };
    tracing::warn!(failure, "listener forwarding task failed");
    crate::metrics::record_handler_failure("_forwarder");
}

async fn forward_provider(
    channel_id: String,
    mut provider_rx: mpsc::Receiver<InboundMessage>,
    mut provider_handles: JoinSet<()>,
    merged_tx: mpsc::Sender<InboundMessage>,
) {
    let mut receiver_open = true;

    while receiver_open || !provider_handles.is_empty() {
        while let Some(result) = provider_handles.try_join_next() {
            record_provider_completion(result, &channel_id);
        }
        if !receiver_open && provider_handles.is_empty() {
            break;
        }

        tokio::select! {
            message = provider_rx.recv(), if receiver_open => {
                match message {
                    Some(message) => {
                        if merged_tx.send(message).await.is_err() {
                            // The merged receiver is gone. Dropping the JoinSet
                            // aborts provider work that no longer has a consumer.
                            return;
                        }
                    }
                    None => receiver_open = false,
                }
            }
            result = provider_handles.join_next(), if !provider_handles.is_empty() => {
                if let Some(result) = result {
                    record_provider_completion(result, &channel_id);
                }
            }
        }
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
    /// Provider tasks remain owned by the listener's `JoinSet`.
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
        let max_concurrent_handlers = max_concurrent_handlers.max(1);
        tracing::info!(
            provider_tasks = handles.len(),
            max_concurrent_handlers,
            "channel listener started"
        );
        Self {
            rx: Some(rx),
            handles: Some(handles),
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
        let mut handler_set = JoinSet::new();
        #[expect(
            clippy::expect_used,
            reason = "run consumes self and rx is only taken here"
        )]
        let mut rx = self.rx.take().expect("receiver already consumed");
        #[expect(
            clippy::expect_used,
            reason = "run consumes self and handles are only taken here"
        )]
        let mut forwarding_handles = self.handles.take().expect("handles already consumed");
        let mut receiver_open = true;
        let mut pending_message = None;

        // Provider completions, inbound messages, and handler completions are
        // observed in one lifecycle loop. A failed account is therefore visible
        // immediately even while another account keeps the merged receiver open.
        while receiver_open || pending_message.is_some() || !forwarding_handles.is_empty() {
            while let Some(result) = forwarding_handles.try_join_next() {
                record_forwarder_completion(result);
            }
            while let Some(result) = handler_set.try_join_next() {
                Self::record_handler_completion(result);
            }
            if !receiver_open && pending_message.is_none() && forwarding_handles.is_empty() {
                break;
            }

            if pending_message.is_some()
                && handler_set.len() < self.max_concurrent_handlers
                && let Some(message) = pending_message.take()
            {
                Self::spawn_handler(&mut handler_set, &handler, message);
                continue;
            }

            tokio::select! {
                message = rx.recv(), if receiver_open && pending_message.is_none() => {
                    match message {
                        Some(message) if handler_set.len() < self.max_concurrent_handlers => {
                            Self::spawn_handler(&mut handler_set, &handler, message);
                        }
                        Some(message) => {
                            crate::metrics::record_inbound_handler_saturation();
                            pending_message = Some(message);
                        }
                        None => receiver_open = false,
                    }
                }
                result = forwarding_handles.join_next(), if !forwarding_handles.is_empty() => {
                    if let Some(result) = result {
                        record_forwarder_completion(result);
                    }
                }
                result = handler_set.join_next(), if !handler_set.is_empty() => {
                    if let Some(result) = result {
                        Self::record_handler_completion(result);
                    }
                }
            }
        }

        // The input and every provider are drained; finish already-accepted
        // handlers rather than detaching or aborting them during shutdown.
        while let Some(result) = handler_set.join_next().await {
            Self::record_handler_completion(result);
        }

        // Defensively drain any handle left by a future change to the unified
        // loop; shutdown must never detach provider work.
        while let Some(result) = forwarding_handles.join_next().await {
            record_forwarder_completion(result);
        }

        tracing::info!("channel listener stopped");
    }

    fn spawn_handler<F, Fut>(
        handler_set: &mut JoinSet<()>,
        handler: &Arc<F>,
        message: InboundMessage,
    ) where
        F: Fn(InboundMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        let span = info_span!(
            "inbound_message",
            msg.channel = %message.channel,
            msg.source = %crate::redact::identifier(&message.sender),
        );
        let channel_id = message.channel.clone();
        let future = invoke_handler(
            Arc::clone(handler),
            message,
            channel_id,
            crate::metrics::InboundHandlerGuard::new(),
        );
        handler_set.spawn(future.instrument(span));
    }

    fn record_handler_completion(result: Result<(), tokio::task::JoinError>) {
        if let Err(error) = result {
            let failure = if error.is_cancelled() {
                "unexpected cancellation"
            } else {
                "internal wrapper panic"
            };
            tracing::warn!(failure, "handler wrapper task failed");
            crate::metrics::record_handler_failure("_unknown");
        }
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
            let (provider_rx, provider_handles) = provider.listen(poll_interval, cancel.clone());
            let tx = merged_tx.clone();
            merged_handles.spawn(forward_provider(
                channel_id,
                provider_rx,
                provider_handles,
                tx,
            ));
        }

        drop(merged_tx);
        (merged_rx, merged_handles)
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    const TEST_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

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
                    account_id: None,
                    message_id: None,
                    text: text.to_owned(),
                    timestamp: 100,
                    attachments: vec![],
                    raw: None,
                }],
            }
        }
    }

    fn test_message(channel: &str, text: &str) -> InboundMessage {
        InboundMessage {
            channel: channel.to_owned(),
            sender: format!("{channel}-sender"),
            sender_name: None,
            group_id: None,
            account_id: None,
            message_id: None,
            text: text.to_owned(),
            timestamp: 100,
            attachments: vec![],
            raw: None,
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
        assert_eq!(crate::redact::identifier("+1234567890"), "...7890");
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
            account_id: None,
            message_id: None,
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
    #[expect(
        clippy::await_holding_lock,
        reason = "current_thread executor; the lock serializes process-global gauge mutations"
    )]
    async fn listener_run_dispatches_to_handler() {
        let _gauge_lock = crate::metrics::GAUGE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
                account_id: None,
                message_id: None,
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

    #[tokio::test(flavor = "multi_thread")]
    #[expect(
        clippy::await_holding_lock,
        reason = "the lock serializes process-global gauge mutations and is never acquired by handler tasks"
    )]
    async fn run_caps_concurrent_handlers_at_configured_limit() {
        let _gauge_lock = crate::metrics::GAUGE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (tx, rx) = mpsc::channel(16);
        let listener = ChannelListener::from_parts_with_config(rx, JoinSet::new(), 2);

        let active = std::sync::Arc::new(AtomicUsize::new(0));
        let max_seen = std::sync::Arc::new(AtomicUsize::new(0));
        let completed = std::sync::Arc::new(AtomicUsize::new(0));
        // WHY: Barrier(2) forces overlapping handlers to rendezvous; a
        // serialized (cap-1) run can never release the barrier, and an
        // uncapped run would push max_seen past 2.
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));

        for i in 0_u64..4 {
            tx.send(InboundMessage {
                channel: "signal".to_owned(),
                sender: format!("+{i}"),
                sender_name: None,
                group_id: None,
                account_id: None,
                message_id: None,
                text: format!("flood-{i}"),
                timestamp: i,
                attachments: vec![],
                raw: None,
            })
            .await
            .expect("send");
        }
        drop(tx);

        let max_seen_clone = max_seen.clone();
        let completed_clone = completed.clone();

        listener
            .run(move |_msg| {
                let active = active.clone();
                let max_seen = max_seen_clone.clone();
                let completed = completed_clone.clone();
                let barrier = barrier.clone();
                async move {
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);
                    tokio::time::timeout(TEST_DRAIN_TIMEOUT, barrier.wait())
                        .await
                        .expect("configured concurrency must allow two handlers to overlap");
                    active.fetch_sub(1, Ordering::SeqCst);
                    completed.fetch_add(1, Ordering::SeqCst);
                }
            })
            .await;

        assert_eq!(completed.load(Ordering::SeqCst), 4);
        assert_eq!(
            max_seen.load(Ordering::SeqCst),
            2,
            "handler concurrency must reach but never exceed the configured cap"
        );
    }

    #[tokio::test]
    async fn listener_drop_aborts_owned_tasks_promptly() {
        let (_tx, rx) = mpsc::channel::<InboundMessage>(16);

        struct NotifyOnDrop(Option<tokio::sync::oneshot::Sender<()>>);

        impl Drop for NotifyOnDrop {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(());
                }
            }
        }

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        let mut handles = JoinSet::new();
        handles.spawn(async move {
            let _drop_signal = NotifyOnDrop(Some(dropped_tx));
            started_tx
                .send(())
                .expect("task-start receiver remains live");
            std::future::pending::<()>().await;
        });
        let listener = ChannelListener::from_parts(rx, handles);
        tokio::time::timeout(TEST_DRAIN_TIMEOUT, started_rx)
            .await
            .expect("owned provider task must start before the deadline")
            .expect("task-start sender must remain valid");

        drop(listener);

        tokio::time::timeout(TEST_DRAIN_TIMEOUT, dropped_rx)
            .await
            .expect("owned provider task must be aborted promptly when listener drops")
            .expect("drop notification sender must remain valid until task teardown");
    }

    #[tokio::test]
    async fn into_receiver_returns_handles() {
        let (_tx, rx) = mpsc::channel::<InboundMessage>(16);

        let mut join_set = JoinSet::new();
        join_set.spawn(async {
            std::future::pending::<()>().await;
        });
        let listener = ChannelListener::from_parts(rx, join_set);
        let (_rx, mut handles) = listener.into_receiver();

        assert_eq!(handles.len(), 1);
        handles.abort_all();
        let result = tokio::time::timeout(TEST_DRAIN_TIMEOUT, handles.join_next())
            .await
            .expect("aborted handle must become joinable promptly")
            .expect("one handle must be returned");
        assert!(result.is_err_and(|error| error.is_cancelled()));
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

    /// One task fails while a sibling keeps this provider's receiver alive.
    struct PartiallyFailedProvider;

    impl ChannelProvider for PartiallyFailedProvider {
        fn id(&self) -> &'static str {
            "partially-failed-provider"
        }

        fn name(&self) -> &'static str {
            self.id()
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
            cancel: CancellationToken,
        ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>) {
            let (tx, rx) = mpsc::channel(16);
            let mut handles = JoinSet::new();
            handles.spawn(async move { panic!("one provider account failed") });
            handles.spawn(async move {
                cancel.cancelled().await;
                drop(tx);
            });
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

    /// Stops an owned polling task cooperatively when shutdown is requested.
    struct CooperativeCancellationProvider;

    impl ChannelProvider for CooperativeCancellationProvider {
        fn id(&self) -> &'static str {
            "cooperative-cancellation-provider"
        }

        fn name(&self) -> &'static str {
            self.id()
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
            cancel: CancellationToken,
        ) -> (mpsc::Receiver<InboundMessage>, JoinSet<()>) {
            let (tx, rx) = mpsc::channel(16);
            let mut handles = JoinSet::new();
            handles.spawn(async move {
                cancel.cancelled().await;
                drop(tx);
            });
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

    /// Returns a cancelled task without a shutdown request.
    struct UnexpectedCancellationProvider;

    impl ChannelProvider for UnexpectedCancellationProvider {
        fn id(&self) -> &'static str {
            "unexpected-cancellation-provider"
        }

        fn name(&self) -> &'static str {
            self.id()
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
            let mut handles = JoinSet::new();
            let task = handles.spawn(async move {
                std::future::pending::<()>().await;
                drop(tx);
            });
            task.abort();
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

    fn gauge_value_for(encoded: &str, metric: &str) -> Option<i64> {
        let needle = format!("{metric} ");
        encoded.lines().find_map(|line| {
            line.strip_prefix(&needle)
                .and_then(|value| value.split_whitespace().next())
                .and_then(|value| value.parse::<i64>().ok())
        })
    }

    async fn wait_for_counter_increment(
        registry: &koina::metrics::MetricsRegistry,
        metric: &str,
        labels: &str,
        before: u64,
    ) {
        tokio::time::timeout(TEST_DRAIN_TIMEOUT, async {
            loop {
                let observed =
                    counter_value_for(&encode_metrics(registry), metric, labels).unwrap_or(0);
                if observed > before {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("metric increment must be observed before the drain deadline");
    }

    #[tokio::test]
    async fn provider_task_failure_is_counted() {
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
    async fn run_observes_forwarder_failure_before_input_eof() {
        let (tx, rx) = mpsc::channel(1);
        let mut handles = JoinSet::new();
        handles.spawn(async { panic!("forwarder failed while input remains open") });
        let listener = ChannelListener::from_parts(rx, handles);
        let registry = fresh_registry();
        let labels = "channel_id=\"_forwarder\"";
        let before = counter_value_for(
            &encode_metrics(&registry),
            "aletheia_handler_failures_total",
            labels,
        )
        .unwrap_or(0);

        let mut run = tokio::spawn(listener.run(|_message| std::future::ready(())));
        wait_for_counter_increment(&registry, "aletheia_handler_failures_total", labels, before)
            .await;
        assert!(
            !run.is_finished(),
            "the open input sender must keep the listener alive"
        );

        drop(tx);
        tokio::time::timeout(TEST_DRAIN_TIMEOUT, &mut run)
            .await
            .expect("listener must finish after input EOF")
            .expect("listener task must join without panic");
    }

    #[tokio::test]
    async fn provider_failure_is_observed_while_its_peer_remains_live() {
        let provider = PartiallyFailedProvider;
        let providers: [&dyn ChannelProvider; 1] = [&provider];
        let cancel = CancellationToken::new();
        let listener = ChannelListener::start_many(providers, None, &cancel);
        let registry = fresh_registry();
        let labels = "channel_id=\"partially-failed-provider\"";
        let before = counter_value_for(
            &encode_metrics(&registry),
            "aletheia_provider_failures_total",
            labels,
        )
        .unwrap_or(0);

        let mut run = tokio::spawn(listener.run(|_message| std::future::ready(())));
        wait_for_counter_increment(
            &registry,
            "aletheia_provider_failures_total",
            labels,
            before,
        )
        .await;
        assert!(
            !run.is_finished(),
            "the live peer must still keep the provider receiver open"
        );

        cancel.cancel();
        tokio::time::timeout(TEST_DRAIN_TIMEOUT, &mut run)
            .await
            .expect("listener must drain promptly after cancellation")
            .expect("listener task must join without panic");
    }

    #[tokio::test]
    async fn shutdown_cancellation_is_prompt_and_not_a_provider_failure() {
        let provider = CooperativeCancellationProvider;
        let providers: [&dyn ChannelProvider; 1] = [&provider];
        let cancel = CancellationToken::new();
        let listener = ChannelListener::start_many(providers, None, &cancel);
        let registry = fresh_registry();
        let labels = "channel_id=\"cooperative-cancellation-provider\"";
        let before = counter_value_for(
            &encode_metrics(&registry),
            "aletheia_provider_failures_total",
            labels,
        )
        .unwrap_or(0);

        let run = tokio::spawn(listener.run(|_message| std::future::ready(())));
        tokio::task::yield_now().await;
        cancel.cancel();
        tokio::time::timeout(TEST_DRAIN_TIMEOUT, run)
            .await
            .expect("listener must stop promptly after its token is cancelled")
            .expect("listener task must join without panic");

        let after = counter_value_for(
            &encode_metrics(&registry),
            "aletheia_provider_failures_total",
            labels,
        )
        .unwrap_or(0);
        assert_eq!(after, before, "normal shutdown cancellation is not failure");
    }

    #[tokio::test]
    async fn unexpected_task_cancellation_is_a_provider_failure() {
        let provider = UnexpectedCancellationProvider;
        let providers: [&dyn ChannelProvider; 1] = [&provider];
        let cancel = CancellationToken::new();
        let listener = ChannelListener::start_many(providers, None, &cancel);
        let registry = fresh_registry();
        let labels = "channel_id=\"unexpected-cancellation-provider\"";
        let before = counter_value_for(
            &encode_metrics(&registry),
            "aletheia_provider_failures_total",
            labels,
        )
        .unwrap_or(0);

        tokio::time::timeout(
            TEST_DRAIN_TIMEOUT,
            listener.run(|_message| std::future::ready(())),
        )
        .await
        .expect("cancelled provider task must be observed without hanging");

        let after = counter_value_for(
            &encode_metrics(&registry),
            "aletheia_provider_failures_total",
            labels,
        )
        .unwrap_or(0);
        assert_eq!(after, before + 1);
    }

    #[tokio::test]
    #[expect(
        clippy::await_holding_lock,
        reason = "current_thread executor; the lock serializes process-global gauge assertions"
    )]
    async fn concurrent_listeners_add_their_in_flight_gauge_contributions() {
        let _gauge_lock = crate::metrics::GAUGE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let registry = fresh_registry();
        let before = gauge_value_for(
            &encode_metrics(&registry),
            "aletheia_inbound_handlers_in_flight",
        )
        .unwrap_or(0);

        let (tx_a, rx_a) = mpsc::channel(1);
        let (tx_b, rx_b) = mpsc::channel(1);
        tx_a.send(test_message("gauge-a", "first"))
            .await
            .expect("send first handler message");
        tx_b.send(test_message("gauge-b", "second"))
            .await
            .expect("send second handler message");
        drop(tx_a);
        drop(tx_b);

        let permits = Arc::new(tokio::sync::Semaphore::new(0));
        let (started_tx, mut started_rx) = mpsc::channel(2);
        let make_handler = || {
            let permits = Arc::clone(&permits);
            let started_tx = started_tx.clone();
            move |_message| {
                let permits = Arc::clone(&permits);
                let started_tx = started_tx.clone();
                async move {
                    started_tx.send(()).await.expect("report handler start");
                    let permit = permits.acquire().await.expect("semaphore remains open");
                    permit.forget();
                }
            }
        };

        let run_a =
            tokio::spawn(ChannelListener::from_parts(rx_a, JoinSet::new()).run(make_handler()));
        let run_b =
            tokio::spawn(ChannelListener::from_parts(rx_b, JoinSet::new()).run(make_handler()));
        drop(started_tx);

        for _ in 0..2 {
            tokio::time::timeout(TEST_DRAIN_TIMEOUT, started_rx.recv())
                .await
                .expect("both handlers must start before the deadline")
                .expect("handler start channel must remain open");
        }
        let during = gauge_value_for(
            &encode_metrics(&registry),
            "aletheia_inbound_handlers_in_flight",
        )
        .unwrap_or(0);
        assert_eq!(during, before + 2);

        permits.add_permits(2);
        tokio::time::timeout(TEST_DRAIN_TIMEOUT, run_a)
            .await
            .expect("first listener must drain")
            .expect("first listener task must join");
        tokio::time::timeout(TEST_DRAIN_TIMEOUT, run_b)
            .await
            .expect("second listener must drain")
            .expect("second listener task must join");
        let after = gauge_value_for(
            &encode_metrics(&registry),
            "aletheia_inbound_handlers_in_flight",
        )
        .unwrap_or(0);
        assert_eq!(after, before);
    }

    #[tokio::test]
    #[expect(
        clippy::await_holding_lock,
        reason = "current_thread executor; the lock serializes process-global gauge mutations"
    )]
    async fn handler_task_failure_is_counted() {
        let _gauge_lock = crate::metrics::GAUGE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (tx, rx) = mpsc::channel(16);
        let listener = ChannelListener::from_parts(rx, JoinSet::new());

        tx.send(test_message("async-panic-test", "boom"))
            .await
            .expect("send");
        drop(tx);

        let r = fresh_registry();
        let labels = "channel_id=\"async-panic-test\"";
        let before = counter_value_for(
            &encode_metrics(&r),
            "aletheia_handler_failures_total",
            labels,
        )
        .unwrap_or(0);
        listener
            .run(|_msg| async move { panic!("handler task failed") })
            .await;

        let out = encode_metrics(&r);
        let count = counter_value_for(&out, "aletheia_handler_failures_total", labels);
        assert_eq!(
            count,
            Some(before + 1),
            "handler failure should be counted once; got: {out}"
        );
    }

    #[tokio::test]
    #[expect(
        clippy::await_holding_lock,
        reason = "current_thread executor; the lock serializes process-global metric assertions"
    )]
    async fn synchronous_handler_panic_keeps_channel_attribution() {
        let _gauge_lock = crate::metrics::GAUGE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (tx, rx) = mpsc::channel(1);
        let listener = ChannelListener::from_parts(rx, JoinSet::new());
        tx.send(test_message("sync-panic-test", "boom"))
            .await
            .expect("send");
        drop(tx);

        let registry = fresh_registry();
        let labels = "channel_id=\"sync-panic-test\"";
        let before = counter_value_for(
            &encode_metrics(&registry),
            "aletheia_handler_failures_total",
            labels,
        )
        .unwrap_or(0);
        listener
            .run(|_message| -> std::future::Ready<()> {
                panic!("synchronous handler construction panic")
            })
            .await;
        let after = counter_value_for(
            &encode_metrics(&registry),
            "aletheia_handler_failures_total",
            labels,
        )
        .unwrap_or(0);
        assert_eq!(after, before + 1);
    }
}
