//! Configuration types for an Aletheia instance.

mod maintenance;
mod resolved;

pub use maintenance::{
    CircuitBreakerSettings, CredentialConfig, CronTaskEntry, CronTaskSettings,
    DbMonitoringSettings, DiskSpaceSettings, DriftDetectionSettings, LoggingSettings,
    MaintenanceSettings, McpConfig, McpRateLimitConfig, RedactionSettings, RetentionSettings,
    SandboxSettings, SqliteRecoverySettings, TraceRotationSettings, WatchdogSettings,
};
pub use resolved::{
    AgentCapabilities, ResolvedModelConfig, ResolvedNousConfig, TokenLimits, resolve_nous,
};

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use koina::secret::SecretString;

/// Root configuration for an Aletheia instance.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AletheiaConfig {
    /// Agent definitions and shared defaults.
    pub agents: AgentsConfig,
    /// HTTP gateway settings (port, bind address, auth, TLS, CORS).
    pub gateway: GatewayConfig,
    /// Messaging transport configuration (Signal, etc.).
    pub channels: ChannelsConfig,
    /// Routes mapping channel sources to nous agents.
    pub bindings: Vec<ChannelBinding>,
    /// Embedding provider configuration for the recall pipeline.
    pub embedding: EmbeddingSettings,
    /// External domain pack paths (directories containing pack.toml).
    pub packs: Vec<PathBuf>,
    /// Periodic maintenance task configuration (trace rotation, drift detection, etc.).
    pub maintenance: MaintenanceSettings,
    /// Per-model pricing for LLM cost metrics. Keyed by model name.
    pub pricing: HashMap<String, ModelPricing>,
    /// Sandbox configuration for tool execution.
    pub sandbox: SandboxSettings,
    /// Credential resolution configuration.
    pub credential: CredentialConfig,
    /// Structured file logging configuration.
    pub logging: LoggingSettings,
    /// MCP server configuration.
    pub mcp: McpConfig,
    /// Training data capture configuration.
    pub training: eidos::training::TrainingConfig,
    /// Deployment-tunable timeout thresholds.
    pub timeouts: TimeoutsConfig,
    /// Deployment-tunable capacity limits for tool output and context windows.
    pub capacity: CapacityConfig,
    /// Deployment-tunable LLM retry and backoff parameters.
    pub retry: RetrySettings,
    /// Nous actor/manager health, restart, GC, and loop-detection settings.
    pub nous_behavior: NousBehaviorConfig,
    /// Episteme conflict resolution, decay, and extraction parameters.
    pub knowledge: KnowledgeConfig,
    /// Hermeneus provider timeout, concurrency, and complexity routing thresholds.
    pub provider_behavior: ProviderBehaviorConfig,
    /// Pylon request size and idempotency limits.
    pub api_limits: ApiLimitsConfig,
    /// Daemon watchdog, prosoche, and runner output settings.
    pub daemon_behavior: DaemonBehaviorConfig,
    /// Organon tool size and timeout limits.
    pub tool_limits: ToolLimitsConfig,
    /// Agora messaging transport poll, buffer, and circuit-breaker settings.
    pub messaging: MessagingConfig,
}

/// Sandbox enforcement level for tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SandboxEnforcementMode {
    /// Reject tool calls that violate sandbox policy.
    Enforcing,
    /// Allow tool calls that violate sandbox policy but log a warning.
    Permissive,
}

/// Network egress policy for child processes spawned by the exec tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum EgressPolicy {
    /// Block all outbound network from child processes.
    Deny,
    /// No egress filtering; child processes have full network access.
    #[default]
    Allow,
    /// Permit only connections to listed destinations.
    Allowlist,
}

/// Agent autonomy level controlling default tool iteration limits and
/// execution permissions.
///
/// - `Unrestricted`: no practical limits on tool iterations (10 000 cap)
/// - `Standard`: balanced defaults (the current default after configuration)
/// - `Restricted`: conservative limits matching pre-expansion behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum AgencyLevel {
    /// No practical limits on tool iterations (10 000 cap).
    Unrestricted,
    /// Balanced defaults for typical agent use.
    #[default]
    Standard,
    /// Conservative limits matching pre-expansion behavior.
    Restricted,
}

/// Per-model pricing rates for cost estimation in metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    /// Cost per million input tokens (USD).
    pub input_cost_per_mtok: f64,
    /// Cost per million output tokens (USD).
    pub output_cost_per_mtok: f64,
}

/// Maps a channel source to a nous agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelBinding {
    /// Channel type (e.g., "signal").
    pub channel: String,
    /// Source pattern: phone number, group ID, or "*" for default.
    pub source: String,
    /// Nous ID to route to.
    pub nous_id: String,
    /// Session key pattern. Supports `{source}` and `{group}` placeholders.
    #[serde(default = "default_session_pattern")]
    pub session_key: String,
}

fn default_session_pattern() -> String {
    "{source}".to_owned()
}

/// Agent configuration: shared defaults and per-agent definitions.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AgentsConfig {
    /// Shared defaults applied to every agent unless overridden per-agent.
    pub defaults: AgentDefaults,
    /// Individual agent definitions; merged with `defaults` at resolution time.
    pub list: Vec<NousDefinition>,
}

/// Per-factor scoring weights for the recall pipeline.
///
/// Mirrors the weights in the nous recall stage but lives in taxis so operators
/// can tune them per-agent via TOML without creating a taxis → nous dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RecallWeights {
    /// Temporal decay weight (0.0--1.0).
    pub decay: f64,
    /// Content relevance weight (0.0--1.0).
    pub relevance: f64,
    /// Epistemic tier weight (0.0--1.0).
    pub epistemic_tier: f64,
    /// Knowledge-graph relationship proximity weight (0.0--1.0).
    pub relationship_proximity: f64,
    /// Access frequency weight (0.0--1.0).
    pub access_frequency: f64,
}

impl Default for RecallWeights {
    fn default() -> Self {
        Self {
            decay: 0.5,
            relevance: 0.5,
            epistemic_tier: 0.3,
            relationship_proximity: 0.0,
            access_frequency: 0.0,
        }
    }
}

/// Per-factor engine scoring weights for the mneme `RecallEngine`.
///
/// These multipliers determine how much each retrieval signal contributes to the
/// final relevance score. Weights need not sum to 1.0: the engine normalises
/// the weighted sum automatically. Defaults match the mneme engine's built-in
/// values so that omitting this section produces identical behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RecallEngineWeights {
    /// Cosine-similarity weight. Default: 0.35
    pub vector_similarity: f64,
    /// FSRS power-law temporal decay weight. Default: 0.20
    pub decay: f64,
    /// Nous-relevance weight (own memories rank higher). Default: 0.15
    pub relevance: f64,
    /// Epistemic-tier weight (verified > inferred > assumed). Default: 0.15
    pub epistemic_tier: f64,
    /// Knowledge-graph relationship proximity weight. Default: 0.10
    pub relationship_proximity: f64,
    /// Access-frequency weight. Default: 0.05
    pub access_frequency: f64,
}

