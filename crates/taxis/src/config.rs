//! Configuration types for an Aletheia instance.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
}

impl Default for AgentDefaults {
    fn default() -> Self {
        Self {
            model: ModelSpec::default(),
            context_tokens: 200_000,
            max_output_tokens: 16_384,
            bootstrap_max_tokens: 40_000,
            thinking_enabled: false,
            thinking_budget: 10_000,
            agency: AgencyLevel::Standard,
            max_tool_iterations: 200,
            allowed_roots: Vec::new(),
            caching: CachingConfig::default(),
            recall: RecallSettings::default(),
            chars_per_token: 4,
            history_budget_ratio: 0.6,
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
}

impl Default for ModelSpec {
    fn default() -> Self {
        Self {
            primary: "claude-sonnet-4-6".to_owned(),
            fallbacks: Vec::new(),
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
    pub signing_key: Option<String>,
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
    /// Maximum requests per minute per client IP.
    pub requests_per_minute: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            requests_per_minute: 60,
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

/// Session retention policy configuration.
///
/// Retention execution is not yet implemented; this struct is a placeholder.
// TODO(#1129): Wire retention policy fields when the executor is implemented.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RetentionConfig {}
/// Instance maintenance settings.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct MaintenanceSettings {
    /// Trace log file rotation and compression.
    pub trace_rotation: TraceRotationSettings,
    /// Filesystem drift detection against expected instance layout.
    pub drift_detection: DriftDetectionSettings,
    /// Database size monitoring and alerting.
    pub db_monitoring: DbMonitoringSettings,
    /// Automatic data retention enforcement.
    pub retention: RetentionSettings,
    /// Whether background knowledge graph maintenance tasks are enabled.
    #[serde(default)]
    pub knowledge_maintenance_enabled: bool,
}

/// Trace file rotation settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct TraceRotationSettings {
    /// Whether automatic trace rotation runs.
    pub enabled: bool,
    /// Delete trace files older than this many days.
    pub max_age_days: u32,
    /// Maximum total trace directory size in MB before pruning.
    pub max_total_size_mb: u64,
    /// Whether to gzip-compress rotated trace files.
    pub compress: bool,
    /// Maximum number of compressed archive files to retain.
    pub max_archives: usize,
}

impl Default for TraceRotationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_age_days: 14,
            max_total_size_mb: 500,
            compress: true,
            max_archives: 30,
        }
    }
}

/// Instance drift detection settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct DriftDetectionSettings {
    /// Whether drift detection runs during maintenance.
    pub enabled: bool,
    /// Emit warnings for files missing from the expected layout.
    pub alert_on_missing: bool,
    /// Glob patterns for paths to ignore during drift checks.
    pub ignore_patterns: Vec<String>,
}

impl Default for DriftDetectionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            alert_on_missing: true,
            ignore_patterns: vec![
                "data/".to_owned(),
                "signal/".to_owned(),
                "*.db".to_owned(),
                ".gitkeep".to_owned(),
            ],
        }
    }
}

/// Database size monitoring settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct DbMonitoringSettings {
    /// Whether database size monitoring runs.
    pub enabled: bool,
    /// Emit a warning when any database exceeds this size in MB.
    pub warn_threshold_mb: u64,
    /// Emit an alert when any database exceeds this size in MB.
    pub alert_threshold_mb: u64,
}

impl Default for DbMonitoringSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            warn_threshold_mb: 100,
            alert_threshold_mb: 500,
        }
    }
}

/// Data retention execution settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[derive(Default)]
pub struct RetentionSettings {
    /// Whether automatic retention enforcement (session cleanup) runs.
    pub enabled: bool,
}

/// Sandbox configuration for tool command execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct SandboxSettings {
    /// Whether sandbox restrictions are applied to tool execution.
    pub enabled: bool,
    /// Enforcement level: `enforcing` blocks violations, `permissive` logs them.
    pub enforcement: SandboxEnforcementMode,
    /// Additional filesystem paths granted read access.
    pub extra_read_paths: Vec<PathBuf>,
    /// Additional filesystem paths granted read+write access.
    pub extra_write_paths: Vec<PathBuf>,
    /// Additional filesystem paths granted execute access.
    ///
    /// Values may begin with `~` which is expanded to the HOME environment
    /// variable at policy-build time.
    pub extra_exec_paths: Vec<PathBuf>,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            enforcement: SandboxEnforcementMode::Permissive,
            extra_read_paths: Vec::new(),
            extra_write_paths: Vec::new(),
            extra_exec_paths: Vec::new(),
        }
    }
}

/// Credential resolution configuration.
///
/// Controls how the server discovers LLM API credentials. The `source` field
/// selects the strategy:
///
/// - `"auto"` (default): instance credential file → env vars → Claude Code credentials
/// - `"api-key"`: only instance credential file and env vars
/// - `"claude-code"`: prefer Claude Code's `~/.claude/.credentials.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct CredentialConfig {
    /// Credential source strategy: `"auto"`, `"api-key"`, or `"claude-code"`.
    pub source: String,
    /// Override path to the Claude Code credentials file.
    /// Defaults to `~/.claude/.credentials.json`.
    pub claude_code_credentials: Option<String>,
}

impl Default for CredentialConfig {
    fn default() -> Self {
        Self {
            source: "auto".to_owned(),
            claude_code_credentials: None,
        }
    }
}

