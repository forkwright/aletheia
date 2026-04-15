//! Configuration types for an Aletheia instance.

mod agents;
mod behavior;
mod gateway;
mod maintenance;
mod resolved;

pub use agents::{
    AgentBehaviorDefaults, AgentDefaults, AgentModelDefaults, AgentsConfig,
    CachingConfig, ModelSpec, NousDefinition, RecallEngineWeights, RecallSettings,
    RecallWeights,
};
pub use behavior::{
    ApiLimitsConfig, CapacityConfig, DaemonBehaviorConfig, KnowledgeConfig,
    MessagingConfig, NousBehaviorConfig, ProviderBehaviorConfig, RetrySettings,
    TimeoutsConfig, ToolLimitsConfig, TuningConfig,
};
pub use gateway::{
    BodyLimitConfig, CorsConfig, CsrfConfig, GatewayAuthConfig, GatewayConfig,
    PerUserRateLimitConfig, RateLimitConfig, TlsConfig,
};
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
    /// Self-tuning feedback loop configuration.
    pub tuning: TuningConfig,
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


#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
