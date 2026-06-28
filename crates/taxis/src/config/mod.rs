//! Configuration types for an Aletheia instance.

mod agents;
mod behavior;
mod feature_flags;
mod gateway;
mod maintenance;
mod resolved;
mod tools;

pub use agents::{
    AgentBehaviorDefaults, AgentDefaults, AgentModelDefaults, AgentToolGroupPolicy, AgentsConfig,
    CachingConfig, ModelSpec, NousDefinition, RecallProfile, RecallSettings, RecallWeights,
};
pub use behavior::{
    AdmissionPolicyKind, AnthropicConfig, ApiLimitsConfig, BookkeepingProviderKind, CapacityConfig,
    CompactionStrategyKind, CronTaskConfig, DaemonBehaviorConfig, DeploymentTarget, DispatchConfig,
    DispatchSpecConfig, ExtractionConfig, JwtSettings, KnowledgeConfig, LlmProviderConfig,
    MessagingConfig, NousBehaviorConfig, OpenAiApiFamily, PromptCacheMode, ProviderBehaviorConfig,
    ProviderKind, RetrySettings, TimeoutsConfig, ToolLimitsConfig, TuningConfig,
};
pub use feature_flags::FeatureFlagConfig;
pub use gateway::{
    BodyLimitConfig, CorsConfig, CsrfConfig, GatewayAuthConfig, GatewayConfig,
    PerUserRateLimitConfig, RateLimitConfig, TlsConfig,
};
pub use maintenance::{
    BackupSettings, CircuitBreakerSettings, CredentialConfig, CronTaskEntry, CronTaskSettings,
    DbMonitoringSettings, DiskSpaceSettings, DriftDetectionSettings, KnowledgeGraphMcpConfig,
    LoggingSettings, MaintenanceSettings, McpConfig, McpRateLimitConfig, PromptAuditSettings,
    ProsocheActiveWindowSettings, ProsocheExternalTimerSettings, ProsocheMaintenanceSettings,
    ProsocheScheduleMode, ProsocheTaskScheduleSettings, RedactionSettings, RepomixMcpConfig,
    RetentionSettings, SandboxSettings, SerendipityMaintenanceSettings, TraceRotationSettings,
    WatchdogSettings,
};
pub use resolved::{
    AgentCapabilities, ResolvedModelConfig, ResolvedNousConfig, TokenLimits, resolve_nous,
};
pub use tools::{
    ExternalToolAuth, ExternalToolEntry, ExternalToolGroupId, ExternalToolKind, ExternalToolMethod,
    ExternalToolReversibility, ExternalToolsConfig,
};

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Runtime observability feature toggles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
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

/// Gateway workspace file-API configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSettings {
    /// Root directory served by the workspace file API.
    ///
    /// Relative paths resolve against the instance root. When unset, the
    /// gateway serves the instance theke directory if present, then the
    /// shared agent workspace, then the instance root.
    pub root: Option<PathBuf>,
}

/// Data lifecycle configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
pub struct DataConfig {
    /// Retention policy mirrored into `maintenance.retention` for compatibility.
    pub retention: RetentionSettings,
}

impl DataConfig {
    fn is_default(value: &Self) -> bool {
        value.retention == RetentionSettings::default()
    }
}

/// Root configuration for an Aletheia instance.
// kanon:ignore RUST/no-debug-derive-on-public-types
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
#[rustfmt::skip]
pub struct AletheiaConfig {
    /// Agent definitions and shared defaults.
    pub agents: AgentsConfig,
    /// HTTP gateway settings (port, bind address, auth, TLS, CORS).
    pub gateway: GatewayConfig,
    /// Workspace file-API root override.
    ///
    /// WHY configurable: the desktop Theke view browses one directory tree;
    /// deployments choose which tree the gateway exposes (theke vault, agent
    /// workspace, ...) without code changes.
    pub workspace: WorkspaceSettings,
    /// Runtime data lifecycle settings.
    #[serde(skip_serializing_if = "DataConfig::is_default")]
    pub data: DataConfig,
    /// Messaging transport configuration (Signal, etc.).
    pub channels: ChannelsConfig,
    /// Routes mapping channel sources to nous agents.
    pub bindings: Vec<ChannelBinding>,
    /// Operator-controlled feature toggles surfaced through the config API.
    #[serde(rename = "feature_flags")]
    #[serde(default)]
    pub feature_flags: Vec<FeatureFlagConfig>,
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
    /// Ordered list of backends — the provider registry prefers the highest
    /// model-match specificity and breaks equal-specificity ties by this
    /// order. Empty by default for backward compatibility: when empty, the
    /// runtime falls back to the
    /// legacy single-Anthropic setup driven by [`Self::anthropic`] and the
    /// top-level credential chain. Once populated, this list is the complete
    /// provider-ordering contract. An `anthropic` entry without `apiKeyEnv`
    /// uses the top-level credential chain at that entry's declared position.
    /// Populate this to enable OpenAI-compatible endpoints (local
    /// llama.cpp/ollama/vllm, other cloud APIs) or to declare explicit
    /// deployment targets for the factsensitivity filter.
    #[serde(default)]
    pub providers: Vec<LlmProviderConfig>,
    /// Prompt audit log: operator visibility into outbound LLM requests (#3411).
    ///
    /// WHY configurable: operators can disable the log or tune retention and
    /// filtered-ID inclusion. Default is on with 90-day retention because
    /// the log is a sovereignty feature — operators should be able to see
    /// what the system sent out without opting in.
    pub prompt_audit: PromptAuditSettings,
    /// Runtime-bridged external tools (HTTP proxies and MCP clients).
    ///
    /// WHY configurable: deployments expose different external capabilities;
    /// declaring them in config lets agents adapt without rebuilding the
    /// binary. Owned by taxis so the section participates in the config
    /// cascade, secret handling, and validation.
    #[serde(default)]
    pub tools: ExternalToolsConfig,
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
// kanon:ignore RUST/no-debug-derive-on-public-types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelBinding {
    /// Channel type (e.g., "signal").
    pub channel: String,
    /// Source pattern: phone number, group ID, or "*" for default.
    pub source: String,
    /// Nous ID to route to.
    // kanon:ignore RUST/primitive-for-domain-id — wire/serde config field: nous_id is a TOML routing string, not a runtime domain identifier
    pub nous_id: String,
    /// Session key pattern. Supports `{source}` and `{group}` placeholders.
    #[serde(default = "default_session_pattern")]
    // kanon:ignore RUST/plain-string-secret
    pub session_key: String,
}

