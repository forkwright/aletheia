//! Configuration types for an Aletheia instance.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Root configuration for an Aletheia instance.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct AletheiaConfig {
    pub agents: AgentsConfig,
    pub gateway: GatewayConfig,
    pub channels: ChannelsConfig,
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
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            port: 18789,
            bind: "lan".to_owned(),
            auth: GatewayAuthConfig::default(),
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
#[expect(clippy::struct_excessive_bools, reason = "mirrors TS config schema 1:1")]
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
        assert!(config.channels.signal.enabled);
        assert!(config.channels.signal.accounts.is_empty());
    }

    #[test]
    fn serde_roundtrip() {
        let config = AletheiaConfig::default();
        let json = serde_json::to_string(&config).expect("serialize");
        let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.agents.defaults.context_tokens, 200_000);
        assert_eq!(back.gateway.port, 18789);
        assert!(back.channels.signal.enabled);
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
}
