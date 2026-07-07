//! Channel registry: the single source of truth for available channels.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::StreamExt;
use indexmap::IndexMap;
use snafu::ensure;

use crate::error::{self, Result};
use crate::types::{ChannelProvider, ProbeResult, SendParams, SendResult};

/// Default wall-clock timeout applied to each provider probe.
const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Default maximum number of provider probes that may run concurrently.
const DEFAULT_MAX_CONCURRENT_PROBES: usize = 8;

/// Registry of available channel providers.
///
/// Channels are registered at startup and looked up by ID during send operations.
/// Uses `IndexMap` to preserve insertion order.
pub struct ChannelRegistry {
    providers: IndexMap<String, Arc<dyn ChannelProvider>>,
    /// Wall-clock timeout applied to each individual provider probe.
    probe_timeout: Duration,
    /// Maximum number of provider probes that may run concurrently.
    max_concurrent_probes: usize,
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            providers: IndexMap::new(),
            probe_timeout: DEFAULT_PROBE_TIMEOUT,
            max_concurrent_probes: DEFAULT_MAX_CONCURRENT_PROBES,
        }
    }

    /// Register a channel provider. Fails if a provider with the same ID exists.
    ///
    /// # Errors
    ///
    /// Returns an error if a provider with the same ID is already registered.
    pub fn register(&mut self, provider: Arc<dyn ChannelProvider>) -> Result<()> {
        let id = provider.id().to_owned();
        ensure!(
            !self.providers.contains_key(&id),
            error::DuplicateChannelSnafu { id }
        );
        self.providers.insert(id, provider);
        Ok(())
    }

    /// Send a message through a specific channel.
    ///
    /// Provider-level failures are captured in [`SendResult::error`].
    ///
    /// # Errors
    ///
    /// Returns an error if the channel is not registered.
    pub async fn send(&self, channel_id: &str, params: &SendParams) -> Result<SendResult> {
        let provider = self.providers.get(channel_id).ok_or_else(|| {
            error::UnknownChannelSnafu {
                id: channel_id.to_owned(),
            }
            .build()
        })?;
        let result = provider.send(params).await;
        crate::metrics::record_channel_message(channel_id, result.sent);
        Ok(result)
    }

    /// Set the wall-clock timeout applied to each provider probe.
    ///
    /// Values below one millisecond are clamped to one millisecond so that
    /// probes still have a chance to complete on fast providers.
    #[must_use]
    pub fn with_probe_timeout(mut self, timeout: Duration) -> Self {
        self.probe_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    /// Set the maximum number of provider probes that may run concurrently.
    ///
    /// Values below one are clamped to one so that progress is always made.
    #[must_use]
    pub fn with_max_concurrent_probes(mut self, max: usize) -> Self {
        self.max_concurrent_probes = max.max(1);
        self
    }

    /// Probe all registered channels for health status.
    ///
    /// # Complexity
    ///
    /// O(c) where c is the number of registered channels. Each probe is
    /// executed concurrently (bounded by [`Self::with_max_concurrent_probes`]),
    /// so wall-clock time is O(1) (bounded by the configured per-provider
    /// timeout), but total work scales linearly with channels.
    pub async fn probe_all(&self) -> IndexMap<String, ProbeResult> {
        let mut results = IndexMap::with_capacity(self.providers.len());
        if self.providers.is_empty() {
            return results;
        }

        let probe_futures = self.providers.iter().map(|(id, provider)| {
            let id = id.clone();
            let provider = Arc::clone(provider);
            let timeout = self.probe_timeout;
            async move {
                let result = match tokio::time::timeout(timeout, provider.probe()).await {
                    Ok(result) => result,
                    Err(_) => ProbeResult {
                        ok: false,
                        latency_ms: None,
                        error: Some("probe timed out".to_owned()),
                        details: None,
                    },
                };
                (id, result)
            }
        });

        // WHY: bounded concurrency prevents a large fleet from spawning an
        // unbounded number of concurrent probe tasks.
        let mut by_id: HashMap<String, ProbeResult> =
            futures::stream::iter(probe_futures)
                .buffer_unordered(self.max_concurrent_probes)
                .collect()
                .await;

        // WHY: restore provider insertion order after concurrent collection.
        for id in self.providers.keys() {
            if let Some(result) = by_id.remove(id) {
                results.insert(id.clone(), result);
            }
        }
        results
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: HashMap key indexing; key presence asserted by results.len() == 2"
)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use crate::types::ChannelCapabilities;

    use super::*;

    static MOCK_CAPS: ChannelCapabilities = ChannelCapabilities {
        threads: false,
        reactions: false,
        typing: false,
        media: false,
        streaming: false,
        rich_formatting: false,
        max_text_length: 1000,
    };

    struct MockProvider {
        channel_id: String,
        channel_name: String,
        send_result: SendResult,
        probe_result: ProbeResult,
        probe_delay: Option<Duration>,
    }

    impl MockProvider {
        fn new(id: &str) -> Self {
            Self {
                channel_id: id.to_owned(),
                channel_name: format!("Mock {id}"),
                send_result: SendResult::ok(),
                probe_result: ProbeResult {
                    ok: true,
                    latency_ms: Some(42),
                    error: None,
                    details: None,
                },
                probe_delay: None,
            }
        }

        fn with_send_result(mut self, result: SendResult) -> Self {
            self.send_result = result;
            self
        }

        fn with_probe_result(mut self, result: ProbeResult) -> Self {
            self.probe_result = result;
            self
        }

        fn with_probe_delay(mut self, delay: Duration) -> Self {
            self.probe_delay = Some(delay);
            self
        }
    }

    impl ChannelProvider for MockProvider {
        fn id(&self) -> &str {
            &self.channel_id
        }

        fn name(&self) -> &str {
            &self.channel_name
        }

        fn capabilities(&self) -> &ChannelCapabilities {
            &MOCK_CAPS
        }

        fn send<'a>(
            &'a self,
            _params: &'a SendParams,
        ) -> Pin<Box<dyn Future<Output = SendResult> + Send + 'a>> {
            Box::pin(async { self.send_result.clone() })
        }

        fn probe<'a>(&'a self) -> Pin<Box<dyn Future<Output = ProbeResult> + Send + 'a>> {
            let result = self.probe_result.clone();
            let delay = self.probe_delay;
            Box::pin(async move {
                if let Some(delay) = delay {
                    tokio::time::sleep(delay).await;
                }
                result
            })
        }

        fn listen(
            &self,
            _poll_interval: Option<std::time::Duration>,
            _cancel: tokio_util::sync::CancellationToken,
        ) -> (
            tokio::sync::mpsc::Receiver<crate::types::InboundMessage>,
            tokio::task::JoinSet<()>,
        ) {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            (rx, tokio::task::JoinSet::new())
        }
    }

    fn test_params(to: &str) -> SendParams {
        SendParams {
            to: to.to_owned(),
            text: "hello".to_owned(),
            account_id: None,
            thread_id: None,
            attachments: None,
        }
    }

    #[tokio::test]
    async fn register_and_send() {
        let mut reg = ChannelRegistry::new();
        let provider = Arc::new(MockProvider::new("signal"));
        reg.register(provider).expect("register");

        let result = reg.send("signal", &test_params("+1234567890")).await;
        assert!(result.expect("send").sent);
    }

    #[test]
    fn duplicate_registration_fails() {
        let mut reg = ChannelRegistry::new();
        reg.register(Arc::new(MockProvider::new("signal")))
            .expect("first");
        let err = reg
            .register(Arc::new(MockProvider::new("signal")))
            .expect_err("duplicate");
        assert!(err.to_string().contains("duplicate channel: signal"));
    }

    #[tokio::test]
    async fn send_routes_to_correct_provider() {
        let mut reg = ChannelRegistry::new();
        reg.register(Arc::new(
            MockProvider::new("signal").with_send_result(SendResult::ok()),
        ))
        .expect("register signal");
        reg.register(Arc::new(
            MockProvider::new("slack").with_send_result(SendResult::err("slack down")),
        ))
        .expect("register slack");

        let signal_result = reg
            .send("signal", &test_params("+1234567890"))
            .await
            .expect("send");
        assert!(signal_result.sent);

        let slack_result = reg
            .send("slack", &test_params("C0123"))
            .await
            .expect("send");
        assert!(!slack_result.sent);
        assert_eq!(slack_result.error.as_deref(), Some("slack down"));
    }

    #[tokio::test]
    async fn send_unknown_channel_errors() {
        let reg = ChannelRegistry::new();
        let err = reg
            .send("nonexistent", &test_params("x"))
            .await
            .expect_err("unknown");
        assert!(err.to_string().contains("unknown channel: nonexistent"));
    }

    #[tokio::test]
    async fn probe_all_collects_results() {
        let mut reg = ChannelRegistry::new();
        reg.register(Arc::new(MockProvider::new("signal")))
            .expect("register");
        reg.register(Arc::new(MockProvider::new("slack").with_probe_result(
            ProbeResult {
                ok: false,
                latency_ms: None,
                error: Some("unreachable".to_owned()),
                details: None,
            },
        )))
        .expect("register");

        let results = reg.probe_all().await;
        assert_eq!(results.len(), 2);
        assert!(results["signal"].ok);
        assert!(!results["slack"].ok);
    }

    #[tokio::test]
    async fn probe_all_preserves_insertion_order() {
        let mut reg = ChannelRegistry::new();
        reg.register(Arc::new(MockProvider::new("gamma")))
            .expect("register gamma");
        reg.register(Arc::new(MockProvider::new("alpha")))
            .expect("register alpha");
        reg.register(Arc::new(MockProvider::new("beta")))
            .expect("register beta");

        let results = reg.probe_all().await;
        let ids: Vec<&str> = results.keys().map(String::as_str).collect();
        assert_eq!(ids, vec!["gamma", "alpha", "beta"]);
    }

    #[tokio::test]
    async fn probe_all_slow_provider_does_not_block_fast_provider() {
        // WHY: a slow provider must not delay health results for fast providers.
        let mut reg = ChannelRegistry::new()
            .with_probe_timeout(Duration::from_millis(200))
            .with_max_concurrent_probes(2);

        reg.register(Arc::new(
            MockProvider::new("slow").with_probe_delay(Duration::from_secs(10)),
        ))
        .expect("register slow");
        reg.register(Arc::new(MockProvider::new("fast")))
            .expect("register fast");

        let start = std::time::Instant::now();
        let results = reg.probe_all().await;
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 2);
        assert!(!results["slow"].ok);
        assert_eq!(results["slow"].error.as_deref(), Some("probe timed out"));
        assert!(results["fast"].ok);

        // WHY: sequential execution would have taken at least 10s; concurrent
        // execution with the 200ms timeout should finish well under a second.
        assert!(
            elapsed < Duration::from_millis(800),
            "fast provider blocked by slow provider: {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn probe_all_bounds_concurrency() {
        // WHY: with concurrency of 1, two slow providers run back-to-back,
        // so total elapsed time is roughly the sum of both timeouts.
        let mut reg = ChannelRegistry::new()
            .with_probe_timeout(Duration::from_millis(100))
            .with_max_concurrent_probes(1);

        reg.register(Arc::new(
            MockProvider::new("first").with_probe_delay(Duration::from_secs(10)),
        ))
        .expect("register first");
        reg.register(Arc::new(
            MockProvider::new("second").with_probe_delay(Duration::from_secs(10)),
        ))
        .expect("register second");

        let start = std::time::Instant::now();
        let results = reg.probe_all().await;
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 2);
        assert!(!results["first"].ok);
        assert!(!results["second"].ok);
        assert!(elapsed >= Duration::from_millis(150));
    }
}
