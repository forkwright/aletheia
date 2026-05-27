//! LLM provider trait: Anthropic-native with adapter support.
//!
//! Defines the interface all providers must implement. Types are modeled
//! on the Anthropic Messages API; other providers adapt to this surface.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use koina::secret::SecretString;

use crate::anthropic::StreamEvent;
use crate::error::{self, Result};
use crate::health::{HealthConfig, ProviderHealth, ProviderHealthTracker};
use crate::types::{CompletionRequest, CompletionResponse};

/// Trait for LLM providers.
///
/// Implementations handle authentication, request formatting, response parsing,
/// and error mapping. The provider translates between the generic types in
/// [`types`](crate::types) and the wire format of the specific API.
///
/// `Send + Sync` required for use in async contexts and across threads.
/// Async methods return boxed futures to preserve `dyn LlmProvider` compatibility.
pub trait LlmProvider: Send + Sync {
    // kanon:ignore RUST/pub-visibility
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

    /// Where this provider runs, for data-sovereignty gating (#3404, #3413).
    ///
    /// The recall pipeline filters facts whose `FactSensitivity` exceeds the
    /// provider's trust boundary before the system prompt is handed off to
    /// the provider. Defaults to [`DeploymentTarget::Cloud`] — the safe
    /// assumption, so operators cannot accidentally leak `Internal` or
    /// `Confidential` facts by registering a new provider without
    /// classifying it.
    fn deployment_target(&self) -> DeploymentTarget {
        DeploymentTarget::Cloud
    }

    /// Whether this provider supports streaming completions.
    fn supports_streaming(&self) -> bool {
        false
    }

    /// Send a streaming completion request, emitting [`StreamEvent`]s incrementally.
    ///
    /// The default implementation ignores `on_event` and delegates to `complete()`.
    /// Providers that support streaming should override both this method and
    /// `supports_streaming()`.
    ///
    /// # Errors
    /// Same as `complete`, plus mid-stream transport errors when overridden.
    fn complete_streaming<'a>(
        &'a self,
        request: &'a CompletionRequest,
        _on_event: &'a mut (dyn FnMut(StreamEvent) + Send),
    ) -> Pin<Box<dyn Future<Output = Result<CompletionResponse>> + Send + 'a>> {
        self.complete(request)
    }
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

/// Controls whether Anthropic prompt-cache markers (`cache_control`) are
/// emitted on outgoing requests.
///
/// Anthropic's prompt cache stores marked content on their servers for up
/// to 5 minutes (`Ephemeral`) or 1 hour (`Extended`) so that repeated
/// requests can reuse it. Disabling the cache keeps the operator system
/// prompt, tool definitions, and conversation history off Anthropic's
/// caching infrastructure at the cost of higher per-turn input token spend.
///
/// Sovereignty default: [`PromptCacheMode::Disabled`]. Operators who
/// accept the tradeoff may opt in via `aletheia.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum PromptCacheMode {
    /// No `cache_control` markers emitted. Operator content never enters
    /// Anthropic's prompt cache. Default for sovereignty-first deployments.
    #[default]
    Disabled,
    /// Standard 5-minute ephemeral cache. `cache_control: {"type": "ephemeral"}`
    /// on system prompt, tools, and recent conversation turns.
    Ephemeral,
    /// Extended 1-hour cache. Currently behaves like [`Ephemeral`](Self::Ephemeral)
    /// since the wire format for extended TTL is provider-specific and not
    /// yet wired through. Reserved for future use.
    Extended,
}

/// Where a provider's inference runs, for data-sovereignty gating.
///
/// Facts classified with a `FactSensitivity` strictly greater than the
/// provider's deployment target are filtered out during recall so they
/// never leave the boundary the operator has chosen (#3404, #3413).
///
/// | Variant | Meaning | Accepts |
/// |---------|---------|---------|
/// | `Cloud` | External API (`Anthropic`, `OpenAI`, etc.) | `Public` only |
/// | `LocalHosted` | Self-hosted but network-accessible (`llama.cpp`, `Ollama`) | `Public`, `Internal` |
/// | `Embedded` | In-process (`candle`, static model) | all sensitivities |
///
/// The ordering `Cloud < LocalHosted < Embedded` mirrors the sensitivity
/// ordering so admission reduces to `sensitivity <= target`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    Default,
)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DeploymentTarget {
    /// External cloud provider; receives only `Public` facts.
    #[default]
    Cloud,
    /// Self-hosted or network-local provider; receives `Public` and `Internal`.
    LocalHosted,
    /// In-process provider; no facts leave the host.
    Embedded,
}