impl Default for RecallEngineWeights {
    fn default() -> Self {
        // WHY: values match mneme::recall::RecallWeights defaults so no behavioural
        //      change occurs when an operator omits this section from the config.
        Self {
            vector_similarity: 0.35,
            decay: 0.20,
            relevance: 0.15,
            epistemic_tier: 0.15,
            relationship_proximity: 0.10,
            access_frequency: 0.05,
        }
    }
}

/// Recall pipeline settings for a nous agent.
///
/// Resolved from taxis config and forwarded to the recall stage via
/// `NousConfig::recall` at startup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RecallSettings {
    /// Whether semantic recall is enabled for this agent.
    pub enabled: bool,
    /// Maximum number of recalled facts to inject per turn.
    pub max_results: usize,
    /// Minimum relevance score (0.0--1.0) to include a recalled fact.
    pub min_score: f64,
    /// Maximum tokens to allocate for recalled knowledge.
    pub max_recall_tokens: u64,
    /// Enable iterative two-cycle retrieval with terminology discovery.
    pub iterative: bool,
    /// Maximum retrieval cycles when iterative mode is enabled.
    pub max_cycles: usize,
    /// Per-factor scoring weights (factor scores for non-vector signals).
    pub weights: RecallWeights,
    /// Per-factor engine scoring weights used by the mneme `RecallEngine`.
    ///
    /// Controls how much each retrieval signal contributes to the final
    /// weighted relevance score. Defaults match mneme's built-in values.
    pub engine_weights: RecallEngineWeights,
}

impl Default for RecallSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_results: 5,
            min_score: 0.3,
            max_recall_tokens: 2000,
            iterative: false,
            max_cycles: 2,
            weights: RecallWeights::default(),
            engine_weights: RecallEngineWeights::default(),
        }
    }
}

/// LLM model and generation defaults for agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AgentModelDefaults {
    /// Primary model and fallback chain.
    pub model: ModelSpec,
    /// Maximum input context window size in tokens.
    pub context_tokens: u32,
    /// Maximum tokens the model may generate per response.
    pub max_output_tokens: u32,
    /// Token budget for bootstrap (system prompt + persona) content.
    pub bootstrap_max_tokens: u32,
    /// Whether extended thinking is enabled by default.
    pub thinking_enabled: bool,
    /// Maximum tokens allocated to extended thinking when enabled.
    pub thinking_budget: u32,
    /// Characters per token for conservative token-budget estimation.
    pub chars_per_token: u32,
    /// Model used for prosoche heartbeat sessions.
    pub prosoche_model: String,
    /// Maximum size in bytes for a single tool result before truncation.
    pub max_tool_result_bytes: u32,
}

impl Default for AgentModelDefaults {
    fn default() -> Self {
        use koina::defaults as d;
        Self {
            model: ModelSpec::default(),
            context_tokens: d::CONTEXT_TOKENS,
            max_output_tokens: d::MAX_OUTPUT_TOKENS,
            bootstrap_max_tokens: d::BOOTSTRAP_MAX_TOKENS,
            thinking_enabled: false,
            thinking_budget: 10_000,
            chars_per_token: d::CHARS_PER_TOKEN,
            prosoche_model: "claude-haiku-4-5-20251001".to_owned(),
            max_tool_result_bytes: d::MAX_TOOL_RESULT_BYTES,
        }
    }
}

/// Default values applied to every agent unless overridden.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AgentDefaults {
    /// Model and generation settings.
    #[serde(flatten)]
    pub model_defaults: AgentModelDefaults,
    /// Agent autonomy level. Controls effective tool iteration limits when
    /// `max_tool_iterations` is not explicitly overridden per-agent.
    pub agency: AgencyLevel,
    /// Safety limit on consecutive tool use iterations per turn.
    pub max_tool_iterations: u32,
    /// Filesystem paths the agent is permitted to access.
    pub allowed_roots: Vec<String>,
    /// Prompt caching configuration.
    pub caching: CachingConfig,
    /// Recall pipeline settings applied to all agents unless overridden.
    pub recall: RecallSettings,
    /// Fraction of the context window reserved for conversation history.
    pub history_budget_ratio: f64,
    /// Default per-agent behavioral parameters (safety, hooks, distillation, etc.).
    pub behavior: AgentBehaviorDefaults,
}

impl Default for AgentDefaults {
    fn default() -> Self {
        use koina::defaults as d;
        Self {
            model_defaults: AgentModelDefaults::default(),
            agency: AgencyLevel::Standard,
            max_tool_iterations: d::MAX_TOOL_ITERATIONS,
            allowed_roots: Vec::new(),
            caching: CachingConfig::default(),
            recall: RecallSettings::default(),
            history_budget_ratio: d::HISTORY_BUDGET_RATIO,
            behavior: AgentBehaviorDefaults::default(),
        }
    }
}

/// Model specification with primary model and fallbacks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ModelSpec {
    /// Primary model identifier (e.g. `claude-sonnet-4-6`).
    pub primary: String,
    /// Ordered fallback models tried when the primary is unavailable.
    pub fallbacks: Vec<String>,
    /// How many times to retry the primary model before trying the next fallback.
    pub retries_before_fallback: u32,
}

impl Default for ModelSpec {
    fn default() -> Self {
        Self {
            primary: koina::defaults::DEFAULT_MODEL_SHORT.to_owned(),
            fallbacks: Vec::new(),
            retries_before_fallback: 2,
        }
    }
}

/// Prompt caching configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct CachingConfig {
    /// Whether prompt caching is enabled.
    pub enabled: bool,
    /// Caching strategy: `"auto"` or `"disabled"`.
    pub strategy: String,
}

impl Default for CachingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            strategy: "auto".to_owned(),
        }
    }
}

/// Definition of a single nous agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NousDefinition {
    /// Unique agent identifier (matches the `nous/{id}/` directory name).
    pub id: String,
    /// Human-readable display name.
    #[serde(default)]
    pub name: Option<String>,
    /// Model override; when `None`, inherits from `AgentDefaults`.
    #[serde(default)]
    pub model: Option<ModelSpec>,
    /// Filesystem path to the agent's workspace directory.
    pub workspace: String,
    /// Thinking override; when `None`, inherits from `AgentDefaults`.
    #[serde(default)]
    pub thinking_enabled: Option<bool>,
    /// Agency level override; when `None`, inherits from [`AgentDefaults::agency`].
    #[serde(default)]
    pub agency: Option<AgencyLevel>,
    /// Additional filesystem roots this agent may access (merged with defaults).
    #[serde(default)]
    pub allowed_roots: Vec<String>,
    /// Knowledge domains this agent specializes in (e.g. `"code"`, `"research"`).
    #[serde(default)]
    pub domains: Vec<String>,
    /// Whether this is the default agent for unrouted messages.
    #[serde(default)]
    pub default: bool,
    /// Recall pipeline override; when `None`, inherits from [`AgentDefaults::recall`].
    #[serde(default)]
    pub recall: Option<RecallSettings>,
    /// Per-agent behavioral override; when `None`, inherits from [`AgentDefaults::behavior`].
    #[serde(default)]
    pub behavior: Option<AgentBehaviorDefaults>,
}

