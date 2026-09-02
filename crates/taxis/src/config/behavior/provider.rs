//! Hermeneus provider behavior configuration.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Hermeneus provider timeout, concurrency, and complexity routing controls.
///
/// All defaults match the current public defaults in the `hermeneus` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[serde(default)]
pub struct ProviderBehaviorConfig {
    /// Timeout in seconds for non-streaming LLM requests.
    pub non_streaming_timeout_secs: u64,
    /// Default retry delay from SSE stream retry field in milliseconds.
    pub sse_default_retry_ms: u64,
    /// EWMA smoothing factor for adaptive concurrency limiter.
    pub concurrency_ewma_alpha: f64,
    /// Latency threshold in seconds above which concurrency limit is reduced.
    pub concurrency_latency_threshold_secs: f64,
    /// Whether per-turn complexity scoring may select a tier model.
    pub complexity_routing_enabled: bool,
    /// Complexity score at or below which Haiku-class model is selected when routing is enabled.
    pub complexity_low_threshold: u32,
    /// Complexity score at or above which Opus-class model is selected when routing is enabled.
    pub complexity_high_threshold: u32,
}

impl Default for ProviderBehaviorConfig {
    fn default() -> Self {
        let complexity = hermeneus::complexity::ComplexityConfig::default();
        Self {
            non_streaming_timeout_secs: hermeneus::anthropic::NON_STREAMING_TIMEOUT.as_secs(),
            sse_default_retry_ms: hermeneus::anthropic::SSE_DEFAULT_RETRY_MS,
            concurrency_ewma_alpha: hermeneus::concurrency::DEFAULT_EWMA_ALPHA,
            concurrency_latency_threshold_secs:
                hermeneus::concurrency::DEFAULT_LATENCY_THRESHOLD_SECS,
            complexity_routing_enabled: complexity.enabled,
            complexity_low_threshold: complexity.low_threshold,
            complexity_high_threshold: complexity.high_threshold,
        }
    }
}

/// Anthropic-specific sovereignty and privacy settings.
///
/// Defaults are sovereignty-first: nothing is cached on Anthropic servers
/// unless the operator explicitly opts in.
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
/// The runtime wiring in `crates/aletheia` converts this taxis-side policy to
/// and from `hermeneus::provider::PromptCacheMode`.
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

/// Launch-contract hard cap on concurrently running requests for a
/// localhosted provider (#7152): one running request.
pub const LOCAL_ADMISSION_MAX_RUNNING: u32 = 1;

/// Launch-contract bound on parked waiters for a localhosted provider
/// (#7152): at most two bounded waiters; excess work is refused with typed
/// retry guidance.
pub const LOCAL_ADMISSION_MAX_WAITING: u32 = 2;

/// How a provider admits concurrent requests (#7152).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ProviderAdmissionMode {
    /// Hard cap: at most `maxRunning` running and `maxWaiting` waiting
    /// requests; excess work is refused with a typed saturation error
    /// carrying retry guidance.
    #[default]
    Fixed,
    /// Adaptive AIMD limiter (the historical default for cloud endpoints);
    /// waiters park unboundedly.
    Adaptive,
}

/// Per-provider admission bound (#7152).
///
/// Declared as `[providers.admission]` on a `[[providers]]` entry. When the
/// table is omitted entirely, [`LlmProviderConfig::effective_admission`]
/// derives the default from the deployment target: localhosted and embedded
/// providers get the hard launch cap (fixed, 1 running / 2 waiting), cloud
/// providers keep the adaptive limiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[serde(default)]
pub struct ProviderAdmissionConfig {
    /// Admission mode. Writing an admission table without a mode expresses
    /// intent to bound, so the in-table default is `fixed`.
    pub mode: ProviderAdmissionMode,
    /// Maximum concurrently running requests in `fixed` mode.
    pub max_running: u32,
    /// Maximum parked waiters in `fixed` mode before excess requests are
    /// refused with typed retry guidance.
    pub max_waiting: u32,
}

impl Default for ProviderAdmissionConfig {
    fn default() -> Self {
        Self {
            mode: ProviderAdmissionMode::Fixed,
            max_running: LOCAL_ADMISSION_MAX_RUNNING,
            max_waiting: LOCAL_ADMISSION_MAX_WAITING,
        }
    }
}

impl ProviderAdmissionConfig {
    /// The adaptive policy with the launch-cap fields left at their
    /// defaults (ignored in adaptive mode).
    #[must_use]
    pub fn adaptive() -> Self {
        Self {
            mode: ProviderAdmissionMode::Adaptive,
            ..Self::default()
        }
    }
}

/// Launch-contract token budget for the local provider path (#7152):
/// 32,768 context tokens.
pub const LOCAL_BUDGET_CONTEXT_TOKENS: u32 = 32_768;

/// Launch-contract token budget for the local provider path (#7152):
/// 4,096 output tokens.
pub const LOCAL_BUDGET_MAX_OUTPUT_TOKENS: u32 = 4_096;

/// Launch-contract token budget for the local provider path (#7152):
/// 8,192 bootstrap tokens.
pub const LOCAL_BUDGET_BOOTSTRAP_MAX_TOKENS: u32 = 8_192;