impl DeploymentTarget {
    /// Lowercase `snake_case` name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cloud => "cloud",
            Self::LocalHosted => "local_hosted",
            Self::Embedded => "embedded",
        }
    }
}

/// Configuration for provider initialization.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderConfig {
    /// Provider type: `anthropic`, `openai`, `ollama`.
    pub provider_type: String,
    /// API key or credential reference.
    pub api_key: Option<SecretString>,
    /// Base URL override (for proxies or self-hosted).
    pub base_url: Option<String>,
    /// Default model to use.
    pub default_model: Option<String>,
    /// Maximum retries on transient failures.
    pub max_retries: Option<u32>,
    /// Per-model pricing for cost metrics. Keyed by model name.
    #[serde(default)]
    pub pricing: HashMap<String, ModelPricing>,
    /// Enable CC request mimicry for OAuth credentials. Defaults to `true`
    /// when using `with_credential_provider` against the first-party API.
    /// Set to `false` to disable (e.g., when enforcement is lifted or
    /// using API keys).
    #[serde(default)]
    pub cc_mimicry: Option<bool>,
    /// Prompt cache policy. Defaults to [`PromptCacheMode::Disabled`] —
    /// no `cache_control` markers are emitted and operator content never
    /// enters Anthropic's cache infrastructure (#3410).
    #[serde(default)]
    pub prompt_cache_mode: PromptCacheMode,
    /// Where this provider runs, gating which `FactSensitivity` the recall
    /// pipeline is allowed to send to it (#3404, #3413). Defaults to
    /// [`DeploymentTarget::Cloud`] — the safe assumption that an
    /// unconfigured provider speaks to an external service.
    #[serde(default)]
    pub deployment_target: DeploymentTarget,
}

impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("provider_type", &self.provider_type)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("base_url", &self.base_url)
            .field("default_model", &self.default_model)
            .field("max_retries", &self.max_retries)
            .field("cc_mimicry", &self.cc_mimicry)
            .field("prompt_cache_mode", &self.prompt_cache_mode)
            .field("deployment_target", &self.deployment_target)
            .finish_non_exhaustive()
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        // NOTE: Built-in pricing for all first-party Anthropic models (USD per million tokens).
        // Operator configs are merged on top, so these act as sensible fallbacks.
        // Prices last verified against https://www.anthropic.com/pricing (2025-10-01).
        let pricing = HashMap::from([
            (
                "claude-opus-4-6".to_owned(),
                ModelPricing {
                    input_cost_per_mtok: 15.0,
                    output_cost_per_mtok: 75.0,
                },
            ),
            (
                "claude-opus-4-20250514".to_owned(),
                ModelPricing {
                    input_cost_per_mtok: 15.0,
                    output_cost_per_mtok: 75.0,
                },
            ),
            (
                "claude-sonnet-4-6".to_owned(),
                ModelPricing {
                    input_cost_per_mtok: 3.0,
                    output_cost_per_mtok: 15.0,
                },
            ),
            (
                "claude-sonnet-4-20250514".to_owned(),
                ModelPricing {
                    input_cost_per_mtok: 3.0,
                    output_cost_per_mtok: 15.0,
                },
            ),
            (
                "claude-haiku-4-5".to_owned(),
                ModelPricing {
                    input_cost_per_mtok: 0.8,
                    output_cost_per_mtok: 4.0,
                },
            ),
            (
                "claude-haiku-4-5-20251001".to_owned(),
                ModelPricing {
                    input_cost_per_mtok: 0.8,
                    output_cost_per_mtok: 4.0,
                },
            ),
        ]);
        Self {
            provider_type: "anthropic".to_owned(),
            api_key: None,
            base_url: None,
            default_model: Some("claude-opus-4-20250514".to_owned()),
            max_retries: Some(3),
            pricing,
            cc_mimicry: None,
            prompt_cache_mode: PromptCacheMode::Disabled,
            // WHY (#3404, #3413): Anthropic is a cloud provider — only
            // `Public`-classified facts may be sent. Operators running a
            // self-hosted proxy or embedded model MUST override this in
            // `aletheia.toml` so the recall filter lets `Internal` /
            // `Confidential` facts through to the non-cloud boundary.
            deployment_target: DeploymentTarget::Cloud,
        }
    }
}

