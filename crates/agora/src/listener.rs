//! Unified channel listener: merges inbound messages from channel providers.

use tokio::sync::mpsc;
use tokio::task::JoinSet;

use tokio_util::sync::CancellationToken;

use crate::types::{ChannelProvider, InboundMessage};

/// Listens on registered channels, merging inbound messages into a single stream.
///
/// Dropping the listener aborts all background polling tasks through
/// [`JoinSet`]'s drop behavior unless [`into_receiver`](Self::into_receiver)
/// was called first, which transfers the receiver and an
/// [`ActiveSubscriptionGuard`] over the handles to the caller. Ownership of
/// the active-subscriptions gauge moves with the guard, since the listener
/// can no longer observe when tasks it no longer holds actually stop.
///
/// WHY(#7103): two shapes for consuming this listener were developed in
/// parallel and never reconciled -- a callback-owning `run(handler)` that
/// held the provider task handles for its own lifetime, and the
/// receiver-owning shape below, where `into_receiver` hands the raw
/// receiver and an `ActiveSubscriptionGuard` over the handles to the
/// caller. Receiver-owning won: it is what `aletheia::runtime::setup` has
/// run in production, and what #7101 (gauge-clearing on drop) and #7102
/// (inbound dedupe, `crate::dedupe::DedupeFilter`) were both landed
/// against. `run()` and its tests were deleted rather than kept
/// half-present as a second, unused consumption path; `into_receiver` plus
/// `ActiveSubscriptionGuard` is the one production affordance this type
/// offers, and handle ownership transfers to the caller on every
/// consumption, full stop.
pub struct ChannelListener {
    rx: Option<mpsc::Receiver<InboundMessage>>,
    handles: Option<JoinSet<()>>,
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
        // WHY(#7103): `max_concurrent_handlers` is accepted here (and by the
        // `_with_config` constructors above) purely to log what the caller
        // configured -- since `run()`'s deletion, nothing in this type
        // enforces it. The cap is enforced by the receiver-owning consumer
        // (`aletheia::dispatch::spawn_dispatcher`), not stored on this type.
        tracing::info!(
            subscriptions = count,
            max_concurrent_handlers,
            "channel listener started"
        );
        Self {
            rx: Some(rx),
            handles: Some(handles),
        }
    }

    /// Decrement the active-subscription gauge when the listener is dropped
    /// while it still owns the receiver and background tasks.
    ///
    /// [`into_receiver`](Self::into_receiver) transfers ownership of the
    /// receiver and handles to the caller as an [`ActiveSubscriptionGuard`];
    /// in that case `self.rx` is already `None`, the subscriptions are still
    /// active, and the gauge must not be cleared here -- the guard clears it
    /// instead, once the caller actually drops the transferred handles.
    fn decrement_on_drop(&mut self) {
        if self.rx.is_some() {
            crate::metrics::set_active_subscriptions(0);
        }
    }

    /// Fallback default; runtime reads `MessagingConfig::max_concurrent_handlers`.
    const DEFAULT_MAX_CONCURRENT_HANDLERS: usize = 64;

    /// Unwrap into the raw receiver and a guard over the background task
    /// handles for manual control.
    ///
    /// The returned [`ActiveSubscriptionGuard`] derefs to the background
    /// polling [`JoinSet`]: callers can abort it for immediate shutdown or
    /// await `join_next` for graceful drain. Tasks also stop naturally once
    /// the receiver is dropped (closed channel). Dropping the guard --
    /// directly, or via [`ActiveSubscriptionGuard::shutdown`] -- clears the
    /// active-subscriptions gauge, since ownership of the handles (and so of
    /// when the tasks actually stop) has moved to the caller.
    // WHY a message rather than a bare `#[must_use]`: `ActiveSubscriptionGuard`
    // already carries one, and clippy's `double_must_use` fires only on the
    // message-less form. The tuple itself is not `must_use`, so dropping the
    // pair on the floor would silently abort the polling tasks -- which is the
    // defect this whole change exists to fix.
    #[must_use = "both halves matter: dropping the receiver closes the channel, and dropping the guard aborts the polling tasks and clears the gauge"]
    pub fn into_receiver(mut self) -> (mpsc::Receiver<InboundMessage>, ActiveSubscriptionGuard) {
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
        (rx, ActiveSubscriptionGuard { handles })
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

/// Owns the background polling-task handles transferred out of a
/// [`ChannelListener`] by [`into_receiver`](ChannelListener::into_receiver).
///
/// Derefs to the underlying [`JoinSet`] so callers can still `abort_all` or
/// `join_next` exactly as they could on the raw handles. What this type adds
/// is a `Drop` impl that clears the active-subscriptions gauge: once
/// `into_receiver` has run, `ChannelListener` no longer holds the handles and
/// its own drop path is a no-op for metrics, so the gauge would otherwise
/// never clear (#5204). Metric ownership moves with the handles rather than
/// staying behind with the listener.
#[must_use = "dropping this re-clears the active-subscriptions gauge; hold it for the tasks' lifetime"]
pub struct ActiveSubscriptionGuard {
    handles: JoinSet<()>,
}

impl ActiveSubscriptionGuard {
    /// Await every background task to completion, logging failures, then
    /// clear the active-subscriptions gauge on drop.
    ///
    /// [`ChannelListener::into_receiver`] transferred these handles to the
    /// caller along with ownership of when they stop and of the
    /// active-subscriptions gauge, so draining them and clearing the gauge
    /// is this type's job, not the listener's.
    pub async fn shutdown(mut self) {
        while let Some(result) = self.handles.join_next().await {
            if let Err(e) = result {
                tracing::warn!(error = %e, "listener forwarding task failed");
                crate::metrics::record_handler_failure("_forwarder");
            }
        }
    }
}

impl std::ops::Deref for ActiveSubscriptionGuard {
    type Target = JoinSet<()>;

    fn deref(&self) -> &Self::Target {
        &self.handles
    }
}

impl std::ops::DerefMut for ActiveSubscriptionGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.handles
    }
}