/// HTTP gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct GatewayConfig {
    /// TCP port the gateway listens on.
    pub port: u16,
    /// Bind mode: `"localhost"` for loopback only, `"lan"` for all interfaces.
    pub bind: String,
    /// Authentication configuration.
    pub auth: GatewayAuthConfig,
    /// TLS termination settings.
    pub tls: TlsConfig,
    /// Cross-origin resource sharing policy.
    pub cors: CorsConfig,
    /// Request body size limit.
    pub body_limit: BodyLimitConfig,
    /// CSRF protection settings.
    pub csrf: CsrfConfig,
    /// Rate limiting settings.
    pub rate_limit: RateLimitConfig,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            port: 18789,
            bind: "localhost".to_owned(),
            auth: GatewayAuthConfig::default(),
            tls: TlsConfig::default(),
            cors: CorsConfig::default(),
            body_limit: BodyLimitConfig::default(),
            csrf: CsrfConfig::default(),
            rate_limit: RateLimitConfig::default(),
        }
    }
}

/// Gateway authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct GatewayAuthConfig {
    /// Auth mode: `"token"` (bearer token), `"none"` (disabled), `"jwt"` (explicit JWT).
    pub mode: String,
    /// Role assigned to anonymous requests when auth mode is `"none"`.
    /// Valid values: `"readonly"`, `"agent"`, `"operator"`, `"admin"`. Defaults to `"admin"`.
    pub none_role: String,
    /// JWT signing key. If `None`, falls back to `ALETHEIA_JWT_SECRET` env var.
    /// Startup fails when auth mode requires JWT and this is still the default placeholder.
    ///
    /// WHY: `SecretString` prevents accidental logging of the key value. Closes #1631.
    pub signing_key: Option<SecretString>,
}

impl Default for GatewayAuthConfig {
    fn default() -> Self {
        Self {
            mode: "token".to_owned(),
            none_role: "admin".to_owned(),
            signing_key: None,
        }
    }
}

/// TLS termination configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct TlsConfig {
    /// Whether TLS termination is active.
    pub enabled: bool,
    /// Path to the PEM-encoded certificate file.
    pub cert_path: Option<String>,
    /// Path to the PEM-encoded private key file.
    pub key_path: Option<String>,
}

/// CORS origin allowlist configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct CorsConfig {
    /// Allowed origins. Empty or `["*"]` means permissive (dev mode).
    pub allowed_origins: Vec<String>,
    /// Preflight cache duration in seconds.
    pub max_age_secs: u64,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: Vec::new(),
            max_age_secs: 3600,
        }
    }
}

/// Request body size limit configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct BodyLimitConfig {
    /// Maximum request body size in bytes.
    pub max_bytes: usize,
}

impl Default for BodyLimitConfig {
    fn default() -> Self {
        Self {
            max_bytes: 1_048_576,
        }
    }
}

/// CSRF protection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct CsrfConfig {
    /// Whether CSRF header checking is active.
    pub enabled: bool,
    /// Required header name (e.g. `x-requested-with`).
    pub header_name: String,
    /// Required header value (e.g. `aletheia`).
    pub header_value: String,
}

impl Default for CsrfConfig {
    fn default() -> Self {
        // WHY: CSRF is disabled by default so the API works out-of-the-box
        // without any client configuration. Operators who expose the gateway
        // to a browser should explicitly enable it.
        Self {
            enabled: false,
            header_name: "x-requested-with".to_owned(),
            header_value: "aletheia".to_owned(),
        }
    }
}

/// Rate limiting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RateLimitConfig {
    /// Whether rate limiting is active.
    pub enabled: bool,
    /// Maximum requests per minute per client IP (global rate limit).
    pub requests_per_minute: u32,
    /// Per-user rate limiting settings keyed by authenticated identity.
    pub per_user: PerUserRateLimitConfig,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            requests_per_minute: 60,
            per_user: PerUserRateLimitConfig::default(),
        }
    }
}

/// Per-user rate limiting configuration keyed by authenticated identity.
///
/// Applies token bucket rate limiting per user with different limits for
/// general, LLM, and tool execution endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct PerUserRateLimitConfig {
    /// Whether per-user rate limiting is active.
    pub enabled: bool,
    /// Default requests per minute for general API endpoints.
    pub default_rpm: u32,
    /// Burst allowance above the sustained rate for general endpoints.
    pub default_burst: u32,
    /// Requests per minute for LLM/chat endpoints (more expensive).
    pub llm_rpm: u32,
    /// Burst allowance for LLM endpoints.
    pub llm_burst: u32,
    /// Requests per minute for tool execution endpoints.
    pub tool_rpm: u32,
    /// Burst allowance for tool execution endpoints.
    pub tool_burst: u32,
    /// Seconds after which an idle user's rate limit state is evicted.
    pub stale_after_secs: u64,
}

impl Default for PerUserRateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_rpm: 60,
            default_burst: 10,
            llm_rpm: 20,
            llm_burst: 5,
            tool_rpm: 30,
            tool_burst: 8,
            stale_after_secs: 600,
        }
    }
}

/// Embedding provider configuration for recall pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct EmbeddingSettings {
    /// Provider type: "mock", "candle".
    pub provider: String,
    /// Provider-specific model name.
    pub model: Option<String>,
    /// Output vector dimension (must match knowledge store HNSW index).
    pub dimension: usize,
}

impl Default for EmbeddingSettings {
    fn default() -> Self {
        Self {
            provider: "candle".to_owned(),
            model: None,
            dimension: 384,
        }
    }
}

/// Channel configuration (messaging transports).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ChannelsConfig {
    /// Signal messenger transport configuration.
    pub signal: SignalConfig,
}

/// Signal messenger channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct SignalConfig {
    /// Whether the Signal channel is active.
    pub enabled: bool,
    /// Named Signal accounts keyed by account label.
    pub accounts: HashMap<String, SignalAccountConfig>,
}

impl Default for SignalConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            accounts: HashMap::new(),
        }
    }
}

/// Configuration for a single Signal account.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct SignalAccountConfig {
    /// Whether this account is active.
    pub enabled: bool,
    /// Hostname for the signal-cli JSON-RPC HTTP interface.
    pub http_host: String,
    /// Port for the signal-cli JSON-RPC HTTP interface.
    pub http_port: u16,
    /// Whether to auto-start the receive loop for this account on server boot.
    pub auto_start: bool,
}

impl Default for SignalAccountConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            http_host: "localhost".to_owned(),
            http_port: 8080,
            auto_start: true,
        }
    }
}

/// Deployment-tunable timeout thresholds.
///
/// Controls wall-clock timeout budgets for LLM and provider calls.
/// Defaults match the hardcoded constants in `koina::defaults` so that
/// omitting this section from `aletheia.toml` produces identical behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct TimeoutsConfig {
    /// Maximum wall-clock seconds for a single LLM API call (Anthropic or CC provider).
    ///
    /// Requests exceeding this limit are cancelled and may trigger a retry.
    /// Valid range: 30–3600. Default: 300.
    pub llm_call_secs: u32,
}

impl Default for TimeoutsConfig {
    fn default() -> Self {
        Self {
            llm_call_secs: koina::defaults::TIMEOUT_SECONDS,
        }
    }
}

