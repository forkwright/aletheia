//! Configuration types for an Aletheia instance.

mod agents;
mod behavior;
mod gateway;
mod maintenance;
mod resolved;

pub use agents::{
    AgentBehaviorDefaults, AgentDefaults, AgentModelDefaults, AgentsConfig, CachingConfig,
    ModelSpec, NousDefinition, RecallProfile, RecallSettings, RecallWeights,
};
pub use behavior::{
    AnthropicConfig, ApiLimitsConfig, BookkeepingProviderKind, CapacityConfig, CronTaskConfig,
    DaemonBehaviorConfig, DeploymentTarget, DispatchConfig, DispatchSpecConfig, ExtractionConfig,
    JwtSettings, KnowledgeConfig, LlmProviderConfig, MessagingConfig, NousBehaviorConfig,
    OpenAiApiFamily, PromptCacheMode, ProviderBehaviorConfig, ProviderKind, RetrySettings,
    TimeoutsConfig, ToolLimitsConfig, TuningConfig,
};
pub use gateway::{
    BodyLimitConfig, CorsConfig, CsrfConfig, GatewayAuthConfig, GatewayConfig,
    PerUserRateLimitConfig, RateLimitConfig, TlsConfig,
};
pub use maintenance::{
    CircuitBreakerSettings, CredentialConfig, CronTaskEntry, CronTaskSettings,
    DbMonitoringSettings, DiskSpaceSettings, DriftDetectionSettings, KnowledgeGraphMcpConfig,
    LoggingSettings, MaintenanceSettings, McpConfig, McpRateLimitConfig, PromptAuditSettings,
    RedactionSettings, RepomixMcpConfig, RetentionSettings, SandboxSettings, TraceRotationSettings,
    WatchdogSettings,
};
pub use resolved::{
    AgentCapabilities, ResolvedModelConfig, ResolvedNousConfig, TokenLimits, resolve_nous,
};

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Runtime observability feature toggles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ObservabilitySettings {
    /// Install the episteme trace-ingest subscriber layer and flush ops facts
    /// into the knowledge store. Default: true.
    #[serde(alias = "trace_ingest")]
    pub trace_ingest: bool,
}

impl Default for ObservabilitySettings {
    fn default() -> Self {
        Self { trace_ingest: true }
    }
}