impl Drop for ActiveSubscriptionGuard {
    fn drop(&mut self) {
        crate::metrics::set_active_subscriptions(0);
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::future::Future;
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
                    message_id: None,
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

    #[tokio::test]
    async fn listener_receives_messages() {
        let (tx, rx) = mpsc::channel(16);
        let listener = ChannelListener::from_parts(rx, JoinSet::new());

        let msg = InboundMessage {
            channel: "signal".to_owned(),
            sender: "+1234567890".to_owned(),
            sender_name: None,
            group_id: None,
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
        reason = "current_thread executor; no deadlock risk — GAUGE_TEST_LOCK is never reacquired within this test's own scope"
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

    fn fresh_registry() -> koina::metrics::MetricsRegistry {
        koina::metrics::fresh_registry_with(crate::metrics::register)
    }

    fn encode_metrics(r: &koina::metrics::MetricsRegistry) -> String {
        koina::metrics::encode_to_string(r)
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
    async fn into_receiver_guard_decrements_on_drop() {
        // #5204: once `into_receiver` transfers the handles out, the
        // listener's own drop path can no longer see them (`self.rx` and
        // `self.handles` are already `None`), so only the returned
        // `ActiveSubscriptionGuard`'s drop can still clear the gauge. Before
        // that guard existed, this exact path left the gauge stuck non-zero
        // forever: dropping the raw `JoinSet` `into_receiver` used to return
        // had no metrics effect at all.
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
        let listener = ChannelListener::from_parts(rx, handles);
        let before = encode_metrics(&r);
        assert!(
            before.contains("aletheia_active_subscriptions 1"),
            "got: {before}"
        );

        let (rx, subscription_handles) = listener.into_receiver();

        let still_active = encode_metrics(&r);
        assert!(
            still_active.contains("aletheia_active_subscriptions 1"),
            "gauge must stay set while the transferred handles are still \
             alive; got: {still_active}"
        );

        drop(rx);
        drop(subscription_handles);

        let after = encode_metrics(&r);
        assert!(
            after.contains("aletheia_active_subscriptions 0"),
            "gauge must clear once the transferred handles are dropped; \
             got: {after}"
        );
    }

    #[tokio::test]
    #[expect(
        clippy::await_holding_lock,
        reason = "current_thread executor; no deadlock risk — GAUGE_TEST_LOCK is never acquired inside shutdown()"
    )]
    async fn into_receiver_guard_shutdown_drains_and_clears_gauge() {
        let _guard = crate::metrics::GAUGE_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (_tx, rx) = mpsc::channel::<InboundMessage>(16);
        let mut handles = JoinSet::new();
        handles.spawn(async {});

        let r = fresh_registry();
        let listener = ChannelListener::from_parts(rx, handles);
        let (_rx, subscription_handles) = listener.into_receiver();

        subscription_handles.shutdown().await;

        let after = encode_metrics(&r);
        assert!(
            after.contains("aletheia_active_subscriptions 0"),
            "got: {after}"
        );
    }
}