/// Deployment-tunable capacity limits for tool output and context windows.
///
/// Controls memory and token budgets that depend on the host's hardware and
/// the LLM provider's context limits. Defaults match `koina::defaults`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct CapacityConfig {
    /// Maximum bytes returned by a single tool call before the output is
    /// truncated with an indicator showing the original size.
    ///
    /// Applies to all built-in tools (filesystem, workspace, shell). Set to
    /// `0` to disable truncation. Valid range: 0–10 MiB. Default: 51200 (50 KiB).
    pub max_tool_output_bytes: usize,
    /// Context window token limit applied to Opus-class models when the
    /// global `contextTokens` is still at its default value (200k).
    ///
    /// Opus models support a 1M token context window; this automatic upgrade
    /// preserves that capability without requiring manual per-agent overrides.
    /// Set to the same value as `contextTokens` to disable the auto-upgrade.
    /// Valid range: 200000–2000000. Default: 1000000.
    pub opus_context_tokens: u32,
}

impl Default for CapacityConfig {
    fn default() -> Self {
        Self {
            max_tool_output_bytes: koina::defaults::MAX_OUTPUT_BYTES,
            opus_context_tokens: koina::defaults::OPUS_CONTEXT_TOKENS,
        }
    }
}

/// Deployment-tunable LLM retry and backoff parameters.
///
/// Controls how the Anthropic provider retries transient failures. Defaults
/// match the constants in `hermeneus::models` so that omitting this section
/// produces identical behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RetrySettings {
    /// Maximum number of retry attempts after an initial transient failure.
    ///
    /// The total number of LLM calls is `max_attempts + 1`. Set to `0` to
    /// disable retries. Valid range: 0–10. Default: 3.
    pub max_attempts: u32,
    /// Initial exponential backoff delay in milliseconds.
    ///
    /// Each successive retry doubles this delay until `backoff_max_ms` is
    /// reached. Valid range: 100–30000. Default: 1000.
    pub backoff_base_ms: u64,
    /// Maximum backoff delay cap in milliseconds.
    ///
    /// No retry will wait longer than this value regardless of how many
    /// attempts have failed. Valid range: `backoff_base_ms`–300000. Default: 30000.
    pub backoff_max_ms: u64,
}

impl Default for RetrySettings {
    fn default() -> Self {
        // WHY: values mirror hermeneus::models::{DEFAULT_MAX_RETRIES, BACKOFF_BASE_MS,
        // BACKOFF_MAX_MS} so that omitting [retry] from aletheia.toml produces
        // identical behaviour to the pre-parameterization defaults.
        Self {
            max_attempts: 3,
            backoff_base_ms: 1_000,
            backoff_max_ms: 30_000,
        }
    }
}

/// Nous actor/manager health, restart, GC, and loop-detection thresholds.
///
/// All defaults match the current hardcoded constants in the `nous` crate so
/// that omitting this section from `aletheia.toml` produces identical behaviour.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct NousBehaviorConfig {
    /// Panics within the window that trigger degraded mode. Default: 5.
    /// Mirrors `nous::actor::DEGRADED_PANIC_THRESHOLD`.
    pub degraded_panic_threshold: u32,
    /// Window in seconds for counting panics toward degraded threshold. Default: 600.
    /// Mirrors `nous::actor::DEGRADED_WINDOW`.
    pub degraded_window_secs: u64,
    /// Actor inbox receive timeout in seconds before a warning is logged. Default: 30.
    /// Mirrors `nous::actor::INBOX_RECV_TIMEOUT`.
    pub inbox_recv_timeout_secs: u64,
    /// Consecutive receive timeouts before a warning log is emitted. Default: 3.
    /// Mirrors `nous::actor::CONSECUTIVE_TIMEOUT_WARN_THRESHOLD`.
    pub consecutive_timeout_warn_threshold: u32,
    /// Actor inbox channel capacity. Default: 32.
    pub inbox_capacity: usize,
    /// Maximum number of concurrently spawned tasks per agent. Default: 8.
    pub max_spawned_tasks: usize,
    /// Maximum number of concurrent sessions across all agents. Default: 1000.
    pub max_sessions: usize,
    /// Completed-task garbage collection interval in seconds. Default: 300.
    /// Mirrors `nous::tasks::gc::DEFAULT_GC_INTERVAL`.
    pub gc_interval_secs: u64,
    /// Consecutive failed pings before marking an agent dead. Default: 3.
    /// Mirrors `nous::manager::DEAD_THRESHOLD`.
    pub manager_dead_threshold: u32,
    /// Cap on exponential restart backoff in seconds. Default: 300.
    /// Mirrors `nous::manager::MAX_RESTART_BACKOFF`.
    pub manager_max_restart_backoff_secs: u64,
    /// Drain timeout in seconds before forcing an agent restart. Default: 30.
    /// Mirrors `nous::manager::RESTART_DRAIN_TIMEOUT`.
    pub manager_restart_drain_timeout_secs: u64,
    /// Window in seconds over which the failure count decays to zero. Default: 3600.
    /// Mirrors `nous::manager::RESTART_DECAY_WINDOW`.
    pub manager_restart_decay_window_secs: u64,
    /// Agent health poll interval in seconds. Default: 30.
    /// Mirrors `nous::manager::DEFAULT_HEALTH_INTERVAL`.
    pub manager_health_interval_secs: u64,
    /// Timeout in seconds for health-ping responses. Default: 5.
    /// Mirrors `nous::manager::DEFAULT_PING_TIMEOUT`.
    pub manager_ping_timeout_secs: u64,
    /// Number of recent tool calls scanned for loop detection. Default: 50.
    /// Mirrors `nous::pipeline::DEFAULT_LOOP_WINDOW`.
    pub loop_detection_window: usize,
    /// Maximum sequence length examined for repeating cycles. Default: 10.
    /// Mirrors `nous::pipeline::CYCLE_DETECTION_MAX_LEN`.
    pub cycle_detection_max_len: usize,
    /// Events accumulated before self-audit runs. Default: 50.
    /// Mirrors `nous::self_audit::DEFAULT_EVENT_THRESHOLD`.
    pub self_audit_event_threshold: u32,
}

impl Default for NousBehaviorConfig {
    fn default() -> Self {
        Self {
            degraded_panic_threshold: 5,
            degraded_window_secs: 600,
            inbox_recv_timeout_secs: 30,
            consecutive_timeout_warn_threshold: 3,
            inbox_capacity: 32,
            max_spawned_tasks: 8,
            max_sessions: 1_000,
            gc_interval_secs: 300,
            manager_dead_threshold: 3,
            manager_max_restart_backoff_secs: 300,
            manager_restart_drain_timeout_secs: 30,
            manager_restart_decay_window_secs: 3_600,
            manager_health_interval_secs: 30,
            manager_ping_timeout_secs: 5,
            loop_detection_window: 50,
            cycle_detection_max_len: 10,
            self_audit_event_threshold: 50,
        }
    }
}

