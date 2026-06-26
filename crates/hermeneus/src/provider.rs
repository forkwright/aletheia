// kanon:ignore RUST/file-too-long — provider types, trait, registry, and tests colocated; splitting the test module would separate assertions from the implementations they cover
//! LLM provider trait: Anthropic-native with adapter support.
//!
//! Defines the interface all providers must implement. Types are modeled
//! on the Anthropic Messages API; other providers adapt to this surface.

use std::borrow::Cow;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use koina::secret::SecretString;

use crate::anthropic::StreamEvent;
use crate::error::{self, Result};
use crate::health::{HealthConfig, ProviderHealth, ProviderHealthTracker};
use crate::types::{CompletionRequest, CompletionResponse};

/// How precisely a provider matches a model ID.
///
/// Used by [`ProviderRegistry::find_provider`] to select the most-specific
/// provider when multiple providers claim support for the same model ID.
/// Higher-specificity matches always win over lower-specificity ones,
/// regardless of registration order.
///
/// | Variant  | Example                                      |
/// |----------|----------------------------------------------|
/// | `Exact`  | Provider's `supported_models()` contains the exact model ID |
/// | `Prefix` | Provider matches by a namespaced prefix (e.g. `cc/`, `codex/`) |
/// | `CatchAll` | Provider matches by a broad family pattern (e.g. any `claude-*`) |
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum MatchKind {
    // WHY: lower numeric value = lower specificity; `find_provider` keeps
    // the maximum. Ord derives compare numerically so CatchAll < Prefix < Exact.
    /// Broad family-pattern match; lowest specificity.
    CatchAll = 0,
    /// Namespaced-prefix match (e.g. `cc/`, `codex/`); medium specificity.
    Prefix = 1,
    /// Exact model-ID match; highest specificity.
    Exact = 2,
}

/// Routing intent for provider selection.
///
/// WHY: execution needs to distinguish "use any healthy provider for this model"
/// from "the operator explicitly named a provider". The two paths have different
/// failure semantics: model-only routing may try equivalent providers, while an
/// explicit provider id fails fast if that provider is unhealthy.
#[derive(Debug, Clone, Copy)]
pub enum ProviderRoute<'a> {
    /// Select any provider that claims the model, preferring the healthiest,
    /// most-specific match.
    ModelOnly,
    /// Use the provider with this exact instance name, regardless of specificity.
    Explicit(&'a str),
}

/// Provider selection failure.
#[derive(Debug, Clone)]
pub enum ProviderResolutionError {
    /// No registered provider supports the requested model (model-only routing),
    /// or the explicitly named provider does not exist.
    NoProvider {
        /// The model ID for which no provider was found.
        model: String,
    },
    /// The explicitly named provider exists but is not available.
    ProviderUnavailable {
        /// The registered name of the unavailable provider.
        name: String,
        /// The current health state of the unavailable provider.
        health: ProviderHealth,
    },
}

impl std::fmt::Display for ProviderResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoProvider { model } => write!(f, "no provider for model: {model}"),
            Self::ProviderUnavailable { name, health } => {
                write!(f, "provider '{name}' is currently unavailable: {health:?}")
            }
        }
    }
}