/// Per-provider token-budget clamp (#7152).
///
/// Declared as `[providers.budgets]` on a `[[providers]]` entry. Each field
/// is independently optional: an unset field leaves the agent-resolved
/// limit for that dimension unclamped. [`resolved::resolve_nous`](super::super::resolved::resolve_nous)
/// applies the clamp as `min(agent_limit, provider_budget)` against the
/// provider that ends up serving the agent's primary model — a declared
/// budget can only lower a resolved limit, never raise it above what the
/// agent itself was configured for.
///
/// Unlike [`ProviderAdmissionConfig`], budgets have no deployment-target-derived
/// default: an operator wiring the launch-contract local path sets
/// `[providers.budgets]` explicitly with `contextTokens = 32768`,
/// `maxOutputTokens = 4096`, `bootstrapMaxTokens = 8192` (the
/// [`LOCAL_BUDGET_CONTEXT_TOKENS`] / [`LOCAL_BUDGET_MAX_OUTPUT_TOKENS`] /
/// [`LOCAL_BUDGET_BOOTSTRAP_MAX_TOKENS`] constants); omitting the table
/// entirely leaves every agent limit unclamped for that provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
#[serde(default)]
pub struct ProviderBudgetsConfig {
    /// Clamp on the resolved input context window, in tokens.
    pub context_tokens: Option<u32>,
    /// Clamp on the resolved maximum output tokens per response.
    pub max_output_tokens: Option<u32>,
    /// Clamp on the resolved bootstrap content token budget.
    pub bootstrap_max_tokens: Option<u32>,
}

/// Which concrete provider implementation to instantiate at startup.
///
/// Matches on this in `crates/aletheia/src/runtime/setup.rs` to pick between
/// the Anthropic HTTP client, OpenAI-compatible HTTP client, or a subprocess
/// adapter.
// kanon:ignore RUST/no-debug-derive-on-public-types — ProviderKind is a classification enum with no secret fields; derived Debug leaks only non-secret metadata
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
    /// Codex CLI subprocess adapter (delegates to the `codex` CLI).
    /// Requires the `codex-provider` feature flag on hermeneus.
    #[serde(rename = "codex_oauth", alias = "codex-oauth")]
    CodexOauth,
    /// Kimi CLI subprocess adapter (delegates to the `kimi` CLI).
    /// Requires the `kimi-provider` feature flag on hermeneus.
    Kimi,
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
    /// subprocess adapters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Environment variable name holding the API key. Read at startup via
    /// `std::env::var`. Optional for loopback / embedded providers that do
    /// not require authentication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    /// `OpenAI` API family to use. If omitted, `providerType = "openai"`
    /// defaults to `responses`, while `openai-compatible` defaults to
    /// `chat-completions` for local/proxy compatibility. Ignored for
    /// subprocess adapters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_family: Option<OpenAiApiFamily>,
    /// Path to the CLI binary for subprocess providers. Omit to resolve from
    /// PATH and well-known user install directories.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binary: Option<PathBuf>,
    /// Working directory for subprocess providers. Relative paths are resolved
    /// against the instance root by the runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<PathBuf>,
    /// Wall-clock timeout for subprocess provider calls, in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// Where this provider's traffic terminates. Drives the
    /// factsensitivity filter (#3414) and air-gapped mode.
    #[serde(default)]
    pub deployment_target: DeploymentTarget,
    /// Model identifiers this provider advertises support for. Used by the
    /// provider registry for routing: the first provider in list order that
    /// claims the requested model wins.
    #[serde(default)]
    pub models: Vec<String>,
    /// Optional per-provider admission bound (#7152). When omitted,
    /// [`Self::effective_admission`] derives the default from
    /// [`deployment_target`](Self::deployment_target).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub admission: Option<ProviderAdmissionConfig>,
    /// Optional per-provider token-budget clamp (#7152). When omitted, this
    /// provider never clamps an agent's resolved token limits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budgets: Option<ProviderBudgetsConfig>,
}

impl LlmProviderConfig {
    /// Effective admission bound for this provider (#7152).
    ///
    /// An explicit `[providers.admission]` table wins. Otherwise the
    /// deployment target decides: localhosted and embedded providers get
    /// the hard launch cap (fixed, [`LOCAL_ADMISSION_MAX_RUNNING`] running /
    /// [`LOCAL_ADMISSION_MAX_WAITING`] waiting) so they do not inherit the
    /// adaptive concurrency defaults; cloud providers keep the adaptive
    /// limiter.
    #[must_use]
    pub fn effective_admission(&self) -> ProviderAdmissionConfig {
        self.admission
            .unwrap_or_else(|| match self.deployment_target {
                DeploymentTarget::LocalHosted | DeploymentTarget::Embedded => {
                    ProviderAdmissionConfig::default()
                }
                // NOTE: `#[non_exhaustive]` does not apply in the defining
                // crate, so this match is exhaustive here — a future
                // variant must pick its admission default explicitly.
                DeploymentTarget::Cloud => ProviderAdmissionConfig::adaptive(),
            })
    }
}

impl std::fmt::Debug for LlmProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmProviderConfig")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("base_url", &self.base_url)
            .field("api_key_env", &self.api_key_env)
            .field("api_family", &self.api_family)
            .field("binary", &self.binary)
            .field("workdir", &self.workdir)
            .field("timeout_secs", &self.timeout_secs)
            .field("deployment_target", &self.deployment_target)
            .field("models", &self.models)
            .field("admission", &self.admission)
            .field("budgets", &self.budgets)
            .finish()
    }
}