/// Episteme knowledge conflict resolution, decay, and extraction parameters.
///
/// All defaults match the current hardcoded constants in the `episteme` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct KnowledgeConfig {
    /// Maximum LLM calls per fact during conflict resolution. Default: 3.
    /// Mirrors `episteme::conflict::MAX_LLM_CALLS_PER_FACT`.
    pub conflict_max_llm_calls_per_fact: usize,
    /// Similarity threshold above which intra-batch candidates are merged. Default: 0.95.
    /// Mirrors `episteme::conflict::INTRA_BATCH_DEDUP_THRESHOLD`.
    pub conflict_intra_batch_dedup_threshold: f64,
    /// Maximum vector distance for a fact to be a conflict candidate. Default: 0.28.
    /// Mirrors `episteme::conflict::CANDIDATE_DISTANCE_THRESHOLD`.
    pub conflict_candidate_distance_threshold: f64,
    /// Maximum conflict candidates evaluated per fact. Default: 5.
    /// Mirrors `episteme::conflict::MAX_CANDIDATES`.
    pub conflict_max_candidates: usize,
    /// Confidence boost per reinforcement event. Default: 0.02.
    /// Mirrors `episteme::decay::REINFORCEMENT_BOOST`.
    pub decay_reinforcement_boost: f64,
    /// Maximum cumulative reinforcement bonus. Default: 1.0.
    /// Mirrors `episteme::decay::MAX_REINFORCEMENT_BONUS`.
    pub decay_max_reinforcement_bonus: f64,
    /// Confidence bonus per additional corroborating agent. Default: 0.15.
    /// Mirrors `episteme::decay::CROSS_AGENT_BONUS_PER_AGENT`.
    pub decay_cross_agent_bonus_per_agent: f64,
    /// Cap on total cross-agent multiplier. Default: 1.75.
    /// Mirrors `episteme::decay::MAX_CROSS_AGENT_MULTIPLIER`.
    pub decay_max_cross_agent_multiplier: f64,
    /// Minimum confidence for a fact to pass extraction filtering. Default: 0.3.
    pub extraction_confidence_threshold: f64,
    /// Minimum character length for an extracted fact. Default: 10.
    pub extraction_min_fact_length: usize,
    /// Maximum character length for an extracted fact. Default: 500.
    pub extraction_max_fact_length: usize,
    /// Minimum tool calls before operational instinct scoring fires. Default: 5.
    /// Mirrors `episteme::ops_facts::MIN_TOOL_CALLS`.
    pub instinct_min_tool_calls: u64,
}

impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            conflict_max_llm_calls_per_fact: 3,
            conflict_intra_batch_dedup_threshold: 0.95,
            conflict_candidate_distance_threshold: 0.28,
            conflict_max_candidates: 5,
            decay_reinforcement_boost: 0.02,
            decay_max_reinforcement_bonus: 1.0,
            decay_cross_agent_bonus_per_agent: 0.15,
            decay_max_cross_agent_multiplier: 1.75,
            extraction_confidence_threshold: 0.3,
            extraction_min_fact_length: 10,
            extraction_max_fact_length: 500,
            instinct_min_tool_calls: 5,
        }
    }
}

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

/// Pylon API request size and idempotency cache limits.
///
/// All defaults match the current hardcoded constants in the `pylon` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ApiLimitsConfig {
    /// Maximum characters in a session name. Default: 255.
    /// Mirrors `pylon::handlers::sessions::MAX_SESSION_NAME_LEN`.
    pub max_session_name_len: usize,
    /// Maximum bytes in a session identifier. Default: 256.
    /// Mirrors `pylon::handlers::sessions::MAX_IDENTIFIER_BYTES`.
    pub max_identifier_bytes: usize,
    /// Maximum messages returned by the history endpoint. Default: 1000.
    /// Mirrors `pylon::handlers::sessions::MAX_HISTORY_LIMIT`.
    pub max_history_limit: u32,
    /// Default messages returned by the history endpoint. Default: 50.
    /// Mirrors `pylon::handlers::sessions::DEFAULT_HISTORY_LIMIT`.
    pub default_history_limit: u32,
    /// Maximum bytes per streaming message body. Default: 262144 (256 KiB).
    /// Mirrors `pylon::handlers::sessions::streaming::MAX_MESSAGE_BYTES`.
    pub max_message_bytes: usize,
    /// Maximum facts returned by a single knowledge list request. Default: 1000.
    /// Mirrors `pylon::handlers::knowledge::MAX_FACTS_LIMIT`.
    pub max_facts_limit: usize,
    /// Maximum results for a single knowledge search request. Default: 1000.
    /// Mirrors `pylon::handlers::knowledge::MAX_SEARCH_LIMIT`.
    pub max_search_limit: usize,
    /// Maximum facts in a single bulk-import request. Default: 1000.
    /// Mirrors `pylon::handlers::knowledge::bulk_import::MAX_IMPORT_BATCH_SIZE`.
    pub max_import_batch_size: usize,
    /// TTL in seconds for idempotency key cache entries. Default: 300.
    /// Mirrors `pylon::idempotency::DEFAULT_TTL`.
    pub idempotency_ttl_secs: u64,
    /// Maximum idempotency cache entries (LRU cap). Default: 10000.
    /// Mirrors `pylon::idempotency::DEFAULT_CAPACITY`.
    pub idempotency_capacity: usize,
    /// Maximum character length of an idempotency key. Default: 64.
    pub idempotency_max_key_length: usize,
    /// Acceptable clock skew in seconds before token expiry check warns. Default: 30.
    /// Mirrors `pylon::handlers::health::CLOCK_SKEW_LEEWAY`.
    pub clock_skew_leeway_secs: u64,
    /// Time in seconds before token expiry that triggers a warning. Default: 3600.
    /// Mirrors `pylon::handlers::health::EXPIRY_WARNING_THRESHOLD`.
    pub expiry_warning_threshold_secs: u64,
}

impl Default for ApiLimitsConfig {
    fn default() -> Self {
        Self {
            max_session_name_len: 255,
            max_identifier_bytes: 256,
            max_history_limit: 1_000,
            default_history_limit: 50,
            max_message_bytes: 262_144,
            max_facts_limit: 1_000,
            max_search_limit: 1_000,
            max_import_batch_size: 1_000,
            idempotency_ttl_secs: 300,
            idempotency_capacity: 10_000,
            idempotency_max_key_length: 64,
            clock_skew_leeway_secs: 30,
            expiry_warning_threshold_secs: 3_600,
        }
    }
}

/// Daemon watchdog, prosoche anomaly detection, and runner output settings.
///
/// All defaults match the current hardcoded constants in the `daemon` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct DaemonBehaviorConfig {
    /// Base duration in seconds for watchdog restart backoff. Default: 2.
    /// Mirrors `daemon::watchdog::BACKOFF_BASE`.
    pub watchdog_backoff_base_secs: u64,
    /// Maximum watchdog restart backoff duration in seconds. Default: 300.
    /// Mirrors `daemon::watchdog::BACKOFF_CAP`.
    pub watchdog_backoff_cap_secs: u64,
    /// Samples used for anomaly detection in prosoche attention check. Default: 15.
    /// Mirrors `daemon::prosoche::ANOMALY_SAMPLE_SIZE`.
    pub prosoche_anomaly_sample_size: usize,
    /// Lines from task output head to include in brief summary. Default: 5.
    /// Mirrors `daemon::runner::output::BRIEF_HEAD_LINES`.
    pub runner_output_brief_head_lines: usize,
    /// Lines from task output tail to include in brief summary. Default: 3.
    /// Mirrors `daemon::runner::output::BRIEF_TAIL_LINES`.
    pub runner_output_brief_tail_lines: usize,
}

