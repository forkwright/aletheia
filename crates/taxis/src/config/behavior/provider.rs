//! Hermeneus provider behavior configuration.

use serde::{Deserialize, Serialize};

/// Hermeneus provider timeout, concurrency, and complexity routing thresholds.
///
/// All defaults match the current hardcoded constants in the `hermeneus` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ProviderBehaviorConfig {
    /// Timeout in seconds for non-streaming LLM requests. Default: 120.
    /// Mirrors `hermeneus::anthropic::client::NON_STREAMING_TIMEOUT`.
    pub non_streaming_timeout_secs: u64,
    /// Default retry delay from SSE stream retry field in milliseconds. Default: 1000.
    /// Mirrors `hermeneus::anthropic::error::SSE_DEFAULT_RETRY_MS`.
    pub sse_default_retry_ms: u64,
    /// EWMA smoothing factor for adaptive concurrency limiter. Default: 0.8.
    /// Mirrors `hermeneus::concurrency::DEFAULT_EWMA_ALPHA`.
    pub concurrency_ewma_alpha: f64,
    /// Latency threshold in seconds above which concurrency limit is reduced. Default: 30.0.
    /// Mirrors `hermeneus::concurrency::DEFAULT_LATENCY_THRESHOLD_SECS`.
    pub concurrency_latency_threshold_secs: f64,
    /// Complexity score below which Haiku-class model is selected. Default: 30.
    /// Mirrors `hermeneus::complexity::DEFAULT_LOW_THRESHOLD`.
    pub complexity_low_threshold: u32,
    /// Complexity score above which Opus-class model is selected. Default: 70.
    /// Mirrors `hermeneus::complexity::DEFAULT_HIGH_THRESHOLD`.
    pub complexity_high_threshold: u32,
}

impl Default for ProviderBehaviorConfig {
    fn default() -> Self {
        Self {
            non_streaming_timeout_secs: 120,
            sse_default_retry_ms: 1_000,
            concurrency_ewma_alpha: 0.8,
            concurrency_latency_threshold_secs: 30.0,
            complexity_low_threshold: 30,
            complexity_high_threshold: 70,
        }
    }
}

/// Anthropic-specific sovereignty and privacy settings.
///
/// Mirrors the operator-facing controls at the hermeneus (Anthropic client)
/// boundary. Defaults are sovereignty-first: nothing is cached on Anthropic
/// servers unless the operator explicitly opts in.
///
/// Issues: #3406, #3410, #3409.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AnthropicConfig {
    /// Prompt cache policy (#3410).
    ///
    /// Controls whether outgoing requests carry `cache_control` markers that
    /// let Anthropic store operator system prompts, tool definitions, and
    /// recent conversation turns on their side for reuse. `"disabled"` (the
    /// default) strips every marker so operator content never enters the
    /// Anthropic prompt cache; `"ephemeral"` opts in to the standard 5-minute
    /// cache; `"extended"` reserves the slot for the 1-hour cache wire format
    /// and currently behaves the same as `"ephemeral"`.
    ///
    /// Tradeoff: enabling caching lowers per-turn token spend at the cost of
    /// storing the operator's system prompt on Anthropic infrastructure for
    /// the cache lifetime.
    pub prompt_cache_mode: PromptCacheMode,
}

/// Prompt cache policy for the Anthropic provider.
///
/// Mirrors `hermeneus::provider::PromptCacheMode` so the taxis config layer
/// does not depend on hermeneus; the runtime wiring in `crates/aletheia`
/// converts between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum PromptCacheMode {
    /// No `cache_control` markers emitted — operator content never enters
    /// Anthropic's prompt cache. Sovereignty default.
    #[default]
    Disabled,
    /// Standard 5-minute ephemeral cache.
    Ephemeral,
    /// Extended 1-hour cache (reserved; behaves like `Ephemeral` until the
    /// wire format for extended TTL is plumbed through).
    Extended,
}