struct ProviderEntry {
    provider: Box<dyn LlmProvider>,
    health: ProviderHealthTracker,
}

/// Provider registry: maps model IDs to providers with health tracking.
#[derive(Default)]
pub struct ProviderRegistry {
    // kanon:ignore RUST/pub-visibility
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
        // kanon:ignore RUST/pub-visibility
        Self::default()
    }

    /// Register a provider with default health config.
    pub fn register(&mut self, provider: Box<dyn LlmProvider>) {
        // kanon:ignore RUST/pub-visibility
        self.register_with_config(provider, HealthConfig::default());
    }

    /// Register a provider with custom health thresholds.
    pub fn register_with_config(&mut self, provider: Box<dyn LlmProvider>, config: HealthConfig) {
        // kanon:ignore RUST/pub-visibility
        self.providers.push(ProviderEntry {
            provider,
            health: ProviderHealthTracker::new(config),
        });
    }

    /// Find a provider that supports the given model.
    ///
    /// # Complexity
    ///
    /// O(p) where p is the number of registered providers.
    #[must_use]
    pub fn find_provider(&self, model: &str) -> Option<&dyn LlmProvider> {
        // kanon:ignore RUST/pub-visibility
        for entry in &self.providers {
            let matches = entry.provider.supports_model(model);
            tracing::debug!(
                provider = entry.provider.name(),
                model,
                matches,
                "provider selection check"
            );
            if matches {
                return Some(entry.provider.as_ref());
            }
        }
        None
    }

    /// List all registered providers.
    ///
    /// # Complexity
    ///
    /// O(p) where p is the number of registered providers.
    #[must_use]
    pub fn providers(&self) -> Vec<&dyn LlmProvider> {
        // kanon:ignore RUST/pub-visibility
        self.providers.iter().map(|e| e.provider.as_ref()).collect()
    }

    /// Query health of a provider by name.
    ///
    /// # Complexity
    ///
    /// O(p) where p is the number of registered providers.
    #[must_use]
    pub fn provider_health(&self, name: &str) -> Option<ProviderHealth> {
        // kanon:ignore RUST/pub-visibility
        self.providers
            .iter()
            .find(|e| e.provider.name() == name)
            .map(|e| e.health.health())
    }

    /// Record a successful request for the named provider.
    pub fn record_success(&self, name: &str) {
        // kanon:ignore RUST/pub-visibility
        if let Some(entry) = self.providers.iter().find(|e| e.provider.name() == name) {
            entry.health.record_success();
        }
    }

    /// Find a streaming-capable provider for the given model.
    ///
    /// Returns `Some` if the provider supports streaming.
    #[must_use]
    pub fn find_streaming_provider(&self, model: &str) -> Option<&dyn LlmProvider> {
        // kanon:ignore RUST/pub-visibility
        self.find_provider(model).filter(|p| p.supports_streaming())
    }

    /// Record a failed request for the named provider.
    pub fn record_error(&self, name: &str, error: &error::Error) {
        // kanon:ignore RUST/pub-visibility
        if let Some(entry) = self.providers.iter().find(|e| e.provider.name() == name) {
            entry.health.record_error(error);
        }
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: map key is asserted present by contains_key check above"
)]
mod tests {
    use super::*;
    use crate::test_utils::MockProvider;
    use crate::types::*;