fn default_session_pattern() -> String {
    "{source}".to_owned()
}

/// Embedding provider configuration for recall pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
#[rustfmt::skip]
pub struct EmbeddingSettings {
    /// Provider type: "candle", "openai-compat", "voyage".
    pub provider: String,
    /// Provider-specific model name.
    pub model: Option<String>,
    /// Output vector dimension (must match knowledge store HNSW index).
    pub dimension: usize,
    /// OpenAI-compatible embedding endpoint base URL.
    #[serde(alias = "baseurl")]
    pub base_url: Option<String>,
    /// Environment variable that stores the embedding provider API key.
    #[serde(alias = "apikeyenv")]
    pub api_key_env: Option<String>,
}

impl Default for EmbeddingSettings {
    fn default() -> Self {
        Self {
            provider: "candle".to_owned(),
            model: None,
            dimension: 384,
            base_url: None,
            api_key_env: None,
        }
    }
}

impl EmbeddingSettings {
    /// Convert public TOML settings into the embedding provider config shape
    /// without resolving secrets from the process environment.
    #[must_use]
    pub fn to_embedding_config(&self) -> episteme::embedding::EmbeddingConfig {
        self.to_embedding_config_with_api_key(None)
    }

    /// Convert public TOML settings into the embedding provider config shape.
    #[must_use]
    pub fn to_embedding_config_with_api_key(
        &self,
        api_key: Option<koina::secret::SecretString>,
    ) -> episteme::embedding::EmbeddingConfig {
        episteme::embedding::EmbeddingConfig {
            provider: self.provider.clone(),
            model: self.model.clone(),
            dimension: Some(self.dimension),
            api_key,
            base_url: self.base_url.clone(),
        }
    }
}

/// Channel configuration (messaging transports).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
#[rustfmt::skip]
pub struct ChannelsConfig {
    /// Signal messenger transport configuration.
    pub signal: SignalConfig,
    /// Matrix messenger transport configuration.
    pub matrix: MatrixConfig,
}

/// Signal messenger channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
#[rustfmt::skip]
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
#[serde(deny_unknown_fields)]
#[rustfmt::skip]
pub struct SignalAccountConfig {
    /// Operator-facing display label for this account.
    pub name: Option<String>,
    /// Whether this account is active.
    pub enabled: bool,
    /// Signal account phone number used for signal-cli JSON-RPC calls.
    pub account: Option<String>,
    /// Hostname for the signal-cli JSON-RPC HTTP interface.
    #[serde(alias = "http_host")]
    pub http_host: String,
    /// Port for the signal-cli JSON-RPC HTTP interface.
    #[serde(alias = "http_port")]
    pub http_port: u16,
    /// Optional path to the signal-cli binary for startup diagnostics.
    #[serde(alias = "cli_path")]
    pub cli_path: Option<PathBuf>,
    /// Whether to auto-start the receive loop for this account on server boot.
    #[serde(alias = "auto_start")]
    pub auto_start: bool,
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
        }
    }
}

/// Matrix messenger channel configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
#[rustfmt::skip]
pub struct MatrixConfig {
    /// Whether the Matrix channel is active.
    pub enabled: bool,
    /// Named Matrix accounts keyed by account label.
    pub accounts: HashMap<String, MatrixAccountConfig>,
}

/// Configuration for a single Matrix account.
// kanon:ignore RUST/no-debug-derive-on-public-types — MatrixAccountConfig holds only homeserver URL and env-var name, not actual tokens; derived Debug leaks no secrets
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
#[serde(deny_unknown_fields)]
#[rustfmt::skip]
pub struct MatrixAccountConfig {
    /// Whether this account is active.
    pub enabled: bool,
    /// Matrix homeserver base URL, e.g. `https://matrix.example.org`.
    pub homeserver: String,
    /// Environment variable that contains the Matrix access token.
    // kanon:ignore RUST/plain-string-secret
    #[serde(alias = "access_token_env")]
    pub access_token_env: String,
    /// Matrix user ID for this account. Used to ignore echoed self messages.
    #[serde(alias = "user_id")]
    pub user_id: Option<String>,
    /// Whether to auto-start the `/sync` receive loop on server boot.
    #[serde(alias = "auto_start")]
    pub auto_start: bool,
    /// Optional initial `/sync` since token.
    #[serde(alias = "initial_since")]
    pub initial_since: Option<String>,
}

impl Default for MatrixAccountConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            homeserver: String::new(),
            access_token_env: String::new(),
            user_id: None,
            auto_start: true,
            initial_since: None,
        }
    }
}

#[cfg(test)]
#[path = "config_tests/mod.rs"]
mod tests;
