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
    /// External domain pack paths (directories containing pack.yaml).
    pub packs: Vec<PathBuf>,
    /// Periodic maintenance task configuration (trace rotation, drift detection, etc.).
    pub maintenance: MaintenanceSettings,
    /// Per-model pricing for LLM cost metrics. Keyed by model name.
    pub pricing: HashMap<String, ModelPricing>,
    /// Sandbox configuration for tool execution.
    pub sandbox: SandboxSettings,
}

/// Sandbox enforcement level for tool execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum SandboxEnforcementMode {
    Enforcing,
    Permissive,
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
    /// Source pattern — phone number, group ID, or "*" for default.
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
    /// IANA timezone for date/time formatting in prompts.
    pub user_timezone: String,
    /// Per-turn timeout in seconds before the request is cancelled.
    pub timeout_seconds: u32,
    /// Whether extended thinking is enabled by default.
    pub thinking_enabled: bool,
    /// Maximum tokens allocated to extended thinking when enabled.
    pub thinking_budget: u32,
    /// Safety limit on consecutive tool use iterations per turn.
    pub max_tool_iterations: u32,
    /// Filesystem paths the agent is permitted to access.
    pub allowed_roots: Vec<String>,
    /// Per-tool execution timeout overrides.
    pub tool_timeouts: ToolTimeouts,
    /// Prompt caching configuration.
    pub caching: CachingConfig,
}

impl Default for AgentDefaults {
    fn default() -> Self {
        Self {
            model: ModelSpec::default(),
            context_tokens: 200_000,
            max_output_tokens: 16_384,
            bootstrap_max_tokens: 40_000,
            user_timezone: "UTC".to_owned(),
            timeout_seconds: 300,
            thinking_enabled: false,
            thinking_budget: 10_000,
            max_tool_iterations: 50,
            allowed_roots: Vec::new(),
            tool_timeouts: ToolTimeouts::default(),
            caching: CachingConfig::default(),
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

/// Tool execution timeout settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ToolTimeouts {
    /// Default timeout for all tools in milliseconds.
    pub default_ms: u64,
    /// Per-tool timeout overrides keyed by tool name.
    pub overrides: HashMap<String, u64>,
}

impl Default for ToolTimeouts {
    fn default() -> Self {
        Self {
            default_ms: 120_000,
            overrides: HashMap::new(),
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
    /// Additional filesystem roots this agent may access (merged with defaults).
    #[serde(default)]
    pub allowed_roots: Vec<String>,
    /// Knowledge domains this agent specializes in (e.g. `"code"`, `"research"`).
    #[serde(default)]
    pub domains: Vec<String>,
    /// Whether this is the default agent for unrouted messages.
    #[serde(default)]
    pub default: bool,
}

/// HTTP gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct GatewayConfig {
    /// TCP port the gateway listens on.
    pub port: u16,
    /// Bind mode: `"lan"` for LAN-accessible, `"localhost"` for loopback only.
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
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            port: 18789,
            bind: "lan".to_owned(),
            auth: GatewayAuthConfig::default(),
            tls: TlsConfig::default(),
            cors: CorsConfig::default(),
            body_limit: BodyLimitConfig::default(),
            csrf: CsrfConfig::default(),
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
            provider: "mock".to_owned(),
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
#[expect(
    clippy::struct_excessive_bools,
    reason = "mirrors TS config schema 1:1"
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct SignalAccountConfig {
    /// Human-readable label for this account.
    pub name: Option<String>,
    /// Whether this account is active.
    pub enabled: bool,
    /// Phone number or account identifier registered with Signal.
    pub account: Option<String>,
    /// Hostname for the signal-cli JSON-RPC HTTP interface.
    pub http_host: String,
    /// Port for the signal-cli JSON-RPC HTTP interface.
    pub http_port: u16,
    /// Filesystem path to the signal-cli binary (auto-detected if `None`).
    pub cli_path: Option<String>,
    /// Whether to auto-start signal-cli when the daemon starts.
    pub auto_start: bool,
    /// Direct message policy: `"open"` accepts all, `"allowlist"` restricts.
    pub dm_policy: String,
    /// Group message policy: `"open"` or `"allowlist"`.
    pub group_policy: String,
    /// Whether the bot must be @mentioned to respond in groups.
    pub require_mention: bool,
    /// Whether to send read receipts for processed messages.
    pub send_read_receipts: bool,
    /// Maximum characters per outbound text chunk before splitting.
    pub text_chunk_limit: u32,
}

impl Default for SignalAccountConfig {
    fn default() -> Self {
        Self {
            name: None,
            enabled: true,
            account: None,
            http_host: "localhost".to_owned(),
            http_port: 8080,
            cli_path: None,
            auto_start: true,
            dm_policy: "open".to_owned(),
            group_policy: "allowlist".to_owned(),
            require_mention: true,
            send_read_receipts: true,
            text_chunk_limit: 2000,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct RetentionConfig {
    /// Max age for closed sessions (days).
    pub session_max_age_days: u32,
    /// Max age for orphaned messages (days).
    pub orphan_message_max_age_days: u32,
    /// Max sessions to retain per nous (0 = unlimited).
    pub max_sessions_per_nous: u32,
    /// Archive sessions to JSON before deletion.
    pub archive_before_delete: bool,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            session_max_age_days: 90,
            orphan_message_max_age_days: 30,
            max_sessions_per_nous: 0,
            archive_before_delete: true,
        }
    }
}
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
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            enforcement: SandboxEnforcementMode::Enforcing,
            extra_read_paths: Vec::new(),
            extra_write_paths: Vec::new(),
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
    /// Maximum consecutive tool use iterations per turn.
    pub max_tool_iterations: u32,
    /// Resolved workspace directory path.
    pub workspace: String,
    /// Merged set of permitted filesystem roots.
    pub allowed_roots: Vec<String>,
    /// Knowledge domains this agent covers.
    pub domains: Vec<String>,
    /// IANA timezone for prompt formatting.
    pub user_timezone: String,
    /// Per-turn timeout in seconds.
    pub timeout_seconds: u32,
    /// Whether prompt caching is enabled.
    pub cache_enabled: bool,
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

    let thinking_enabled = agent
        .and_then(|a| a.thinking_enabled)
        .unwrap_or(defaults.thinking_enabled);

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
        max_tool_iterations: defaults.max_tool_iterations,
        workspace,
        allowed_roots,
        domains,
        user_timezone: defaults.user_timezone.clone(),
        timeout_seconds: defaults.timeout_seconds,
        cache_enabled: defaults.caching.enabled && defaults.caching.strategy != "disabled",
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
