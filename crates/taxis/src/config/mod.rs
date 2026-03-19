//! Configuration types for an Aletheia instance.

mod maintenance;
mod resolved;

pub use maintenance::{
    CircuitBreakerSettings, CredentialConfig, DbMonitoringSettings, DiskSpaceSettings,
    DriftDetectionSettings, LoggingSettings, MaintenanceSettings, McpConfig, McpRateLimitConfig,
    RedactionSettings, RetentionConfig, RetentionSettings, SandboxSettings, SqliteRecoverySettings,
    TraceRotationSettings,
};
pub use resolved::{
    AgentCapabilities, ResolvedModelConfig, ResolvedNousConfig, TokenLimits, resolve_nous,
};

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use aletheia_koina::secret::SecretString;

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
    /// Data lifecycle and retention policies.
    pub data: DataConfig,
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
}

/// Sandbox enforcement level for tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SandboxEnforcementMode {
    Enforcing,
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
    Unrestricted,
    #[default]
    Standard,
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
    /// Temporal decay weight (0.0–1.0).
    pub decay: f64,
    /// Content relevance weight (0.0–1.0).
    pub relevance: f64,
    /// Epistemic tier weight (0.0–1.0).
    pub epistemic_tier: f64,
    /// Knowledge-graph relationship proximity weight (0.0–1.0).
    pub relationship_proximity: f64,
    /// Access frequency weight (0.0–1.0).
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
    /// Minimum relevance score (0.0–1.0) to include a recalled fact.
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

/// Default values applied to every agent unless overridden.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AgentDefaults {
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
    /// Characters per token for conservative token-budget estimation.
    ///
    /// Used by `CharEstimator` when counting tokens from raw text length.
    /// The default of 4 follows the common "1 token ≈ 4 chars" heuristic for
    /// English text. Increase for more conservative budgets; decrease for
    /// languages with shorter tokens.
    pub chars_per_token: u32,
    /// Fraction of the context window reserved for conversation history.
    ///
    /// The pipeline partitions the context window into three zones:
    /// `history` (this fraction), `turn reserve` (`max_output_tokens`), and
    /// `bootstrap` (the remainder, capped at `bootstrap_max_tokens`).
    /// Default: 0.6 (60 % of the context window).
    pub history_budget_ratio: f64,
    /// Model used for prosoche heartbeat sessions instead of the primary model.
    ///
    /// Prosoche checks are simple health/attention tasks that don't need
    /// advanced reasoning. Defaults to Haiku-tier to reduce cost.
    pub prosoche_model: String,
    /// Maximum size in bytes for a single tool result before truncation.
    ///
    /// Tool results exceeding this limit are truncated to fit, with a
    /// `[truncated: {original} -> {truncated} bytes]` indicator appended.
    /// Set to `0` to disable truncation. Default: 32 768 (32 KB).
    pub max_tool_result_bytes: u32,
}

impl Default for AgentDefaults {
    fn default() -> Self {
        use aletheia_koina::defaults as d;
        Self {
            model: ModelSpec::default(),
            context_tokens: d::CONTEXT_TOKENS,
            max_output_tokens: d::MAX_OUTPUT_TOKENS,
            bootstrap_max_tokens: d::BOOTSTRAP_MAX_TOKENS,
            thinking_enabled: false,
            thinking_budget: 10_000,
            agency: AgencyLevel::Standard,
            max_tool_iterations: d::MAX_TOOL_ITERATIONS,
            allowed_roots: Vec::new(),
            caching: CachingConfig::default(),
            recall: RecallSettings::default(),
            chars_per_token: d::CHARS_PER_TOKEN,
            history_budget_ratio: d::HISTORY_BUDGET_RATIO,
            prosoche_model: "claude-haiku-4-5-20251001".to_owned(),
            max_tool_result_bytes: d::MAX_TOOL_RESULT_BYTES,
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
            primary: "claude-sonnet-4-6".to_owned(),
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
    /// Model override; when `None`, inherits from [`AgentDefaults::model`].
    #[serde(default)]
    pub model: Option<ModelSpec>,
    /// Filesystem path to the agent's workspace directory.
    pub workspace: String,
    /// Thinking override; when `None`, inherits from [`AgentDefaults::thinking_enabled`].
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
        Self {
            enabled: true,
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
}

impl Default for SignalAccountConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            http_host: "localhost".to_owned(),
            http_port: 8080,
        }
    }
}

/// Data lifecycle configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct DataConfig {
    /// Session and message retention policies.
    pub retention: RetentionConfig,
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