/// Root configuration for an Aletheia instance.
#[derive(Debug, Default, Clone, Serialize, Deserialize)] // kanon:ignore RUST/no-debug-derive-on-public-types
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[rustfmt::skip]
pub struct AletheiaConfig { // kanon:ignore RUST/config-deny-unknown-fields
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
    /// Runtime observability feature toggles.
    pub observability: ObservabilitySettings,
    /// MCP server configuration.
    pub mcp: McpConfig,
    /// Training data capture configuration.
    pub training: eidos::training::TrainingConfig,
    /// Deployment-tunable timeout thresholds.
    ///
    /// WHY configurable: LLM call timeouts vary by provider and network
    /// conditions; operators running behind proxies or on slow links need to
    /// adjust without code changes.
    pub timeouts: TimeoutsConfig,
    /// Deployment-tunable capacity limits for tool output and context windows.
    ///
    /// WHY configurable: tool output truncation and Opus context upgrade
    /// thresholds depend on host hardware and model provider limits.
    pub capacity: CapacityConfig,
    /// Deployment-tunable LLM retry and backoff parameters.
    ///
    /// WHY configurable: retry aggressiveness must adapt to provider SLAs and
    /// cost constraints; operators may want zero retries in latency-sensitive
    /// deployments or more retries behind rate-limited providers.
    pub retry: RetrySettings,
    /// Nous actor/manager health, restart, GC, and loop-detection settings.
    ///
    /// WHY configurable: actor inbox sizes, session caps, and health poll
    /// intervals depend on workload characteristics and host resources.
    pub nous_behavior: NousBehaviorConfig,
    /// Episteme conflict resolution, decay, and extraction parameters.
    ///
    /// WHY configurable: knowledge extraction thresholds and conflict
    /// resolution aggressiveness vary by deployment use case (research vs
    /// production, single-agent vs multi-agent).
    pub knowledge: KnowledgeConfig,
    /// Hermeneus provider timeout, concurrency, and complexity routing thresholds.
    ///
    /// WHY configurable: non-streaming timeouts and concurrency limits depend
    /// on provider rate limits and latency characteristics. Complexity
    /// thresholds control model routing (Haiku vs Opus) which affects cost.
    pub provider_behavior: ProviderBehaviorConfig,
    /// Pylon request size and idempotency limits.
    ///
    /// WHY configurable: API body size limits, idempotency cache capacity,
    /// and history pagination defaults vary by deployment scale and client
    /// requirements.
    pub api_limits: ApiLimitsConfig,
    /// Daemon watchdog, prosoche, and runner output settings.
    ///
    /// WHY configurable: watchdog backoff and anomaly detection sensitivity
    /// depend on system stability requirements and agent workload patterns.
    pub daemon_behavior: DaemonBehaviorConfig,
    /// Recurring dispatch task configuration (cron-scheduled prompt runs).
    #[serde(default)]
    pub dispatch: DispatchConfig,
    /// Organon tool size and timeout limits.
    ///
    /// WHY configurable: filesystem write caps, subprocess timeouts, and
    /// message size limits must match the deployment's security posture and
    /// resource constraints.
    pub tool_limits: ToolLimitsConfig,
    /// Agora messaging transport poll, buffer, and circuit-breaker settings.
    ///
    /// WHY configurable: poll intervals and buffer sizes depend on channel
    /// message volume; circuit-breaker thresholds must balance reliability
    /// against false positives in flaky network conditions.
    pub messaging: MessagingConfig,
    /// Self-tuning feedback loop configuration.
    ///
    /// WHY configurable: tuning is disabled by default (experimental). The
    /// global kill switch and evidence thresholds let operators enable and
    /// tune the feedback loop incrementally.
    pub tuning: TuningConfig,
    /// Anthropic-specific sovereignty and privacy settings (#3410, #3406, #3409).
    ///
    /// WHY configurable: prompt caching stores operator system prompts on
    /// Anthropic servers. The default (`disabled`) is sovereignty-first;
    /// operators who accept the tradeoff may opt in to reduce per-turn token
    /// cost.
    pub anthropic: AnthropicConfig,
    /// JWT validation tuning (clock-skew leeway, etc.).
    ///
    /// WHY configurable: clock drift between issuer and validator can
    /// immediately invalidate fresh tokens. Default 30s leeway tolerates
    /// typical NTP drift; operators on tightly synchronized hosts may
    /// lower this, and those behind mis-synced proxies may raise it.
    pub jwt: JwtSettings,
    /// LLM provider definitions (#3424, #3414).
    ///
    /// Ordered list of backends — the provider registry routes each request
    /// to the first entry that claims the requested model. Empty by default
    /// for backward compatibility: when empty, the runtime falls back to the
    /// legacy single-Anthropic setup driven by [`Self::anthropic`] and the
    /// top-level credential chain. Populate this to enable OpenAI-compatible
    /// endpoints (local llama.cpp/ollama/vllm, other cloud APIs) or to
    /// declare explicit deployment targets for the factsensitivity filter.
    #[serde(default)]
    pub providers: Vec<LlmProviderConfig>,
    /// Prompt audit log: operator visibility into outbound LLM requests (#3411).
    ///
    /// WHY configurable: operators can disable the log or tune retention and
    /// filtered-ID inclusion. Default is on with 90-day retention because
    /// the log is a sovereignty feature — operators should be able to see
    /// what the system sent out without opting in.
    pub prompt_audit: PromptAuditSettings,
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
#[derive(Debug, Clone, Serialize, Deserialize)] // kanon:ignore RUST/no-debug-derive-on-public-types
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
    pub session_key: String, // kanon:ignore RUST/plain-string-secret
}

fn default_session_pattern() -> String {
    "{source}".to_owned()
}

/// Embedding provider configuration for recall pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[rustfmt::skip]
pub struct EmbeddingSettings { // kanon:ignore RUST/config-deny-unknown-fields
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
#[rustfmt::skip]
pub struct ChannelsConfig { // kanon:ignore RUST/config-deny-unknown-fields
    /// Signal messenger transport configuration.
    pub signal: SignalConfig,
}

/// Signal messenger channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[rustfmt::skip]
pub struct SignalConfig { // kanon:ignore RUST/config-deny-unknown-fields
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
#[rustfmt::skip]
pub struct SignalAccountConfig { // kanon:ignore RUST/config-deny-unknown-fields
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
#[path = "config_tests/mod.rs"]
mod tests;