impl Default for DaemonBehaviorConfig {
    fn default() -> Self {
        Self {
            watchdog_backoff_base_secs: 2,
            watchdog_backoff_cap_secs: 300,
            prosoche_anomaly_sample_size: 15,
            runner_output_brief_head_lines: 5,
            runner_output_brief_tail_lines: 3,
        }
    }
}

/// Organon tool size, timeout, and length limits.
///
/// All defaults match the current hardcoded constants in the `organon` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ToolLimitsConfig {
    /// Maximum character length for glob patterns. Default: 1000.
    /// Mirrors `organon::builtins::filesystem::MAX_PATTERN_LENGTH`.
    pub max_pattern_length: usize,
    /// Timeout in seconds for filesystem subprocess commands. Default: 60.
    /// Mirrors `organon::builtins::filesystem::SUBPROCESS_TIMEOUT`.
    pub subprocess_timeout_secs: u64,
    /// Maximum bytes per workspace write operation. Default: 10485760 (10 MiB).
    /// Mirrors `organon::builtins::workspace::MAX_WRITE_BYTES`.
    pub max_write_bytes: usize,
    /// Maximum bytes per workspace read operation. Default: 52428800 (50 MiB).
    /// Mirrors `organon::builtins::workspace::MAX_READ_BYTES`.
    pub max_read_bytes: u64,
    /// Maximum character length of a shell command. Default: 10000.
    /// Mirrors `organon::builtins::workspace::MAX_COMMAND_LENGTH`.
    pub max_command_length: usize,
    /// Maximum characters per intra-session message. Default: 4000.
    /// Mirrors `organon::builtins::communication::MESSAGE_MAX_LEN`.
    pub message_max_len: usize,
    /// Maximum characters per inter-session message. Default: 100000.
    /// Mirrors `organon::builtins::communication::INTER_SESSION_MAX_MESSAGE_LEN`.
    pub inter_session_max_message_len: usize,
    /// Maximum wait timeout in seconds for inter-session messages. Default: 300.
    /// Mirrors `organon::builtins::communication::INTER_SESSION_MAX_TIMEOUT_SECS`.
    pub inter_session_max_timeout_secs: u64,
}

impl Default for ToolLimitsConfig {
    fn default() -> Self {
        Self {
            max_pattern_length: 1_000,
            subprocess_timeout_secs: 60,
            max_write_bytes: 10_485_760,
            max_read_bytes: 52_428_800,
            max_command_length: 10_000,
            message_max_len: 4_000,
            inter_session_max_message_len: 100_000,
            inter_session_max_timeout_secs: 300,
        }
    }
}

/// Agora messaging transport poll, buffer, circuit-breaker, and RPC settings.
///
/// All defaults match the current hardcoded constants in the `agora` crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct MessagingConfig {
    /// How often Semeion polls for new channel messages in milliseconds. Default: 2000.
    /// Mirrors `agora::semeion::DEFAULT_POLL_INTERVAL`.
    pub poll_interval_ms: u64,
    /// Inbound message buffer size per channel. Default: 100.
    /// Mirrors `agora::semeion::DEFAULT_BUFFER_CAPACITY`.
    pub buffer_capacity: usize,
    /// Consecutive channel errors before the channel is halted. Default: 5.
    /// Mirrors `agora::semeion::CIRCUIT_BREAKER_THRESHOLD`.
    pub circuit_breaker_threshold: u32,
    /// How often a halted channel is health-checked in seconds. Default: 60.
    /// Mirrors `agora::semeion::HALTED_HEALTH_CHECK_INTERVAL`.
    pub halted_health_check_interval_secs: u64,
    /// Timeout in seconds for Semeion RPC calls. Default: 10.
    /// Mirrors `agora::semeion::client::RPC_TIMEOUT`.
    pub rpc_timeout_secs: u64,
    /// Timeout in seconds for Semeion health-check requests. Default: 2.
    /// Mirrors `agora::semeion::client::HEALTH_TIMEOUT`.
    pub health_timeout_secs: u64,
    /// Timeout in seconds waiting to receive a Semeion response. Default: 15.
    /// Mirrors `agora::semeion::client::RECEIVE_TIMEOUT`.
    pub receive_timeout_secs: u64,
    /// Default timeout in seconds for agent-dispatch tool calls. Default: 300.
    /// Mirrors `organon::builtins::agent::DEFAULT_TIMEOUT_SECS`.
    pub agent_dispatch_timeout_secs: u64,
}

impl Default for MessagingConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 2_000,
            buffer_capacity: 100,
            circuit_breaker_threshold: 5,
            halted_health_check_interval_secs: 60,
            rpc_timeout_secs: 10,
            health_timeout_secs: 2,
            receive_timeout_secs: 15,
            agent_dispatch_timeout_secs: 300,
        }
    }
}

/// Per-agent behavioral parameters: safety, hooks, distillation, competence,
/// drift, uncertainty, skills, planning, knowledge tuning, fact lifecycle,
/// similarity, tool behavior, and correction limits.
///
/// All defaults match the current hardcoded constants spread across `nous`,
/// `episteme`, `dianoia`, `melete`, `eidos`, and `organon`. Wave 0 adds the
/// schema; waves 1-4 will replace the individual `const` declarations with
/// reads from the resolved config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "hook toggles are a genuine set of independent feature flags, not a state machine"
)]
pub struct AgentBehaviorDefaults {
    // --- Safety ---
    /// Consecutive identical tool-call sequences before loop detection fires. Default: 3.
    pub safety_loop_detection_threshold: u32,
    /// Consecutive errors before the pipeline aborts with a safety interrupt. Default: 4.
    pub safety_consecutive_error_threshold: u32,
    /// Maximum loop-detection warnings before the session is halted. Default: 2.
    pub safety_loop_max_warnings: u32,
    /// Hard token cap for a single session. Default: 500000.
    pub safety_session_token_cap: u64,
    /// Maximum consecutive tool-only iterations before forcing a text response. Default: 3.
    pub safety_max_consecutive_tool_only_iterations: u32,

    // --- Hooks ---
    /// Whether cost-control hooks are active. Default: true.
    pub hooks_cost_control_enabled: bool,
    /// Per-turn token budget (0 = unlimited). Default: 0.
    pub hooks_turn_token_budget: u64,
    /// Whether scope-enforcement hooks are active. Default: true.
    pub hooks_scope_enforcement_enabled: bool,
    /// Whether correction hooks are active. Default: true.
    pub hooks_correction_hooks_enabled: bool,
    /// Whether audit logging hooks are active. Default: true.
    pub hooks_audit_logging_enabled: bool,

