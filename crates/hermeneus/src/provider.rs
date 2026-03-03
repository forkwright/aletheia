//! LLM provider trait.
//!
//! Defines the interface that all providers (Anthropic, `OpenAI`, Ollama)
//! must implement. The primary implementation is [`crate::anthropic::AnthropicProvider`].
//! Designed for the current sync-first approach with async streaming planned for M2.

use crate::error::Result;
use crate::types::{CompletionRequest, CompletionResponse};

/// Trait for LLM providers.
///
/// Implementations handle authentication, request formatting, response parsing,
/// and error mapping. The provider translates between the generic types in
/// [`types`](crate::types) and the wire format of the specific API.
///
/// `Send + Sync` required for use in async contexts and across threads.
pub trait LlmProvider: Send + Sync {
    /// Send a completion request and return the full response.
    ///
    /// # Errors
    /// Returns an error on network failure, authentication issues,
    /// rate limiting, or response parsing failure.
    fn complete(&self, request: &CompletionRequest) -> Result<CompletionResponse>;

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
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            provider_type: "anthropic".to_owned(),
            api_key: None,
            base_url: None,
            default_model: Some("claude-opus-4-20250514".to_owned()),
            max_retries: Some(3),
        }
    }
}

/// Provider registry — maps model IDs to providers.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: Vec<Box<dyn LlmProvider>>,
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.providers.iter().map(|p| p.name()).collect();
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

    /// Register a provider.
    pub fn register(&mut self, provider: Box<dyn LlmProvider>) {
        self.providers.push(provider);
    }

    /// Find a provider that supports the given model.
    #[must_use]
    pub fn find_provider(&self, model: &str) -> Option<&dyn LlmProvider> {
        self.providers
            .iter()
            .find(|p| p.supports_model(model))
            .map(AsRef::as_ref)
    }

    /// List all registered providers.
    #[must_use]
    pub fn providers(&self) -> &[Box<dyn LlmProvider>] {
        &self.providers
    }
}

#[cfg(test)]
mod tests {
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
        fn complete(&self, _request: &CompletionRequest) -> Result<CompletionResponse> {
            Ok(CompletionResponse {
                id: "mock-response-1".to_owned(),
                model: "mock-model-v1".to_owned(),
                stop_reason: StopReason::EndTurn,
                content: vec![ContentBlock::Text {
                    text: "mock response".to_owned(),
                }],
                usage: Usage {
                    input_tokens: 100,
                    output_tokens: 50,
                    ..Usage::default()
                },
            })
        }

        fn supported_models(&self) -> &[&str] {
            &self.models
        }

        #[expect(clippy::unnecessary_literal_bound, reason = "trait requires &str return")]
        fn name(&self) -> &str {
            "mock"
        }
    }

    #[test]
    fn mock_provider_completes() {
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
        };

        let response = provider.complete(&request).unwrap();
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
    }

    #[test]
    fn mock_provider_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockProvider>();
    }
}