impl std::error::Error for ProviderResolutionError {}

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
    ///
    /// WHY (#5259): this method returns `&[&str]` and can only expose
    /// static/built-in model IDs without leaking owned config data. Providers
    /// with dynamic model lists should return an empty slice here and override
    /// [`Self::supports_model`] and [`Self::match_specificity`] to use their
    /// owned data. Use [`Self::supported_model_list`] for a leak-free
    /// diagnostic enumeration of all claimed models.
    fn supported_models(&self) -> &[&str];

    /// Diagnostic enumeration of every model ID this provider claims.
    ///
    /// WHY (#5259): returns owned/borrowed `Cow<'_, str>` items so config-owned
    /// model IDs can be enumerated without leaking them for the lifetime of the
    /// process. The default implementation converts [`Self::supported_models`].
    fn supported_model_list(&self) -> Vec<Cow<'_, str>> {
        self.supported_models()
            .iter()
            .map(|&model| Cow::Borrowed(model))
            .collect()
    }

    /// Check if a specific model is supported.
    fn supports_model(&self, model: &str) -> bool {
        self.supported_models().contains(&model)
    }

    /// Return this provider's match specificity for `model`, or `None` if not supported.
    ///
    /// The default implementation returns `Some(MatchKind::Exact)` when `model`
    /// appears in `supported_models()`, and `None` otherwise. Providers with
    /// broader matching logic (prefix patterns, family catch-alls) should
    /// override this to return the appropriate [`MatchKind`] so that
    /// [`ProviderRegistry::find_provider`] can prefer an explicitly-configured
    /// exact-model provider over a generic catch-all for the same model ID.
    fn match_specificity(&self, model: &str) -> Option<MatchKind> {
        if self.supported_models().contains(&model) {
            Some(MatchKind::Exact)
        } else {
            None
        }
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

    /// Signal the provider to release any background resources.
    ///
    /// Called during server shutdown so providers can cancel background tasks
    /// before the runtime begins draining in-flight requests. The default
    /// implementation is a no-op for stateless providers.
    fn shutdown(&self) {}
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
/// Anthropic's prompt cache stores marked content for up to 5 minutes.
/// The `Extended` variant is reserved for a future longer-TTL cache type
/// not yet supported by the Anthropic API and currently behaves like
/// [`Ephemeral`](Self::Ephemeral). Disabling the cache keeps the operator
/// system prompt, tool definitions, and conversation history off
/// Anthropic's caching infrastructure at the cost of higher per-turn input
/// token spend.
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
    /// Instance name for logs, health tracking, and registry diagnostics.
    /// `None` uses the implementation's static name (e.g. `"anthropic"`).
    /// Set when declaring multiple instances of one provider type so each
    /// is distinguishable (e.g. first-party Anthropic plus a compatible
    /// third-party endpoint).
    #[serde(default)]
    pub name: Option<String>,
    /// Model identifiers this instance claims for registry routing. Empty
    /// uses the implementation's built-in catalog. Set when the endpoint
    /// serves models outside that catalog (e.g. an Anthropic-protocol
    /// endpoint hosting non-Anthropic models).
    #[serde(default)]
    pub models: Vec<String>,
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
            .field("name", &self.name)
            .field("models", &self.models)
            .finish_non_exhaustive()
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        let pricing = koina::models::pricing_entries()
            .map(|(model, price)| {
                (
                    model.to_owned(),
                    ModelPricing {
                        input_cost_per_mtok: price.input_cost_per_mtok,
                        output_cost_per_mtok: price.output_cost_per_mtok,
                    },
                )
            })
            .collect();
        Self {
            provider_type: "anthropic".to_owned(),
            api_key: None,
            base_url: None,
            default_model: Some(crate::models::names::opus().to_owned()),
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
            name: None,
            models: Vec::new(),
        }
    }
}

