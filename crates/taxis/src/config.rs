//! Configuration types for an Aletheia instance.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Root configuration for an Aletheia instance.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AletheiaConfig {
    pub agents: AgentsConfig,
    pub gateway: GatewayConfig,
    pub channels: ChannelsConfig,
    pub bindings: Vec<ChannelBinding>,
    pub embedding: EmbeddingSettings,
    pub data: DataConfig,
    /// External domain pack paths (directories containing pack.yaml).
    pub packs: Vec<PathBuf>,
    pub maintenance: MaintenanceSettings,
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
    pub defaults: AgentDefaults,
    pub list: Vec<NousDefinition>,
}

/// Default values applied to every agent unless overridden.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AgentDefaults {
    pub model: ModelSpec,
    pub context_tokens: u32,
    pub max_output_tokens: u32,
    pub bootstrap_max_tokens: u32,
    pub user_timezone: String,
    pub timeout_seconds: u32,
    pub thinking_enabled: bool,
    pub thinking_budget: u32,
    pub max_tool_iterations: u32,
    pub allowed_roots: Vec<String>,
    pub tool_timeouts: ToolTimeouts,
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
        }
    }
}

/// Model specification with primary model and fallbacks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct ModelSpec {
    pub primary: String,
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
    pub default_ms: u64,
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

/// Definition of a single nous agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NousDefinition {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub model: Option<ModelSpec>,
    pub workspace: String,
    #[serde(default)]
    pub thinking_enabled: Option<bool>,
    #[serde(default)]
    pub allowed_roots: Vec<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub default: bool,
}

/// HTTP gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct GatewayConfig {
    pub port: u16,
    pub bind: String,
    pub auth: GatewayAuthConfig,
    pub tls: TlsConfig,
    pub cors: CorsConfig,
    pub body_limit: BodyLimitConfig,
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
    pub mode: String,
}

impl Default for GatewayAuthConfig {
    fn default() -> Self {
        Self {
            mode: "token".to_owned(),
        }
    }
}

/// TLS termination configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct TlsConfig {
    pub enabled: bool,
    pub cert_path: Option<String>,
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
    pub enabled: bool,
    pub header_name: String,
    pub header_value: String,
}

impl Default for CsrfConfig {
    fn default() -> Self {
        Self {
            enabled: false,
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
    /// Provider type: "mock", "fastembed".
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
    pub signal: SignalConfig,
}

/// Signal messenger channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct SignalConfig {
    pub enabled: bool,
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
    pub name: Option<String>,
    pub enabled: bool,
    pub account: Option<String>,
    pub http_host: String,
    pub http_port: u16,
    pub cli_path: Option<String>,
    pub auto_start: bool,
    pub dm_policy: String,
    pub group_policy: String,
    pub require_mention: bool,
    pub send_read_receipts: bool,
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
    pub trace_rotation: TraceRotationSettings,
    pub drift_detection: DriftDetectionSettings,
    pub db_monitoring: DbMonitoringSettings,
    pub retention: RetentionSettings,
}

/// Trace file rotation settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct TraceRotationSettings {
    pub enabled: bool,
    pub max_age_days: u32,
    pub max_total_size_mb: u64,
    pub compress: bool,
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
    pub enabled: bool,
    pub alert_on_missing: bool,
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
    pub enabled: bool,
    pub warn_threshold_mb: u64,
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
    pub enabled: bool,
}