/// Structured file logging configuration.
///
/// Controls where server logs are written, how long they are retained, and
/// which minimum severity level is written to the log files.
///
/// Log files are written in JSON format with daily rotation using
/// `tracing_appender`. Old files are pruned after `retention_days` days by
/// the log retention background task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct LoggingSettings {
    /// Directory where daily log files are written.
    ///
    /// Relative paths are resolved from the instance root. `None` (the
    /// default) resolves to `{instance}/logs/`.
    pub log_dir: Option<String>,
    /// Number of days to retain log files before they are deleted.
    ///
    /// Cleanup is performed once daily at server startup and every 24 hours
    /// thereafter. Default: 14 days.
    pub retention_days: u32,
    /// Minimum log level written to log files.
    ///
    /// Accepts any `tracing` filter directive (e.g. `"warn"`, `"error"`,
    /// `"aletheia=debug,warn"`). Default: `"warn"`, which captures WARN and
    /// ERROR events from all crates regardless of the console log level.
    pub level: String,
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            log_dir: None,
            retention_days: 14,
            level: "warn".to_owned(),
        }
    }
}

/// MCP server configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct McpConfig {
    /// Per-session rate limiting for MCP tool calls.
    pub rate_limit: McpRateLimitConfig,
}

/// Per-session rate limiting configuration for MCP requests.
///
/// Applies separate token bucket limits for expensive operations
/// (session_message, session_create, knowledge_search) and cheap
/// read/status operations. Limits are enforced per MCP session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct McpRateLimitConfig {
    /// Whether MCP rate limiting is active.
    pub enabled: bool,
    /// Maximum requests per minute for expensive operations.
    pub message_requests_per_minute: u32,
    /// Maximum requests per minute for read/status operations.
    pub read_requests_per_minute: u32,
}

impl Default for McpRateLimitConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            message_requests_per_minute: 60,
            read_requests_per_minute: 300,
        }
    }
}

/// Resolved configuration for a specific nous agent.
///
/// Produced by merging [`AgentDefaults`] with a matching [`NousDefinition`].
#[derive(Debug, Clone)]
pub struct ResolvedNousConfig {
    /// Agent identifier.
    pub id: String,
    /// Human-readable display name (from the agent definition, if set).
    pub name: Option<String>,
    /// Resolved primary model identifier.
    pub model: String,
    /// Ordered fallback models.
    pub fallbacks: Vec<String>,
    /// Maximum input context window in tokens.
    pub context_tokens: u32,
    /// Maximum output tokens per response.
    pub max_output_tokens: u32,
    /// Token budget for bootstrap content.
    pub bootstrap_max_tokens: u32,
    /// Whether extended thinking is enabled for this agent.
    pub thinking_enabled: bool,
    /// Token budget for extended thinking.
    pub thinking_budget: u32,
    /// Effective agency level for this agent.
    pub agency: AgencyLevel,
    /// Maximum consecutive tool use iterations per turn.
    pub max_tool_iterations: u32,
    /// Resolved workspace directory path.
    pub workspace: String,
    /// Merged set of permitted filesystem roots.
    pub allowed_roots: Vec<String>,
    /// Knowledge domains this agent covers.
    pub domains: Vec<String>,
    /// Whether prompt caching is enabled.
    pub cache_enabled: bool,
    /// Resolved recall pipeline settings.
    pub recall: RecallSettings,
    /// Characters per token for token-budget estimation.
    pub chars_per_token: u32,
    /// Fraction of the context window reserved for conversation history.
    pub history_budget_ratio: f64,
}

/// Resolve effective configuration for a specific nous agent.
///
/// Merges `agents.defaults` with the matching entry from `agents.list`.
/// If no matching agent is found, returns defaults with the given id.
#[must_use]
pub fn resolve_nous(config: &AletheiaConfig, nous_id: &str) -> ResolvedNousConfig {
    let defaults = &config.agents.defaults;
    let agent = config.agents.list.iter().find(|a| a.id == nous_id);

    let (model, fallbacks) = match agent.and_then(|a| a.model.as_ref()) {
        Some(spec) => (spec.primary.clone(), spec.fallbacks.clone()),
        None => (
            defaults.model.primary.clone(),
            defaults.model.fallbacks.clone(),
        ),
    };

    let agency = agent.and_then(|a| a.agency).unwrap_or(defaults.agency);

    let thinking_enabled = agent
        .and_then(|a| a.thinking_enabled)
        .unwrap_or(defaults.thinking_enabled);

    let max_tool_iterations = match agency {
        AgencyLevel::Unrestricted => 10_000,
        AgencyLevel::Standard => defaults.max_tool_iterations,
        AgencyLevel::Restricted => 50,
    };

    let workspace = agent.map_or_else(
        || format!("instance/nous/{nous_id}"),
        |a| a.workspace.clone(),
    );

    let mut allowed_roots = defaults.allowed_roots.clone();
    if let Some(agent) = agent {
        for root in &agent.allowed_roots {
            if !allowed_roots.contains(root) {
                allowed_roots.push(root.clone());
            }
        }
    }

    let domains = agent.map(|a| a.domains.clone()).unwrap_or_default();
    let name = agent.and_then(|a| a.name.clone());

    // NOTE: Agent-level recall overrides; falls back to shared defaults.
    let recall = agent
        .and_then(|a| a.recall.clone())
        .unwrap_or_else(|| defaults.recall.clone());

    ResolvedNousConfig {
        id: nous_id.to_owned(),
        name,
        model,
        fallbacks,
        context_tokens: defaults.context_tokens,
        max_output_tokens: defaults.max_output_tokens,
        bootstrap_max_tokens: defaults.bootstrap_max_tokens,
        thinking_enabled,
        thinking_budget: defaults.thinking_budget,
        agency,
        max_tool_iterations,
        workspace,
        allowed_roots,
        domains,
        cache_enabled: defaults.caching.enabled && defaults.caching.strategy != "disabled",
        recall,
        chars_per_token: defaults.chars_per_token,
        history_budget_ratio: defaults.history_budget_ratio,
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