/// Borrow an owned model list as a `Cow<'_, str>` vector.
///
/// WHY (#5259): helper for [`LlmProvider::supported_model_list`] so
/// config-owned model IDs are enumerated without leaking them for the
/// lifetime of the process.
pub(crate) fn owned_model_list(models: &[String]) -> Vec<Cow<'_, str>> {
    models.iter().map(|s| Cow::Borrowed(s.as_str())).collect()
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

    /// Signal every registered provider to shut down.
    ///
    /// Propagates the shutdown signal to each provider so background tasks
    /// (e.g. OAuth token refresh) are cancelled before the runtime drains
    /// in-flight requests.
    pub fn shutdown(&self) {
        for entry in &self.providers {
            entry.provider.shutdown();
        }
    }

    /// Find the best healthy provider for `model` using specificity-based selection.
    ///
    /// # Selection contract
    ///
    /// WHY: provider health is live state, not a static preference. When multiple
    /// providers claim the same model ID, a down provider must not block an
    /// equivalent healthy alternative. This method first determines the highest
    /// specificity tier claimed by any matching provider, then selects the first
    /// healthy provider within that tier (registration order breaks ties).
    /// Lower-specificity providers are not used as fallbacks because they are
    /// not equivalent matches.
    ///
    /// Providers that match at the target specificity but are unhealthy are logged
    /// so operators can see which providers were skipped and why.
    ///
    /// # Complexity
    ///
    /// O(p) where p is the number of registered providers.
    #[must_use]
    pub fn find_provider(&self, model: &str) -> Option<&dyn LlmProvider> {
        // kanon:ignore RUST/pub-visibility
        self.resolve_provider(model, ProviderRoute::ModelOnly).ok()
    }

    /// Resolve a provider for `model` according to `route`.
    ///
    /// Model-only routing is health-aware and specificity-ordered. An explicit
    /// provider id respects operator intent and reports the provider's health
    /// directly when it is unavailable.
    pub fn resolve_provider<'a>(
        &'a self,
        model: &str,
        route: ProviderRoute<'_>,
    ) -> std::result::Result<&'a dyn LlmProvider, ProviderResolutionError> {
        // kanon:ignore RUST/pub-visibility
        match route {
            ProviderRoute::ModelOnly => self.resolve_model_only(model),
            ProviderRoute::Explicit(name) => self.resolve_explicit_provider(model, name),
        }
    }

    /// Whether a health snapshot represents a provider that may receive traffic.
    ///
    /// WHY: routing uses the non-mutating [`ProviderHealthTracker::health`]
    /// snapshot so that merely querying the registry does not elect recovery
    /// probes or alter the state of providers that are not selected.
    fn is_available(health: &ProviderHealth) -> bool {
        matches!(health, ProviderHealth::Up | ProviderHealth::Degraded { .. })
    }

    fn resolve_model_only<'a>(
        &'a self,
        model: &str,
    ) -> std::result::Result<&'a dyn LlmProvider, ProviderResolutionError> {
        // WHY: specificity is an intentful contract. Determine the best
        // specificity claimed by any matching provider first, then only
        // consider healthy providers at that tier. Lower-specificity providers
        // are not used as fallbacks because they are not equivalent matches.
        let mut target_specificity: Option<MatchKind> = None;
        for entry in &self.providers {
            if let Some(kind) = entry.provider.match_specificity(model)
                && target_specificity.as_ref().is_none_or(|prev| kind > *prev)
            {
                target_specificity = Some(kind);
            }
        }

        let Some(target_kind) = target_specificity else {
            tracing::debug!(model, "no provider claims model");
            return Err(ProviderResolutionError::NoProvider {
                model: model.to_owned(),
            });
        };

        let mut skipped: Vec<(String, ProviderHealth)> = Vec::new();

        for entry in &self.providers {
            if entry.provider.match_specificity(model) != Some(target_kind) {
                continue;
            }

            let health = entry.health.health();
            if Self::is_available(&health) {
                let skipped_summary: Vec<String> = skipped
                    .iter()
                    .map(|(name, health)| format!("{name} ({target_kind:?}: {health:?})"))
                    .collect();

                tracing::info!(
                    provider = entry.provider.name(),
                    model,
                    specificity = ?target_kind,
                    skipped = ?skipped_summary,
                    "provider selected"
                );

                return Ok(entry.provider.as_ref());
            }

            tracing::debug!(
                provider = entry.provider.name(),
                model,
                specificity = ?target_kind,
                ?health,
                "provider skipped: unhealthy"
            );
            skipped.push((entry.provider.name().to_owned(), health));
        }

        tracing::warn!(
            model,
            specificity = ?target_kind,
            skipped = ?skipped,
            "no healthy provider at target specificity"
        );

        if let Some((name, health)) = skipped.into_iter().next() {
            Err(ProviderResolutionError::ProviderUnavailable { name, health })
        } else {
            Err(ProviderResolutionError::NoProvider {
                model: model.to_owned(),
            })
        }
    }

    fn resolve_explicit_provider<'a>(
        &'a self,
        model: &str,
        name: &str,
    ) -> std::result::Result<&'a dyn LlmProvider, ProviderResolutionError> {
        let entry = self
            .providers
            .iter()
            .find(|e| e.provider.name() == name)
            .ok_or_else(|| ProviderResolutionError::NoProvider {
                model: model.to_owned(),
            })?;

        let health = entry.health.health();
        if !Self::is_available(&health) {
            tracing::info!(
                provider = name,
                model,
                ?health,
                "explicit provider unavailable"
            );
            return Err(ProviderResolutionError::ProviderUnavailable {
                name: name.to_owned(),
                health,
            });
        }

        tracing::info!(provider = name, model, "explicit provider selected");
        Ok(entry.provider.as_ref())
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

    /// Check whether the named provider may receive traffic.
    ///
    /// This is the authoritative gate: it performs cooldown recovery and
    /// single-flight probe election, so registry-level routing and provider-local
    /// execution see the same availability decision.
    ///
    /// Returns `None` if no provider with `name` is registered.
    /// Returns `Some(Ok(()))` if the provider is `Up`, `Degraded`, or has been
    /// elected as the single recovery probe.
    /// Returns `Some(Err(health))` if the provider is `Down` (before cooldown),
    /// `Down(AuthFailure)`, or a probe is already in flight.
    ///
    /// # Complexity
    ///
    /// O(p) where p is the number of registered providers.
    #[must_use]
    pub fn check_available(&self, name: &str) -> Option<std::result::Result<(), ProviderHealth>> {
        // kanon:ignore RUST/pub-visibility
        self.providers
            .iter()
            .find(|e| e.provider.name() == name)
            .map(|e| e.health.check_available())
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
#[path = "provider_tests.rs"]
mod tests;