/// Resolved configuration for a specific nous agent.
///
/// Produced by merging [`AgentDefaults`] with a matching [`NousDefinition`].
#[derive(Debug, Clone)]
pub struct ResolvedNousConfig {
    pub id: String,
    pub name: Option<String>,
    pub model: String,
    pub fallbacks: Vec<String>,
    pub context_tokens: u32,
    pub max_output_tokens: u32,
    pub bootstrap_max_tokens: u32,
    pub thinking_enabled: bool,
    pub thinking_budget: u32,
    pub max_tool_iterations: u32,
    pub workspace: String,
    pub allowed_roots: Vec<String>,
    pub domains: Vec<String>,
    pub user_timezone: String,
    pub timeout_seconds: u32,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let config = AletheiaConfig::default();
        assert_eq!(config.agents.defaults.context_tokens, 200_000);
        assert_eq!(config.agents.defaults.max_output_tokens, 16_384);
        assert_eq!(config.agents.defaults.bootstrap_max_tokens, 40_000);
        assert_eq!(config.agents.defaults.model.primary, "claude-sonnet-4-6");
        assert_eq!(config.agents.defaults.user_timezone, "UTC");
        assert_eq!(config.agents.defaults.timeout_seconds, 300);
        assert!(!config.agents.defaults.thinking_enabled);
        assert_eq!(config.agents.defaults.thinking_budget, 10_000);
        assert_eq!(config.agents.defaults.max_tool_iterations, 50);
        assert_eq!(config.agents.defaults.tool_timeouts.default_ms, 120_000);
        assert_eq!(config.gateway.port, 18789);
        assert_eq!(config.gateway.bind, "lan");
        assert_eq!(config.gateway.auth.mode, "token");
        // Security config defaults
        assert!(!config.gateway.tls.enabled);
        assert!(config.gateway.tls.cert_path.is_none());
        assert!(config.gateway.cors.allowed_origins.is_empty());
        assert_eq!(config.gateway.cors.max_age_secs, 3600);
        assert_eq!(config.gateway.body_limit.max_bytes, 1_048_576);
        assert!(!config.gateway.csrf.enabled);
        assert_eq!(config.gateway.csrf.header_name, "x-requested-with");
        assert_eq!(config.gateway.csrf.header_value, "aletheia");
        assert!(config.channels.signal.enabled);
        assert!(config.channels.signal.accounts.is_empty());
        assert!(config.bindings.is_empty());
        assert_eq!(config.embedding.provider, "mock");
        assert!(config.embedding.model.is_none());
        assert_eq!(config.embedding.dimension, 384);
        // Maintenance defaults
        assert!(config.maintenance.trace_rotation.enabled);
        assert_eq!(config.maintenance.trace_rotation.max_age_days, 14);
        assert!(config.maintenance.drift_detection.enabled);
        assert!(config.maintenance.db_monitoring.enabled);
        assert_eq!(config.maintenance.db_monitoring.warn_threshold_mb, 100);
        assert!(!config.maintenance.retention.enabled);
    }

    #[test]
    fn serde_roundtrip() {
        let config = AletheiaConfig::default();
        let json = serde_json::to_string(&config).expect("serialize");
        let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.agents.defaults.context_tokens, 200_000);
        assert_eq!(back.gateway.port, 18789);
        assert!(back.channels.signal.enabled);
        assert_eq!(back.embedding.provider, "mock");
        assert_eq!(back.embedding.dimension, 384);
    }

    #[test]
    fn minimal_yaml_parses() {
        let yaml = r#"{"agents": {"list": []}}"#;
        let config: AletheiaConfig = serde_json::from_str(yaml).expect("parse minimal");
        assert_eq!(config.agents.defaults.context_tokens, 200_000);
        assert!(config.agents.list.is_empty());
        assert_eq!(config.gateway.port, 18789);
    }

    #[test]
    fn camel_case_compat() {
        let yaml = r#"{
            "agents": {
                "defaults": {
                    "contextTokens": 100000,
                    "maxOutputTokens": 8192,
                    "bootstrapMaxTokens": 20000,
                    "userTimezone": "America/New_York",
                    "toolTimeouts": {
                        "defaultMs": 60000
                    }
                },
                "list": []
            }
        }"#;
        let config: AletheiaConfig = serde_json::from_str(yaml).expect("parse camelCase");
        assert_eq!(config.agents.defaults.context_tokens, 100_000);
        assert_eq!(config.agents.defaults.max_output_tokens, 8192);
        assert_eq!(config.agents.defaults.bootstrap_max_tokens, 20_000);
        assert_eq!(config.agents.defaults.user_timezone, "America/New_York");
        assert_eq!(config.agents.defaults.tool_timeouts.default_ms, 60_000);
    }

    #[test]
    fn resolve_uses_defaults_for_unknown_agent() {
        let config = AletheiaConfig::default();
        let resolved = resolve_nous(&config, "unknown-agent");
        assert_eq!(resolved.id, "unknown-agent");
        assert_eq!(resolved.model, "claude-sonnet-4-6");
        assert_eq!(resolved.context_tokens, 200_000);
        assert!(!resolved.thinking_enabled);
        assert_eq!(resolved.workspace, "instance/nous/unknown-agent");
        assert!(resolved.domains.is_empty());
    }

    #[test]
    fn resolve_merges_agent_overrides() {
        let mut config = AletheiaConfig::default();
        config.agents.list.push(NousDefinition {
            id: "syn".to_owned(),
            name: Some("Synthetic".to_owned()),
            model: Some(ModelSpec {
                primary: "claude-opus-4-6".to_owned(),
                fallbacks: vec!["claude-sonnet-4-6".to_owned()],
            }),
            workspace: "/home/user/nous/syn".to_owned(),
            thinking_enabled: None,
            allowed_roots: Vec::new(),
            domains: vec!["code".to_owned()],
            default: false,
        });

        let resolved = resolve_nous(&config, "syn");
        assert_eq!(resolved.model, "claude-opus-4-6");
        assert_eq!(resolved.fallbacks, vec!["claude-sonnet-4-6"]);
        assert_eq!(resolved.name, Some("Synthetic".to_owned()));
        assert_eq!(resolved.workspace, "/home/user/nous/syn");
        assert_eq!(resolved.domains, vec!["code"]);
        assert!(!resolved.thinking_enabled);
    }

    #[test]
    fn resolve_merges_allowed_roots() {
        let mut config = AletheiaConfig::default();
        config.agents.defaults.allowed_roots = vec!["/shared".to_owned()];
        config.agents.list.push(NousDefinition {
            id: "syn".to_owned(),
            name: None,
            model: None,
            workspace: "/home/user/nous/syn".to_owned(),
            thinking_enabled: None,
            allowed_roots: vec!["/extra".to_owned(), "/shared".to_owned()],
            domains: Vec::new(),
            default: false,
        });

        let resolved = resolve_nous(&config, "syn");
        assert_eq!(resolved.allowed_roots, vec!["/shared", "/extra"]);
    }

    #[test]
    fn resolve_thinking_override() {
        let mut config = AletheiaConfig::default();
        config.agents.list.push(NousDefinition {
            id: "thinker".to_owned(),
            name: None,
            model: None,
            workspace: "/home/user/nous/thinker".to_owned(),
            thinking_enabled: Some(true),
            allowed_roots: Vec::new(),
            domains: Vec::new(),
            default: false,
        });

        let resolved = resolve_nous(&config, "thinker");
        assert!(resolved.thinking_enabled);
    }

    #[test]
    fn signal_account_defaults() {
        let account = SignalAccountConfig::default();
        assert!(account.enabled);
        assert_eq!(account.http_host, "localhost");
        assert_eq!(account.http_port, 8080);
        assert!(account.auto_start);
        assert_eq!(account.dm_policy, "open");
        assert_eq!(account.group_policy, "allowlist");
        assert!(account.require_mention);
        assert!(account.send_read_receipts);
        assert_eq!(account.text_chunk_limit, 2000);
    }

    #[test]
    fn channel_binding_serde_roundtrip() {
        let json = r#"{
            "channel": "signal",
            "source": "+1234567890",
            "nousId": "syn"
        }"#;
        let binding: ChannelBinding = serde_json::from_str(json).expect("parse binding");
        assert_eq!(binding.channel, "signal");
        assert_eq!(binding.source, "+1234567890");
        assert_eq!(binding.nous_id, "syn");
        assert_eq!(binding.session_key, "{source}");
    }

    #[test]
    fn embedding_override_from_json() {
        let json = r#"{
            "embedding": {
                "provider": "fastembed",
                "model": "BAAI/bge-small-en-v1.5",
                "dimension": 512
            }
        }"#;
        let config: AletheiaConfig = serde_json::from_str(json).expect("parse embedding");
        assert_eq!(config.embedding.provider, "fastembed");
        assert_eq!(
            config.embedding.model,
            Some("BAAI/bge-small-en-v1.5".to_owned())
        );
        assert_eq!(config.embedding.dimension, 512);
    }

    #[test]
    fn bindings_in_config_default_empty() {
        let config = AletheiaConfig::default();
        assert!(config.bindings.is_empty());
    }
}