    // --- Distillation ---
    /// Context token count that triggers automatic distillation. Default: 120000.
    /// Mirrors `nous::distillation::CONTEXT_TOKEN_TRIGGER`.
    pub distillation_context_token_trigger: u64,
    /// Message count that triggers distillation. Default: 150.
    /// Mirrors `nous::distillation::MESSAGE_COUNT_TRIGGER`.
    pub distillation_message_count_trigger: u64,
    /// Days idle before a session is considered stale for distillation. Default: 7.
    /// Mirrors `nous::distillation::STALE_SESSION_DAYS`.
    pub distillation_stale_session_days: u64,
    /// Minimum messages required for stale-session distillation. Default: 20.
    /// Mirrors `nous::distillation::STALE_SESSION_MIN_MESSAGES`.
    pub distillation_stale_min_messages: u64,
    /// Message count trigger for sessions never distilled. Default: 30.
    /// Mirrors `nous::distillation::NEVER_DISTILLED_MESSAGE_TRIGGER`.
    pub distillation_never_distilled_trigger: u64,
    /// Minimum messages for legacy distillation threshold. Default: 10.
    /// Mirrors `nous::distillation::LEGACY_THRESHOLD_MIN_MESSAGES`.
    pub distillation_legacy_min_messages: u64,
    /// Maximum backoff turns before distillation is forced. Default: 8.
    /// Mirrors `melete::distill::MAX_BACKOFF_TURNS`.
    pub distillation_max_backoff_turns: u32,

    // --- Competence scoring ---
    /// Competence score penalty per correction. Default: 0.05.
    /// Mirrors `nous::competence::CORRECTION_PENALTY`.
    pub competence_correction_penalty: f64,
    /// Competence score bonus per successful turn. Default: 0.02.
    /// Mirrors `nous::competence::SUCCESS_BONUS`.
    pub competence_success_bonus: f64,
    /// Competence score penalty per user disagreement. Default: 0.01.
    /// Mirrors `nous::competence::DISAGREEMENT_PENALTY`.
    pub competence_disagreement_penalty: f64,
    /// Competence score floor. Default: 0.1.
    /// Mirrors `nous::competence::MIN_SCORE`.
    pub competence_min_score: f64,
    /// Competence score ceiling. Default: 0.95.
    /// Mirrors `nous::competence::MAX_SCORE`.
    pub competence_max_score: f64,
    /// Initial competence score for a new agent. Default: 0.5.
    /// Mirrors `nous::competence::DEFAULT_SCORE`.
    pub competence_default_score: f64,
    /// Competence score below which escalation fires. Default: 0.30.
    /// Mirrors `nous::competence::ESCALATION_FAILURE_THRESHOLD`.
    pub competence_escalation_failure_threshold: f64,
    /// Minimum samples before escalation threshold is evaluated. Default: 5.
    /// Mirrors `nous::competence::ESCALATION_MIN_SAMPLES`.
    pub competence_escalation_min_samples: u32,

    // --- Drift detection ---
    /// Sliding window size for response-quality drift detection. Default: 20.
    /// Mirrors `nous::drift::DEFAULT_WINDOW_SIZE`.
    pub drift_window_size: usize,
    /// Comparison window for recent vs. historical drift. Default: 5.
    /// Mirrors `nous::drift::DEFAULT_RECENT_SIZE`.
    pub drift_recent_size: usize,
    /// Standard deviations required to flag drift. Default: 2.0.
    /// Mirrors `nous::drift::DEFAULT_DEVIATION_THRESHOLD`.
    pub drift_deviation_threshold: f64,
    /// Minimum samples before drift detection activates. Default: 8.
    /// Mirrors `nous::drift::MIN_SAMPLES`.
    pub drift_min_samples: usize,

    // --- Uncertainty calibration ---
    /// Maximum calibration data points retained for the uncertainty model. Default: 1000.
    /// Mirrors `nous::uncertainty::MAX_CALIBRATION_POINTS`.
    pub uncertainty_max_calibration_points: usize,

    // --- Skills ---
    /// Maximum number of skills loadable per agent. Default: 5.
    pub skills_max_skills: usize,
    /// Maximum chars from context used when matching skills. Default: 200.
    /// Mirrors `nous::skills::MAX_CONTEXT_CHARS`.
    pub skills_max_context_chars: usize,

    // --- Working state ---
    /// Working-state TTL in seconds before expiry. Default: 604800 (7 days).
    pub working_state_ttl_secs: u64,
    /// Maximum task stack depth before oldest entries are evicted. Default: 10.
    /// Mirrors `nous::working_state::MAX_TASK_STACK`.
    pub working_state_max_task_stack: usize,

    // --- Planning ---
    /// Maximum planning iterations per planning cycle. Default: 10.
    /// Mirrors `dianoia::plan::DEFAULT_MAX_ITERATIONS`.
    pub planning_max_iterations: u32,
    /// History turns inspected for stuck-detection. Default: 20.
    /// Mirrors `dianoia::stuck::DEFAULT_HISTORY_WINDOW`.
    pub planning_stuck_history_window: u32,
    /// Repeated errors before agent is flagged stuck. Default: 3.
    /// Mirrors `dianoia::stuck::DEFAULT_REPEATED_ERROR_THRESHOLD`.
    pub planning_stuck_repeated_error_threshold: u32,
    /// Identical-argument tool calls before stuck detection fires. Default: 3.
    /// Mirrors `dianoia::stuck::DEFAULT_SAME_ARGS_THRESHOLD`.
    pub planning_stuck_same_args_threshold: u32,
    /// Alternating tool-call pairs before stuck detection fires. Default: 3.
    /// Mirrors `dianoia::stuck::DEFAULT_ALTERNATING_THRESHOLD`.
    pub planning_stuck_alternating_threshold: u32,
    /// Escalating retry pattern depth before stuck detection fires. Default: 3.
    /// Mirrors `dianoia::stuck::DEFAULT_ESCALATING_RETRY_THRESHOLD`.
    pub planning_stuck_escalating_retry_threshold: u32,

