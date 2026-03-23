//! Channel registry: the single source of truth for available channels.

use std::sync::Arc;

use indexmap::IndexMap;
use snafu::ensure;

use crate::error::{self, Result};
use crate::types::{ChannelProvider, ProbeResult, SendParams, SendResult};

/// Registry of available channel providers.
///
/// Channels are registered at startup and looked up by ID during send operations.
/// Uses `IndexMap` to preserve insertion order.
pub struct ChannelRegistry {
    providers: IndexMap<String, Arc<dyn ChannelProvider>>,
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

    /// Look up a provider by channel ID.
    #[must_use]
    pub fn get(&self, channel_id: &str) -> Option<&Arc<dyn ChannelProvider>> {
        self.providers.get(channel_id)
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

    /// Probe all registered channels for health status.
    pub async fn probe_all(&self) -> IndexMap<String, ProbeResult> {
        let mut results = IndexMap::with_capacity(self.providers.len());
        for (id, provider) in &self.providers {
            let result = provider.probe().await;
            results.insert(id.clone(), result);
        }
        results
    }

    /// List all registered channel IDs, in insertion order.
    #[must_use]
    pub fn channels(&self) -> Vec<&str> {
        self.providers.keys().map(String::as_str).collect()
    }

    /// Number of registered channels.
    #[must_use]
    pub fn len(&self) -> usize {
        self.providers.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
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
    }

    impl MockProvider {
        fn new(id: &str) -> Self {
            Self {
                channel_id: id.to_owned(),
                channel_name: format!("Mock {id}"),
                send_result: SendResult {
                    sent: true,
                    error: None,
                },
                probe_result: ProbeResult {
                    ok: true,
                    latency_ms: Some(42),
                    error: None,
                    details: None,
                },
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
            Box::pin(async { self.probe_result.clone() })
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

    #[test]
    fn register_and_lookup() {
        let mut reg = ChannelRegistry::new();
        let provider = Arc::new(MockProvider::new("signal"));
        reg.register(provider).expect("register");

        let found = reg.get("signal").expect("found");
        assert_eq!(found.id(), "signal");
        assert_eq!(found.name(), "Mock signal");
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
        reg.register(Arc::new(MockProvider::new("signal").with_send_result(
            SendResult {
                sent: true,
                error: None,
            },
        )))
        .expect("register signal");
        reg.register(Arc::new(MockProvider::new("slack").with_send_result(
            SendResult {
                sent: false,
                error: Some("slack down".to_owned()),
            },
        )))
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

    #[test]
    fn channels_lists_registered_ids() {
        let mut reg = ChannelRegistry::new();
        reg.register(Arc::new(MockProvider::new("signal")))
            .expect("register");
        reg.register(Arc::new(MockProvider::new("slack")))
            .expect("register");
        reg.register(Arc::new(MockProvider::new("discord")))
            .expect("register");

        let channels = reg.channels();
        assert_eq!(channels, vec!["signal", "slack", "discord"]);
    }

    #[test]
    fn empty_registry() {
        let reg = ChannelRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.channels().is_empty());
    }

    #[test]
    fn lookup_missing_returns_none() {
        let reg = ChannelRegistry::new();
        assert!(reg.get("nonexistent").is_none());
    }
}