    #[tokio::test]
    async fn mock_provider_completes() {
        let provider =
            MockProvider::new("mock response").models(&["mock-model-v1", "mock-model-v2"]);
        let request = CompletionRequest {
            model: "mock-model-v1".to_owned(),
            system: None,
            messages: vec![Message {
                role: Role::User,
                content: Content::Text("hello".to_owned()),
                cache_breakpoint: false,
            }],
            max_tokens: 1024,
            tools: vec![],
            temperature: None,
            thinking: None,
            stop_sequences: vec![],
            ..Default::default()
        };

        let response = provider.complete(&request).await.unwrap();
        assert_eq!(response.id, "msg_mock");
        assert_eq!(response.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn supports_model_check() {
        let provider =
            MockProvider::new("mock response").models(&["mock-model-v1", "mock-model-v2"]);
        assert!(provider.supports_model("mock-model-v1"));
        assert!(provider.supports_model("mock-model-v2"));
        assert!(!provider.supports_model("nonexistent"));
    }

    #[test]
    fn registry_find_provider() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(
            MockProvider::new("mock response").models(&["mock-model-v1"]),
        ));

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
    fn provider_config_deployment_target_defaults_to_cloud() {
        // WHY (#3404, #3413): the safe default — any unconfigured provider
        // is treated as a cloud target so the sovereignty filter only
        // admits `Public` facts until the operator explicitly opts in to a
        // lower-trust boundary.
        let config = ProviderConfig::default();
        assert_eq!(
            config.deployment_target,
            DeploymentTarget::Cloud,
            "default ProviderConfig must bind deployment_target = Cloud"
        );
    }

    #[test]
    fn deployment_target_ordering() {
        assert!(DeploymentTarget::Cloud < DeploymentTarget::LocalHosted);
        assert!(DeploymentTarget::LocalHosted < DeploymentTarget::Embedded);
    }

    #[test]
    fn llm_provider_default_deployment_target_is_cloud() {
        let provider = MockProvider::new("x");
        assert_eq!(provider.deployment_target(), DeploymentTarget::Cloud);
    }

    #[test]
    fn provider_config_defaults() {
        let config = ProviderConfig::default();
        assert_eq!(config.provider_type, "anthropic");
        assert_eq!(
            config.default_model.as_deref(),
            Some("claude-opus-4-20250514")
        );
        // WHY: Default pricing must cover the models used by background tasks.
        assert!(
            config.pricing.contains_key("claude-haiku-4-5-20251001"),
            "missing default pricing for claude-haiku-4-5-20251001"
        );
        assert!(
            config.pricing.contains_key("claude-sonnet-4-20250514"),
            "missing default pricing for claude-sonnet-4-20250514"
        );
        let haiku = &config.pricing["claude-haiku-4-5-20251001"];
        assert!(
            (haiku.input_cost_per_mtok - 0.8).abs() < f64::EPSILON,
            "unexpected haiku input price"
        );
        assert!(
            (haiku.output_cost_per_mtok - 4.0).abs() < f64::EPSILON,
            "unexpected haiku output price"
        );
    }

    #[test]
    fn mock_provider_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockProvider>();
    }

    #[test]
    fn registry_health_starts_up() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(MockProvider::new("mock response")));

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
        registry.register(Box::new(MockProvider::new("mock response")));

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
        registry.register(Box::new(MockProvider::new("mock response")));

        let err: crate::error::Error = crate::error::ApiRequestSnafu { message: "timeout" }.build();
        registry.record_error("mock", &err);
        registry.record_success("mock");

        assert_eq!(registry.provider_health("mock"), Some(ProviderHealth::Up));
    }

    #[test]
    fn find_streaming_provider_returns_none_for_mock() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(MockProvider::new("mock response")));
        assert!(registry.find_streaming_provider("mock-model-v1").is_none());
    }

    #[test]
    fn registry_record_unknown_provider_does_not_mutate_known_or_insert_unknown() {
        let mut registry = ProviderRegistry::new();
        registry.register(Box::new(MockProvider::new("mock response")));
        let known_health_before = registry.provider_health("mock");
        let known_provider_count_before = registry
            .providers
            .iter()
            .filter(|entry| entry.provider.name() == "mock")
            .count();
        let total_provider_count_before = registry.providers.len();

        registry.record_success("nonexistent");
        let err: crate::error::Error = crate::error::ApiRequestSnafu { message: "timeout" }.build();
        registry.record_error("nonexistent", &err);

        assert_eq!(
            registry.provider_health("mock"),
            known_health_before,
            "unknown-provider records must not affect known-provider health"
        );
        assert_eq!(
            registry
                .providers
                .iter()
                .filter(|entry| entry.provider.name() == "mock")
                .count(),
            known_provider_count_before,
            "unknown-provider records must not duplicate the known provider"
        );
        assert_eq!(
            registry.providers.len(),
            total_provider_count_before,
            "unknown-provider records must not create provider entries"
        );
        assert_eq!(
            registry.provider_health("nonexistent"),
            None,
            "unknown provider must remain absent from health lookup"
        );
    }
}