    // --- Knowledge tuning (instinct / surprise / rules / dedup) ---
    /// Minimum observations before an instinct is eligible. Default: 5.
    pub knowledge_instinct_min_observations: u32,
    /// Minimum success rate for an instinct to surface. Default: 0.80.
    pub knowledge_instinct_min_success_rate: f64,
    /// Minimum stability hours before an instinct is surfaced. Default: 168.0.
    pub knowledge_instinct_stability_hours: f64,
    /// Standard deviations above baseline for surprise detection. Default: 2.0.
    /// Mirrors `episteme::surprise::DEFAULT_THRESHOLD`.
    pub knowledge_surprise_threshold: f64,
    /// EMA alpha for surprise baseline. Default: 0.3.
    /// Mirrors `episteme::surprise::EMA_ALPHA`.
    pub knowledge_surprise_ema_alpha: f64,
    /// Minimum observations before a rule proposal is eligible. Default: 5.
    /// Mirrors `episteme::rule_proposals::MIN_OBSERVATIONS`.
    pub knowledge_rule_min_observations: u32,
    /// Minimum confidence for a rule proposal to surface. Default: 0.60.
    /// Mirrors `episteme::rule_proposals::MIN_CONFIDENCE`.
    pub knowledge_rule_min_confidence: f64,
    /// Weight of name similarity in dedup scoring. Default: 0.4.
    /// Mirrors `episteme::dedup::WEIGHT_NAME`.
    pub knowledge_dedup_weight_name: f64,
    /// Weight of embedding similarity in dedup scoring. Default: 0.3.
    /// Mirrors `episteme::dedup::WEIGHT_EMBED`.
    pub knowledge_dedup_weight_embed: f64,
    /// Weight of fact-type match in dedup scoring. Default: 0.2.
    /// Mirrors `episteme::dedup::WEIGHT_TYPE`.
    pub knowledge_dedup_weight_type: f64,
    /// Weight of alias similarity in dedup scoring. Default: 0.1.
    /// Mirrors `episteme::dedup::WEIGHT_ALIAS`.
    pub knowledge_dedup_weight_alias: f64,
    /// Jaro-Winkler score above which strings are considered similar. Default: 0.85.
    /// Mirrors `episteme::dedup::JW_THRESHOLD`.
    pub knowledge_dedup_jw_threshold: f64,
    /// Cosine similarity above which embeddings are considered similar. Default: 0.80.
    /// Mirrors `episteme::dedup::EMBED_THRESHOLD`.
    pub knowledge_dedup_embed_threshold: f64,

    // --- Fact lifecycle ---
    /// Confidence above which a fact is considered Active. Default: 0.7.
    /// Mirrors `eidos::knowledge::fact::STAGE_ACTIVE_THRESHOLD`.
    pub fact_active_threshold: f64,
    /// Confidence below which a fact is considered Fading. Default: 0.3.
    /// Mirrors `eidos::knowledge::fact::STAGE_FADING_THRESHOLD`.
    pub fact_fading_threshold: f64,
    /// Confidence below which a fact is considered Dormant. Default: 0.1.
    /// Mirrors `eidos::knowledge::fact::STAGE_DORMANT_THRESHOLD`.
    pub fact_dormant_threshold: f64,

    // --- Similarity ---
    /// Similarity score threshold for recall deduplication. Default: 0.85.
    pub similarity_threshold: f64,

    // --- Tool behavior ---
    /// Maximum concurrent agent-dispatch tasks. Default: 10.
    /// Mirrors `organon::builtins::agent::MAX_DISPATCH_TASKS`.
    pub tool_agent_dispatch_max_tasks: usize,
    /// Default row limit for Datalog memory queries. Default: 100.
    /// Mirrors `organon::builtins::memory::datalog::DEFAULT_ROW_LIMIT`.
    pub tool_datalog_default_row_limit: u32,
    /// Default query timeout in seconds for the Datalog memory tool. Default: 5.0.
    /// Mirrors `organon::builtins::memory::datalog::DEFAULT_TIMEOUT_SECS`.
    pub tool_datalog_default_timeout_secs: f64,
    /// Maximum image file size in bytes for the view-file tool. Default: 20971520 (20 MiB).
    /// Mirrors `organon::builtins::view_file::MAX_IMAGE_BYTES`.
    pub tool_max_image_bytes: usize,
    /// Maximum PDF file size in bytes for the view-file tool. Default: 33554432 (32 MiB).
    /// Mirrors `organon::builtins::view_file::MAX_PDF_BYTES`.
    pub tool_max_pdf_bytes: usize,

    // --- Bootstrap ---
    /// Minimum token budget remaining before attempting section truncation.
    /// Below this threshold the section is dropped rather than truncated. Default: 200.
    /// Mirrors `nous::bootstrap::MIN_TRUNCATION_BUDGET`.
    pub bootstrap_min_truncation_budget: u64,

    // --- Corrections ---
    /// Maximum correction entries stored per agent. Default: 50.
    /// Mirrors `nous::hooks::builtins::correction::MAX_CORRECTIONS`.
    pub corrections_max_corrections: usize,
}

impl Default for AgentBehaviorDefaults {
    fn default() -> Self {
        Self {
            // Safety
            safety_loop_detection_threshold: 3,
            safety_consecutive_error_threshold: 4,
            safety_loop_max_warnings: 2,
            safety_session_token_cap: 500_000,
            safety_max_consecutive_tool_only_iterations: 3,
            // Hooks
            hooks_cost_control_enabled: true,
            hooks_turn_token_budget: 0,
            hooks_scope_enforcement_enabled: true,
            hooks_correction_hooks_enabled: true,
            hooks_audit_logging_enabled: true,
            // Distillation
            distillation_context_token_trigger: 120_000,
            distillation_message_count_trigger: 150,
            distillation_stale_session_days: 7,
            distillation_stale_min_messages: 20,
            distillation_never_distilled_trigger: 30,
            distillation_legacy_min_messages: 10,
            distillation_max_backoff_turns: 8,
            // Competence
            competence_correction_penalty: 0.05,
            competence_success_bonus: 0.02,
            competence_disagreement_penalty: 0.01,
            competence_min_score: 0.1,
            competence_max_score: 0.95,
            competence_default_score: 0.5,
            competence_escalation_failure_threshold: 0.30,
            competence_escalation_min_samples: 5,
            // Drift
            drift_window_size: 20,
            drift_recent_size: 5,
            drift_deviation_threshold: 2.0,
            drift_min_samples: 8,
            // Uncertainty
            uncertainty_max_calibration_points: 1_000,
            // Skills
            skills_max_skills: 5,
            skills_max_context_chars: 200,
            // Working state
            working_state_ttl_secs: 604_800,
            working_state_max_task_stack: 10,
            // Planning
            planning_max_iterations: 10,
            planning_stuck_history_window: 20,
            planning_stuck_repeated_error_threshold: 3,
            planning_stuck_same_args_threshold: 3,
            planning_stuck_alternating_threshold: 3,
            planning_stuck_escalating_retry_threshold: 3,
            // Knowledge tuning
            knowledge_instinct_min_observations: 5,
            knowledge_instinct_min_success_rate: 0.80,
            knowledge_instinct_stability_hours: 168.0,
            knowledge_surprise_threshold: 2.0,
            knowledge_surprise_ema_alpha: 0.3,
            knowledge_rule_min_observations: 5,
            knowledge_rule_min_confidence: 0.60,
            knowledge_dedup_weight_name: 0.4,
            knowledge_dedup_weight_embed: 0.3,
            knowledge_dedup_weight_type: 0.2,
            knowledge_dedup_weight_alias: 0.1,
            knowledge_dedup_jw_threshold: 0.85,
            knowledge_dedup_embed_threshold: 0.80,
            // Fact lifecycle
            fact_active_threshold: 0.7,
            fact_fading_threshold: 0.3,
            fact_dormant_threshold: 0.1,
            // Similarity
            similarity_threshold: 0.85,
            // Tool behavior
            tool_agent_dispatch_max_tasks: 10,
            tool_datalog_default_row_limit: 100,
            tool_datalog_default_timeout_secs: 5.0,
            tool_max_image_bytes: 20_971_520,
            tool_max_pdf_bytes: 33_554_432,
            // Bootstrap
            bootstrap_min_truncation_budget: 200,
            // Corrections
            corrections_max_corrections: 50,
        }
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
