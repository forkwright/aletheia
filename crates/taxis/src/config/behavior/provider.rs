//! Hermeneus provider behavior configuration.

use serde::{Deserialize, Serialize};

/// Hermeneus provider timeout, concurrency, and complexity routing thresholds.
///
/// All defaults match the current hardcoded constants in the `hermeneus` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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

/// Where a provider's traffic terminates — used to classify data sensitivity
/// and sovereignty posture for routing decisions (#3414, #3424).
///
/// The factsensitivity filter and air-gapped mode use this to decide whether
/// a given turn may be sent to a given provider: cloud endpoints receive only
/// the facts the operator has explicitly allowed to leave the machine, while
/// locally-hosted and embedded providers are trusted with everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum DeploymentTarget {
    /// Third-party cloud API (e.g., api.anthropic.com, api.openai.com).
    /// Facts marked sensitive are filtered before the request is sent.
    #[default]
    Cloud,
    /// Self-hosted endpoint reachable over the local network (e.g., a
    /// colocated llama.cpp server on the same subnet). Trusted with
    /// operator-sensitive content but not with personally-identifiable data.
    #[serde(alias = "local_hosted", alias = "local-hosted")]
    LocalHosted,
    /// Runs on the same machine as aletheia (loopback llama.cpp / ollama
    /// / vllm). Trusted with every fact the operator would trust to disk.
    Embedded,
}

/// Which concrete provider implementation to instantiate at startup.
///
/// Matches on this in `crates/aletheia/src/runtime/setup.rs` to pick between
/// the Anthropic HTTP client, OpenAI-compatible HTTP client, or the CC
/// subprocess adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ProviderKind {
    /// Anthropic Messages API native client.
    Anthropic,
    /// `OpenAI` Chat Completions API native endpoint.
    #[serde(rename = "openai", alias = "open-ai")]
    OpenAi,
    /// `OpenAI` Chat Completions-compatible HTTP client. Works with
    /// `OpenAI`, llama.cpp, ollama, vllm, and any other server exposing the
    /// same wire format.
    #[serde(alias = "openai-compatible")]
    OpenAiCompatible,
    /// Claude Code subprocess adapter (delegates to the `claude` CLI).
    /// Requires the `cc-provider` feature flag on hermeneus.
    ClaudeCode,
}

/// `OpenAI` HTTP API family for `OpenAI` and OpenAI-compatible providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum OpenAiApiFamily {
    /// `OpenAI` `/v1/chat/completions` and compatible local/proxy endpoints.
    ChatCompletions,
    /// `OpenAI` first-party `/v1/responses` endpoint.
    Responses,
}

/// Per-provider configuration entry. One of these is produced for every
/// `[[providers]]` table in `aletheia.toml`.
///
/// The full set of entries lives on `AletheiaConfig::providers`. At startup
/// `build_provider_registry` iterates the vector and dispatches on
/// [`kind`](Self::kind) to build the corresponding concrete provider.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct LlmProviderConfig {
    /// Operator-facing label for logs and diagnostics (e.g., `"local-qwen"`,
    /// `"anthropic-cloud"`). Must be unique across the provider list.
    pub name: String,
    /// Which concrete provider implementation to instantiate.
    #[serde(rename = "providerType")]
    pub kind: ProviderKind,
    /// HTTP base URL override. Required for OpenAI-compatible providers
    /// (e.g., `http://127.0.0.1:8088/v1` for local llama.cpp). Optional for
    /// Anthropic (defaults to `https://api.anthropic.com`). Ignored for
    /// the Claude Code subprocess adapter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Environment variable name holding the API key. Read at startup via
    /// `std::env::var`. Optional for loopback / embedded providers that do
    /// not require authentication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// `OpenAI` API family to use. If omitted, `providerType = "openai"`
    /// defaults to `responses`, while `openai-compatible` defaults to
    /// `chat-completions` for local/proxy compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_family: Option<OpenAiApiFamily>,
    /// Where this provider's traffic terminates. Drives the
    /// factsensitivity filter (#3414) and air-gapped mode.
    #[serde(default)]
    pub deployment_target: DeploymentTarget,
    /// Model identifiers this provider advertises support for. Used by the
    /// provider registry for routing: the first provider in list order that
    /// claims the requested model wins.
    #[serde(default)]
    pub models: Vec<String>,
}

impl std::fmt::Debug for LlmProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmProviderConfig")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("api_key_env", &self.api_key_env)
            .field("api_family", &self.api_family)
            .field("deployment_target", &self.deployment_target)
            .field("models", &self.models)
            .finish()
    }
}
