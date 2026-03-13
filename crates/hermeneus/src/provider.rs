//! LLM provider trait — Anthropic-native with adapter support.
//!
//! Defines the interface all providers must implement. Types are modeled
//! on the Anthropic Messages API; other providers adapt to this surface.

use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use crate::anthropic::AnthropicProvider;
use crate::error::{self, Result};
use crate::health::{HealthConfig, ProviderHealth, ProviderHealthTracker};
use crate::types::{CompletionRequest, CompletionResponse, TokenCount};

/// Trait for LLM providers.
///
/// Implementations handle authentication, request formatting, response parsing,
/// and error mapping. The provider translates between the generic types in
/// [`types`](crate::types) and the wire format of the specific API.
///
/// `Send + Sync` required for use in async contexts and across threads.
/// Async methods return boxed futures to preserve `dyn LlmProvider` compatibility.
pub trait LlmProvider: Send + Sync {
    /// Send a completion request and return the full response.
    ///
    /// # Errors
    /// Returns an error on network failure, authentication issues,
    /// rate limiting, or response parsing failure.
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>>;

    /// List models supported by this provider.
    fn supported_models(&self) -> &[&str];

    /// Check if a specific model is supported.
    fn supports_model(&self, model: &str) -> bool {
        self.supported_models().contains(&model)
    }

    /// Provider name for logging and diagnostics.
    fn name(&self) -> &str;

    /// Estimate input tokens for a request (without calling the API).
    ///
    /// Default implementation returns `None` (unknown). Providers that
    /// support local tokenization should override this.
    fn estimate_tokens(&self, _text: &str) -> Option<u64> {
        None
    }

    /// Count tokens for a request via the provider's API.
    /// Returns None if the provider doesn't support server-side counting.
    fn count_tokens<'a>(
        &'a self,
        _request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Option<TokenCount>>> + Send + 'a>> {
        Box::pin(async { Ok(None) })
    }

    /// Whether this provider supports prompt caching.
    fn supports_caching(&self) -> bool {
        false
    }

    /// Whether this provider supports citation tracking.
    fn supports_citations(&self) -> bool {
        false
    }

    /// Downcast to concrete type for provider-specific features (e.g., streaming).
    fn as_any(&self) -> &dyn Any;
}

/// Per-model pricing rates for cost estimation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    /// Cost per million input tokens (USD).
    pub input_cost_per_mtok: f64,
    /// Cost per million output tokens (USD).
    pub output_cost_per_mtok: f64,
}

/// Configuration for provider initialization.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderConfig {
    /// Provider type: `anthropic`, `openai`, `ollama`.
    pub provider_type: String,
    /// API key or credential reference.
    pub api_key: Option<String>,
    /// Base URL override (for proxies or self-hosted).
    pub base_url: Option<String>,
    /// Default model to use.
    pub default_model: Option<String>,
    /// Maximum retries on transient failures.
    pub max_retries: Option<u32>,
    /// Per-model pricing for cost metrics. Keyed by model name.
    #[serde(default)]
    pub pricing: HashMap<String, ModelPricing>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            provider_type: "anthropic".to_owned(),
            api_key: None,
            base_url: None,
            default_model: Some("claude-opus-4-20250514".to_owned()),
            max_retries: Some(3),
            pricing: HashMap::new(),
        }
    }
}

struct ProviderEntry {
    provider: Box<dyn LlmProvider>,
    health: ProviderHealthTracker,
}

/// Provider registry — maps model IDs to providers with health tracking.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: Vec<ProviderEntry>,
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.providers.iter().map(|e| e.provider.name()).collect();
        f.debug_struct("ProviderRegistry")
            .field("providers", &names)
            .finish()
    }
}

impl ProviderRegistry {
    /// Create an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a provider with default health config.
    pub fn register(&mut self, provider: Box<dyn LlmProvider>) {
        self.register_with_config(provider, HealthConfig::default());
    }

    /// Register a provider with custom health thresholds.
    pub fn register_with_config(&mut self, provider: Box<dyn LlmProvider>, config: HealthConfig) {
        self.providers.push(ProviderEntry {
            provider,
            health: ProviderHealthTracker::new(config),
        });
    }

    /// Find a provider that supports the given model.
    #[must_use]
    pub fn find_provider(&self, model: &str) -> Option<&dyn LlmProvider> {
        self.providers
            .iter()
            .find(|e| e.provider.supports_model(model))
            .map(|e| e.provider.as_ref())
    }

    /// List all registered providers.
    pub fn providers(&self) -> Vec<&dyn LlmProvider> {
        self.providers.iter().map(|e| e.provider.as_ref()).collect()
    }

    /// Query health of a provider by name.
    #[must_use]
    pub fn provider_health(&self, name: &str) -> Option<ProviderHealth> {
        self.providers
            .iter()
            .find(|e| e.provider.name() == name)
            .map(|e| e.health.health())
    }

    /// Record a successful request for the named provider.
    pub fn record_success(&self, name: &str) {
        if let Some(entry) = self.providers.iter().find(|e| e.provider.name() == name) {
            entry.health.record_success();
        }
    }

    /// Find a streaming-capable provider for the given model.
    ///
    /// Returns `Some` if the provider supports streaming (currently only Anthropic).
    #[must_use]
    pub fn find_streaming_provider(&self, model: &str) -> Option<&AnthropicProvider> {
        self.find_provider(model)
            .and_then(|p| p.as_any().downcast_ref::<AnthropicProvider>())
    }

    /// Record a failed request for the named provider.
    pub fn record_error(&self, name: &str, error: &error::Error) {
        if let Some(entry) = self.providers.iter().find(|e| e.provider.name() == name) {
            entry.health.record_error(error);
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use super::*;
    use crate::types::*;

    /// A mock provider for testing.
    struct MockProvider {
        models: Vec<&'static str>,
    }

    impl MockProvider {
        fn new() -> Self {
            Self {
                models: vec!["mock-model-v1", "mock-model-v2"],
            }
        }
    }

    impl LlmProvider for MockProvider {
        fn complete<'a>(
            &'a self,
            _request: &'a CompletionRequest,
        ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
            Box::pin(async {
                Ok(CompletionResponse {
                    id: "mock-response-1".to_owned(),
                    model: "mock-model-v1".to_owned(),
                    stop_reason: StopReason::EndTurn,
                    content: vec![ContentBlock::Text {
                        text: "mock response".to_owned(),
                        citations: None,
                    }],
                    usage: Usage {
                        input_tokens: 100,
                        output_tokens: 50,
                        ..Usage::default()
                    },
                })
            })
        }

        fn supported_models(&self) -> &[&str] {
            &self.models
        }

        #[expect(
            clippy::unnecessary_literal_bound,
            reason = "trait requires &str return"
        )]
        fn name(&self) -> &str {
            "mock"
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[tokio::test]
    async fn mock_provider_completes() {
        let provider = MockProvider::new();
        let request = CompletionRequest {
            model: "mock-model-v1".to_owned(),
            system: None,
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hello".to_owned()),
            }],
            max_tokens: 1024,
            tools: vec![],
            temperature: None,
            thinking: None,
            stop_sequences: vec![],
            ..Default::default()
        };

        let response = provider.complete(&request).await.unwrap();
        assert_eq!(response.id, "mock-response-1");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn supports_model_check() {
        let provider = MockProvider::new();
        assert!(provider.supports_model("mock-model-v1"));
        assert!(provider.supports_model("mock-model-v2"));
        assert!(!provider.supports_model("nonexistent"));
    }

    #[test]
    fn registry_find_provider() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(MockProvider::new()));

        assert!(registry.find_provider("mock-model-v1").is_some());
        assert!(registry.find_provider("nonexistent").is_none());
    }

    #[test]
    fn registry_empty() {
        let registry = ProviderRegistry::new();
        assert!(registry.find_provider("any-model").is_none());
        assert!(registry.providers().is_empty());
    }

    #[test]
    fn provider_config_defaults() {
        let config = ProviderConfig::default();
        assert_eq!(config.provider_type, "anthropic");
        assert_eq!(
            config.default_model.as_deref(),
            Some("claude-opus-4-20250514")
        );
        assert!(config.pricing.is_empty());
    }

    #[test]
    fn mock_provider_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockProvider>();
    }

    #[test]
    fn registry_health_starts_up() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(MockProvider::new()));

        assert_eq!(registry.provider_health("mock"), Some(ProviderHealth::Up));
    }

    #[test]
    fn registry_health_unknown_provider() {
        let registry = ProviderRegistry::new();
        assert_eq!(registry.provider_health("nonexistent"), None);
    }

    #[test]
    fn registry_record_error_updates_health() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(MockProvider::new()));

        let err: crate::error::Error = crate::error::ApiRequestSnafu { message: "timeout" }.build();
        registry.record_error("mock", &err);

        match registry.provider_health("mock") {
            Some(ProviderHealth::Degraded {
                consecutive_errors, ..
            }) => {
                assert_eq!(consecutive_errors, 1);
            }
            other => panic!("expected Degraded, got {other:?}"),
        }
    }

    #[test]
    fn registry_record_success_resets_health() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(MockProvider::new()));

        let err: crate::error::Error = crate::error::ApiRequestSnafu { message: "timeout" }.build();
        registry.record_error("mock", &err);
        registry.record_success("mock");

        assert_eq!(registry.provider_health("mock"), Some(ProviderHealth::Up));
    }

    #[test]
    fn find_streaming_provider_returns_none_for_mock() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(MockProvider::new()));
        assert!(registry.find_streaming_provider("mock-model-v1").is_none());
    }

    #[test]
    fn registry_record_unknown_is_noop() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(MockProvider::new()));
        // Should not panic
        registry.record_success("nonexistent");
        let err: crate::error::Error = crate::error::ApiRequestSnafu { message: "timeout" }.build();
        registry.record_error("nonexistent", &err);
    }
}
