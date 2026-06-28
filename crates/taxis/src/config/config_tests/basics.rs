//! Config defaults, serde round-trips, and per-agent resolution merging.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: map/vec indexing with keys/indices asserted present by surrounding context"
)]

use std::sync::Arc;

use super::super::*;

#[test]
#[expect(clippy::too_many_lines, reason = "full config default validation")]
fn defaults_are_sensible() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.agents.defaults.model_defaults.context_tokens, 200_000,
        "default context tokens should be 200k"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.max_output_tokens, 16_384,
        "default max output tokens should be 16384"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.bootstrap_max_tokens, 40_000,
        "default bootstrap max tokens should be 40k"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.model.primary, "claude-sonnet-4-6",
        "default primary model should be sonnet"
    );
    assert!(
        !config.agents.defaults.model_defaults.thinking_enabled,
        "thinking should be disabled by default"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.thinking_budget, 10_000,
        "default thinking budget should be 10k"
    );
    assert_eq!(
        config.agents.defaults.max_tool_iterations, 200,
        "default max tool iterations should be 200"
    );
    assert_eq!(
        config.gateway.port, 18789,
        "default gateway port should be 18789"
    );
    assert_eq!(
        config.gateway.bind, "localhost",
        "default gateway bind should be localhost"
    );
    assert_eq!(
        config.gateway.auth.mode, "token",
        "default auth mode should be token"
    );
    assert!(
        !config.gateway.tls.enabled,
        "tls should be disabled by default"
    );
    assert!(
        config.gateway.tls.cert_path.is_none(),
        "tls cert path should be absent by default"
    );
    assert!(
        config.gateway.cors.allowed_origins.is_empty(),
        "cors allowed origins should be empty by default"
    );
    assert_eq!(
        config.gateway.cors.max_age_secs, 3600,
        "default cors max age should be 3600 seconds"
    );
    assert_eq!(
        config.gateway.body_limit.max_bytes, 1_048_576,
        "default body limit should be 1 MiB"
    );
    assert!(
        config.gateway.csrf.enabled,
        "csrf should be enabled by default"
    );
    assert!(
        !config.gateway.csrf.disable_acknowledged,
        "csrf disable acknowledgement should be false by default"
    );
    assert_eq!(
        config.gateway.csrf.header_name, "x-requested-with",
        "default csrf header name should be x-requested-with"
    );
    assert_eq!(
        config.gateway.csrf.header_value.expose_secret(),
        "aletheia",
        "default csrf header value should be aletheia"
    );
    assert!(
        !config.gateway.rate_limit.enabled,
        "rate limiting should be disabled by default"
    );
    assert_eq!(
        config.gateway.rate_limit.requests_per_minute, 60,
        "default rate limit should be 60 rpm"
    );
    assert!(
        !config.gateway.rate_limit.trust_proxy,
        "trust_proxy should default to false to prevent IP spoofing bypasses"
    );
    assert!(
        config.channels.signal.enabled,
        "signal channel should be enabled by default"
    );
    assert!(
        config.channels.signal.accounts.is_empty(),
        "signal accounts should be empty by default"
    );
    assert!(
        !config.channels.matrix.enabled,
        "matrix channel should be disabled by default"
    );
    assert!(
        config.channels.matrix.accounts.is_empty(),
        "matrix accounts should be empty by default"
    );
    assert!(
        config.bindings.is_empty(),
        "bindings should be empty by default"
    );
    assert_eq!(
        config.embedding.provider, "candle",
        "default embedding provider should be candle"
    );
    assert!(
        config.embedding.model.is_none(),
        "embedding model should be unset by default"
    );
    assert_eq!(
        config.embedding.dimension, 384,
        "default embedding dimension should be 384"
    );
    assert!(
        config.maintenance.trace_rotation.enabled,
        "trace rotation should be enabled by default"
    );
    assert_eq!(
        config.maintenance.trace_rotation.max_age_days, 14,
        "default trace rotation max age should be 14 days"
    );
    assert!(
        config.maintenance.drift_detection.enabled,
        "drift detection should be enabled by default"
    );
    assert!(
        config.maintenance.db_monitoring.enabled,
        "db monitoring should be enabled by default"
    );
    assert_eq!(
        config.maintenance.db_monitoring.warn_threshold_mb, 100,
        "default db monitoring warn threshold should be 100 MiB"
    );
    assert!(
        !config.maintenance.retention.enabled,
        "retention should be disabled by default"
    );
    assert!(
        config.pricing.is_empty(),
        "pricing map should be empty by default"
    );
    assert!(
        config.mcp.rate_limit.enabled,
        "mcp rate limit should be enabled by default"
    );
    assert_eq!(
        config.mcp.rate_limit.message_requests_per_minute, 60,
        "default mcp message rate limit should be 60 rpm"
    );
    assert_eq!(
        config.mcp.rate_limit.read_requests_per_minute, 300,
        "default mcp read rate limit should be 300 rpm"
    );
    assert!(
        !config.training.enabled,
        "training capture should be disabled by default"
    );
    assert_eq!(
        config.training.path, "data/training",
        "default training path should be data/training"
    );
}

#[test]
fn serde_roundtrip() {
    let config = AletheiaConfig::default();
    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back.agents.defaults.model_defaults.context_tokens, 200_000,
        "context tokens should survive serde roundtrip"
    );
    assert_eq!(
        back.gateway.port, 18789,
        "gateway port should survive serde roundtrip"
    );
    assert!(
        !back.gateway.rate_limit.trust_proxy,
        "trust_proxy should survive serde roundtrip as false by default"
    );
    assert!(
        back.channels.signal.enabled,
        "signal channel enabled should survive serde roundtrip"
    );
    assert!(
        !back.channels.matrix.enabled,
        "matrix channel enabled flag should survive serde roundtrip"
    );
    assert_eq!(
        back.embedding.provider, "candle",
        "embedding provider should survive serde roundtrip"
    );
    assert_eq!(
        back.embedding.dimension, 384,
        "embedding dimension should survive serde roundtrip"
    );
}

#[test]
fn gateway_rate_limit_trust_proxy_roundtrips() {
    let mut config = AletheiaConfig::default();
    config.gateway.rate_limit.enabled = true;
    config.gateway.rate_limit.trust_proxy = true;

    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");

    assert!(back.gateway.rate_limit.enabled);
    assert!(back.gateway.rate_limit.trust_proxy);
}

#[test]
fn gateway_rate_limit_trust_proxy_parses_from_toml() {
    let toml = r"
[gateway.rateLimit]
enabled = true
requestsPerMinute = 120
trustProxy = true
";

    let config: AletheiaConfig = toml::from_str(toml).expect("parse rate limit config");

    assert!(config.gateway.rate_limit.enabled);
    assert_eq!(config.gateway.rate_limit.requests_per_minute, 120);
    assert!(config.gateway.rate_limit.trust_proxy);
}

#[test]
fn data_retention_parses_strictly() {
    let toml = r"
[data.retention]
enabled = true
sessionMaxAgeDays = 90
orphanMessageMaxAgeDays = 30
maxSessionsPerNous = 200
archiveBeforeDelete = true
";

    let config: AletheiaConfig = toml::from_str(toml).expect("parse data retention config");

    assert!(config.data.retention.enabled);
    assert_eq!(config.data.retention.closed_session_ttl_days, Some(90));
    assert_eq!(config.data.retention.orphan_message_max_age_days, Some(30));
    assert_eq!(config.data.retention.max_sessions_per_nous, 200);
    assert!(config.data.retention.archive_before_delete);
}

#[test]
fn data_config_rejects_unknown_fields() {
    let toml = r"
[data]
surprise = true
";

    let err = toml::from_str::<AletheiaConfig>(toml)
        .expect_err("unknown data field should fail deserialization");
    assert!(
        err.to_string().contains("unknown field"),
        "error should mention unknown field: {err}"
    );
}

#[test]
fn minimal_yaml_parses() {
    let yaml = r#"{"agents": {"list": []}}"#;
    let config: AletheiaConfig = serde_json::from_str(yaml).expect("parse minimal");
    assert_eq!(
        config.agents.defaults.model_defaults.context_tokens, 200_000,
        "minimal yaml should use default context tokens"
    );
    assert!(
        config.agents.list.is_empty(),
        "minimal yaml agent list should be empty"
    );
    assert_eq!(
        config.gateway.port, 18789,
        "minimal yaml should use default gateway port"
    );
}

#[test]
fn camel_case_compat() {
    let yaml = r#"{
        "agents": {
            "defaults": {
                "contextTokens": 100000,
                "maxOutputTokens": 8192,
                "bootstrapMaxTokens": 20000
            },
            "list": []
        }
    }"#;
    let config: AletheiaConfig = serde_json::from_str(yaml).expect("parse camelCase");
    assert_eq!(
        config.agents.defaults.model_defaults.context_tokens, 100_000,
        "camelCase contextTokens should be accepted"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.max_output_tokens, 8192,
        "camelCase maxOutputTokens should be accepted"
    );
    assert_eq!(
        config.agents.defaults.model_defaults.bootstrap_max_tokens, 20_000,
        "camelCase bootstrapMaxTokens should be accepted"
    );
}

#[test]
fn resolve_uses_defaults_for_unknown_agent() {
    let config = AletheiaConfig::default();
    let resolved = resolve_nous(&config, "unknown-agent");
    assert_eq!(
        resolved.id.as_ref(),
        "unknown-agent",
        "resolved id should match requested agent id"
    );
    assert_eq!(
        resolved.model.primary,
        Arc::<str>::from("claude-sonnet-4-6"),
        "unknown agent should use default primary model"
    );
    assert_eq!(
        resolved.limits.context_tokens, 200_000,
        "unknown agent should use default context tokens"
    );
    assert!(
        !resolved.capabilities.thinking_enabled,
        "unknown agent should inherit thinking disabled"
    );
    assert_eq!(
        resolved.workspace, "instance/nous/unknown-agent",
        "unknown agent workspace should use default path"
    );
    assert!(
        resolved.domains.is_empty(),
        "unknown agent should have no domains"
    );
    assert_eq!(
        resolved.tool_groups,
        AgentToolGroupPolicy::DenyAll,
        "unknown agent should default to deny-all tool policy"
    );
}

#[test]
fn tool_groups_accepts_explicit_policies() {
    let toml = r#"
[agents.defaults]
toolGroups = "all"

[[agents.list]]
id = "restricted"
workspace = "/tmp/restricted"
toolGroups = ["read", "verify"]
"#;
    let config: AletheiaConfig = toml::from_str(toml).expect("parse config");
    assert_eq!(
        config.agents.defaults.tool_groups,
        AgentToolGroupPolicy::AllowAll
    );
    assert_eq!(
        config.agents.list[0].tool_groups,
        Some(AgentToolGroupPolicy::Groups(vec![
            "read".to_owned(),
            "verify".to_owned()
        ]))
    );

    let resolved = resolve_nous(&config, "restricted");
    assert_eq!(
        resolved.tool_groups,
        AgentToolGroupPolicy::Groups(vec!["read".to_owned(), "verify".to_owned()])
    );
}

#[test]
fn resolve_nous_preserves_agent_tool_allowlist() {
    let toml = r#"
[agents.defaults]
toolGroups = "all"

[[agents.list]]
id = "restricted"
workspace = "/tmp/restricted"
toolAllowlist = ["read", "tool_schema"]
"#;
    let config: AletheiaConfig = toml::from_str(toml).expect("parse config");
    let resolved = resolve_nous(&config, "restricted");
    assert_eq!(
        resolved.tool_allowlist,
        Some(vec!["read".to_owned(), "tool_schema".to_owned()])
    );
}

#[test]
fn empty_tool_groups_deserializes_to_deny_all() {
    let toml = r"
[agents.defaults]
toolGroups = []
";
    let config: AletheiaConfig = toml::from_str(toml).expect("parse config");
    assert_eq!(
        config.agents.defaults.tool_groups,
        AgentToolGroupPolicy::DenyAll
    );
}

#[test]
fn tools_config_deserializes_to_typed_config() {
    let toml = r#"
[tools.required.search]
type = "http"
endpoint = "https://example.com/search"
method = "get"
groups = ["read", "mcp"]
reversibility = "fully_reversible"

[tools.optional.local_mcp]
type = "mcp"
command = "local-mcp"
args = ["--stdio"]
"#;
    let config: AletheiaConfig = toml::from_str(toml).expect("parse config");

    let search = config.tools.required.get("search").expect("search tool");
    assert_eq!(search.kind, ExternalToolKind::Http);
    assert_eq!(search.method, ExternalToolMethod::Get);
    assert_eq!(
        search.groups.as_deref(),
        Some(&[ExternalToolGroupId::Read, ExternalToolGroupId::Mcp][..])
    );
    assert_eq!(
        search.reversibility,
        Some(ExternalToolReversibility::FullyReversible)
    );
    assert_eq!(
        search.endpoint.as_deref(),
        Some("https://example.com/search")
    );

    let local_mcp = config
        .tools
        .optional
        .get("local_mcp")
        .expect("local MCP tool");
    assert_eq!(local_mcp.kind, ExternalToolKind::Mcp);
    assert_eq!(local_mcp.command.as_deref(), Some("local-mcp"));
    assert_eq!(local_mcp.args, vec!["--stdio"]);
}

#[test]
fn tools_config_requires_type_at_deserialize_time() {
    let toml = r#"
[tools.optional.search]
endpoint = "https://example.com/search"
"#;
    let err = toml::from_str::<AletheiaConfig>(toml)
        .expect_err("missing tool type should fail deserialization");
    assert!(
        err.to_string().contains("missing field `type`"),
        "error should mention missing type: {err}"
    );
}

#[test]
fn tools_config_rejects_unknown_entry_fields() {
    let toml = r#"
[tools.optional.search]
type = "http"
endpoint = "https://example.com/search"
surprise = true
"#;
    let err = toml::from_str::<AletheiaConfig>(toml)
        .expect_err("unknown tool field should fail deserialization");
    assert!(
        err.to_string().contains("unknown field"),
        "error should mention unknown field: {err}"
    );
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
            retries_before_fallback: 2,
        }),
        workspace: "/home/user/nous/syn".to_owned(),
        thinking_enabled: None,
        agency: None,
        allowed_roots: Vec::new(),
        domains: vec!["code".to_owned()],
        default: false,
        private: false,
        episteme_cohort: None,
        recall: None,
        recall_profile: None,
        behavior: None,
        ..Default::default()
    });

    let resolved = resolve_nous(&config, "syn");
    assert_eq!(
        resolved.model.primary,
        Arc::<str>::from("claude-opus-4-6"),
        "agent override should replace primary model"
    );
    assert_eq!(
        resolved.model.fallbacks,
        vec![Arc::<str>::from("claude-sonnet-4-6")],
        "agent override should set fallback model"
    );
    assert_eq!(
        resolved.name,
        Some("Synthetic".to_owned()),
        "agent name override should be applied"
    );
    assert_eq!(
        resolved.workspace, "/home/user/nous/syn",
        "agent workspace override should be applied"
    );
    assert_eq!(
        resolved.domains,
        vec!["code"],
        "agent domain override should be applied"
    );
    assert!(
        !resolved.capabilities.thinking_enabled,
        "thinking should remain disabled when not overridden"
    );
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Standard,
        "agency should default to standard when not overridden"
    );
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
        agency: None,
        allowed_roots: vec!["/extra".to_owned(), "/shared".to_owned()],
        domains: Vec::new(),
        default: false,
        private: false,
        episteme_cohort: None,
        recall: None,
        recall_profile: None,
        behavior: None,
        ..Default::default()
    });

    let resolved = resolve_nous(&config, "syn");
    assert_eq!(
        resolved.allowed_roots,
        vec!["/shared", "/extra"],
        "allowed roots should merge defaults and agent-specific roots deduped"
    );
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
        agency: None,
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        private: false,
        episteme_cohort: None,
        recall: None,
        recall_profile: None,
        behavior: None,
        ..Default::default()
    });

    let resolved = resolve_nous(&config, "thinker");
    assert!(
        resolved.capabilities.thinking_enabled,
        "thinking should be enabled when agent overrides to true"
    );
}

#[test]
fn signal_account_defaults() {
    let account = SignalAccountConfig::default();
    assert!(
        account.name.is_none(),
        "signal account display name should default to unset"
    );
    assert!(
        account.enabled,
        "signal account should be enabled by default"
    );
    assert!(
        account.account.is_none(),
        "signal account phone should default to unset"
    );
    assert_eq!(
        account.http_host, "localhost",
        "signal account default host should be localhost"
    );
    assert_eq!(
        account.http_port, 8080,
        "signal account default port should be 8080"
    );
    assert!(
        account.cli_path.is_none(),
        "signal-cli path should default to auto-detect"
    );
    assert!(
        account.auto_start,
        "signal account should auto-start by default"
    );
}

#[test]
fn signal_account_documented_fields_parse_strictly() {
    let toml = concat!(
        "\n",
        "[channels.signal.accounts.default]\n",
        "name = \"Primary Signal\"\n",
        "enabled = true\n",
        "account = \"+15551234567\"\n", // pii-allow: synthetic Signal test number
        "http_host = \"localhost\"\n",
        "http_port = 8081\n",
        "cli_path = \"/usr/bin/signal-cli\"\n",
        "auto_start = false\n",
    );
    let config: AletheiaConfig = toml::from_str(toml).expect("parse Signal account config");
    let account = config
        .channels
        .signal
        .accounts
        .get("default")
        .expect("default Signal account");

    assert_eq!(account.name.as_deref(), Some("Primary Signal"));
    assert_eq!(account.account.as_deref(), Some("+15551234567")); // pii-allow: synthetic Signal test number
    assert_eq!(account.http_host, "localhost");
    assert_eq!(account.http_port, 8081);
    assert_eq!(
        account.cli_path.as_deref(),
        Some(std::path::Path::new("/usr/bin/signal-cli"))
    );
    assert!(!account.auto_start);
}

#[test]
fn signal_account_rejects_unknown_fields() {
    let toml = r"
[channels.signal.accounts.default]
surprise = true
";
    let err = toml::from_str::<AletheiaConfig>(toml)
        .expect_err("unknown Signal account field should fail deserialization");
    assert!(
        err.to_string().contains("unknown field"),
        "error should mention unknown field: {err}"
    );
}

#[test]
fn channel_binding_serde_roundtrip() {
    let json = r#"{
        "channel": "signal",
        "source": "+1234567890",
        "nousId": "syn"
    }"#;
    let binding: ChannelBinding = serde_json::from_str(json).expect("parse binding");
    assert_eq!(
        binding.channel, "signal",
        "channel field should deserialize correctly"
    );
    assert_eq!(
        binding.source, "+1234567890",
        "source field should deserialize correctly"
    );
    assert_eq!(
        binding.nous_id, "syn",
        "nous_id field should deserialize correctly"
    );
    assert_eq!(
        binding.session_key, "{source}",
        "session_key should default to source pattern"
    );
}

#[test]
fn embedding_override_from_json() {
    let json = r#"{
        "embedding": {
            "provider": "candle",
            "model": "BAAI/bge-small-en-v1.5",
            "dimension": 512,
            "baseUrl": "http://127.0.0.1:5005/v1",
            "apiKeyEnv": "ALETHEIA_EMBEDDING_KEY"
        }
    }"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse embedding");
    assert_eq!(
        config.embedding.provider, "candle",
        "embedding provider should be parsed from json"
    );
    assert_eq!(
        config.embedding.model,
        Some("BAAI/bge-small-en-v1.5".to_owned()),
        "embedding model should be parsed from json"
    );
    assert_eq!(
        config.embedding.dimension, 512,
        "embedding dimension override should be parsed from json"
    );
    assert_eq!(
        config.embedding.base_url,
        Some("http://127.0.0.1:5005/v1".to_owned()),
        "embedding baseUrl should be parsed from json"
    );
    assert_eq!(
        config.embedding.api_key_env,
        Some("ALETHEIA_EMBEDDING_KEY".to_owned()),
        "embedding apiKeyEnv should be parsed from json"
    );
}

#[test]
fn embedding_env_overlay_alias_keys_deserialize() {
    let json = r#"{
        "embedding": {
            "provider": "openai-compat",
            "dimension": 512,
            "baseurl": "http://127.0.0.1:5005/v1",
            "apikeyenv": "ALETHEIA_EMBEDDING_KEY"
        }
    }"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse embedding aliases");
    assert_eq!(
        config.embedding.base_url,
        Some("http://127.0.0.1:5005/v1".to_owned()),
        "lowercase baseurl alias should parse for env overlay"
    );
    assert_eq!(
        config.embedding.api_key_env,
        Some("ALETHEIA_EMBEDDING_KEY".to_owned()),
        "lowercase apikeyenv alias should parse for env overlay"
    );
}

#[test]
fn bindings_in_config_default_empty() {
    let config = AletheiaConfig::default();
    assert!(
        config.bindings.is_empty(),
        "bindings should be empty in default config"
    );
}

#[test]
fn pricing_defaults_empty() {
    let config = AletheiaConfig::default();
    assert!(
        config.pricing.is_empty(),
        "pricing map should be empty in default config"
    );
}

#[test]
fn recall_settings_serde_roundtrip() {
    let mut config = AletheiaConfig::default();
    config.agents.defaults.recall.reranker_url = Some("http://localhost:9999/rerank".to_owned());
    config.agents.defaults.recall.reranker_model_path = Some("/models/cross-encoder".to_owned());

    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(
        back.agents.defaults.recall.reranker_url,
        Some("http://localhost:9999/rerank".to_owned()),
        "reranker_url should survive serde roundtrip"
    );
    assert_eq!(
        back.agents.defaults.recall.reranker_model_path,
        Some("/models/cross-encoder".to_owned()),
        "reranker_model_path should survive serde roundtrip"
    );
}

#[test]
fn pricing_from_json() {
    let json = r#"{
        "pricing": {
            "claude-opus-4-6": {
                "inputCostPerMtok": 15.0,
                "outputCostPerMtok": 75.0
            },
            "claude-sonnet-4-6": {
                "inputCostPerMtok": 3.0,
                "outputCostPerMtok": 15.0
            }
        }
    }"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse pricing");
    assert_eq!(
        config.pricing.len(),
        2,
        "pricing map should contain two entries"
    );
    let opus = &config.pricing["claude-opus-4-6"];
    assert!(
        (opus.input_cost_per_mtok - 15.0).abs() < f64::EPSILON,
        "opus input cost should be 15.0"
    );
    assert!(
        (opus.output_cost_per_mtok - 75.0).abs() < f64::EPSILON,
        "opus output cost should be 75.0"
    );
    let sonnet = &config.pricing["claude-sonnet-4-6"];
    assert!(
        (sonnet.input_cost_per_mtok - 3.0).abs() < f64::EPSILON,
        "sonnet input cost should be 3.0"
    );
    assert!(
        (sonnet.output_cost_per_mtok - 15.0).abs() < f64::EPSILON,
        "sonnet output cost should be 15.0"
    );
}

#[test]
fn prosoche_defaults_match_hardcoded_runtime() {
    let config = AletheiaConfig::default();
    let prosoche = &config.maintenance.prosoche;

    assert_eq!(
        prosoche.mode,
        ProsocheScheduleMode::Daemon,
        "default prosoche scheduling mode should be daemon"
    );
    assert!(
        prosoche.mode.runs_daemon_tasks(),
        "daemon mode should run internal tasks"
    );
    assert!(
        !prosoche.mode.uses_external_timer(),
        "daemon mode should not use the external timer"
    );

    assert!(
        prosoche.heartbeat.enabled,
        "prosoche heartbeat should be enabled by default"
    );
    assert_eq!(
        prosoche.heartbeat.interval_secs,
        45 * 60,
        "prosoche heartbeat interval should be 45 minutes"
    );
    assert_eq!(
        prosoche.heartbeat.active_window,
        Some(ProsocheActiveWindowSettings {
            start_hour: 8,
            end_hour: 23,
        }),
        "prosoche heartbeat active window should be 8..23"
    );

    assert!(
        prosoche.self_audit.enabled,
        "prosoche self-audit should be enabled by default"
    );
    assert_eq!(
        prosoche.self_audit.interval_secs,
        6 * 3600,
        "prosoche self-audit interval should be 6 hours"
    );
    assert_eq!(
        prosoche.self_audit.active_window,
        Some(ProsocheActiveWindowSettings {
            start_hour: 8,
            end_hour: 23,
        }),
        "prosoche self-audit active window should be 8..23"
    );

    assert!(
        !prosoche.external_timer.enabled,
        "external timer should be disabled by default"
    );
    assert_eq!(
        prosoche.external_timer.task_id, "prosoche-self-audit",
        "external timer default task id should be prosoche-self-audit"
    );
    assert_eq!(
        prosoche.external_timer.interval_secs, 300,
        "external timer default interval should be 300 seconds"
    );
}

#[test]
fn prosoche_config_overrides_parse() {
    let toml = r#"
[maintenance.prosoche]
mode = "both"

[maintenance.prosoche.heartbeat]
enabled = false
intervalSecs = 1800

[maintenance.prosoche.heartbeat.activeWindow]
startHour = 9
endHour = 22

[maintenance.prosoche.selfAudit]
intervalSecs = 7200

[maintenance.prosoche.externalTimer]
enabled = true
taskId = "custom-self-audit"
intervalSecs = 60
"#;

    let config: AletheiaConfig = toml::from_str(toml).expect("parse prosoche overrides");
    let prosoche = &config.maintenance.prosoche;

    assert_eq!(prosoche.mode, ProsocheScheduleMode::Both);
    assert!(prosoche.mode.runs_daemon_tasks());
    assert!(prosoche.mode.uses_external_timer());

    assert!(!prosoche.heartbeat.enabled);
    assert_eq!(prosoche.heartbeat.interval_secs, 1800);
    assert_eq!(
        prosoche.heartbeat.active_window,
        Some(ProsocheActiveWindowSettings {
            start_hour: 9,
            end_hour: 22,
        })
    );

    assert!(prosoche.self_audit.enabled);
    assert_eq!(prosoche.self_audit.interval_secs, 7200);
    assert_eq!(
        prosoche.self_audit.active_window,
        Some(ProsocheActiveWindowSettings {
            start_hour: 8,
            end_hour: 23,
        })
    );

    assert!(prosoche.external_timer.enabled);
    assert_eq!(prosoche.external_timer.task_id, "custom-self-audit");
    assert_eq!(prosoche.external_timer.interval_secs, 60);
}

#[test]
fn prosoche_disabled_mode_parses() {
    let toml = r#"
[maintenance.prosoche]
mode = "disabled"

[maintenance.prosoche.heartbeat]
enabled = true
"#;

    let config: AletheiaConfig = toml::from_str(toml).expect("parse disabled prosoche mode");
    let prosoche = &config.maintenance.prosoche;

    assert_eq!(prosoche.mode, ProsocheScheduleMode::Disabled);
    assert!(!prosoche.mode.runs_daemon_tasks());
    assert!(!prosoche.mode.uses_external_timer());
    // Enablement on individual tasks is ignored when the mode disables scheduling.
    assert!(prosoche.heartbeat.enabled);
    assert_eq!(prosoche.heartbeat.interval_secs, 45 * 60);
}

#[test]
fn prosoche_external_only_mode_parses() {
    let toml = r#"
[maintenance.prosoche]
mode = "external"

[maintenance.prosoche.externalTimer]
enabled = true
"#;

    let config: AletheiaConfig = toml::from_str(toml).expect("parse external-only prosoche mode");
    let prosoche = &config.maintenance.prosoche;

    assert_eq!(prosoche.mode, ProsocheScheduleMode::External);
    assert!(!prosoche.mode.runs_daemon_tasks());
    assert!(prosoche.mode.uses_external_timer());
    assert!(prosoche.external_timer.enabled);
    assert_eq!(prosoche.external_timer.task_id, "prosoche-self-audit");
    assert_eq!(prosoche.external_timer.interval_secs, 300);
}

// WHY(#5482): regression guard — each sub-function covers one logical group of
// formerly-hand-duplicated constants; kept small to satisfy too_many_lines lint.
#[test]
#[expect(
    clippy::float_cmp,
    reason = "WHY(#5482): comparing compile-time constants that are by definition identical; exact equality is the invariant"
)]
fn mirrored_defaults_provider_behavior() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.provider_behavior.non_streaming_timeout_secs,
        hermeneus::anthropic::NON_STREAMING_TIMEOUT.as_secs()
    );
    assert_eq!(
        config.provider_behavior.sse_default_retry_ms,
        hermeneus::anthropic::SSE_DEFAULT_RETRY_MS
    );
    assert_eq!(
        config.provider_behavior.concurrency_ewma_alpha,
        hermeneus::concurrency::DEFAULT_EWMA_ALPHA
    );
    assert_eq!(
        config.provider_behavior.concurrency_latency_threshold_secs,
        hermeneus::concurrency::DEFAULT_LATENCY_THRESHOLD_SECS
    );
    assert_eq!(
        config.provider_behavior.complexity_low_threshold,
        hermeneus::complexity::DEFAULT_LOW_THRESHOLD
    );
    assert_eq!(
        config.provider_behavior.complexity_high_threshold,
        hermeneus::complexity::DEFAULT_HIGH_THRESHOLD
    );
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "WHY(#5482): comparing compile-time constants that are by definition identical; exact equality is the invariant"
)]
fn mirrored_defaults_knowledge_behavior() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.knowledge.max_content_length,
        eidos::knowledge::fact::MAX_CONTENT_LENGTH
    );
    assert_eq!(
        config.knowledge.side_query_max_results,
        episteme::side_query::DEFAULT_MAX_RESULTS
    );
    assert_eq!(
        config.knowledge.side_query_cache_ttl_secs,
        episteme::side_query::DEFAULT_CACHE_TTL_SECS
    );
    assert_eq!(
        config.knowledge.side_query_cache_capacity,
        episteme::side_query::DEFAULT_CACHE_CAPACITY
    );
    assert_eq!(
        config.knowledge.skill_decay_needs_review_threshold,
        episteme::skill::decay::NEEDS_REVIEW_THRESHOLD
    );
    assert_eq!(
        config.knowledge.skill_decay_retire_threshold,
        episteme::skill::decay::RETIRE_THRESHOLD
    );
    assert_eq!(
        config.knowledge.skill_decay_stale_days,
        episteme::skill::decay::DEFAULT_STALE_DAYS
    );
    assert_eq!(
        config.knowledge.skill_decay_high_usage_threshold,
        episteme::skill::decay::HIGH_USAGE_THRESHOLD
    );
    assert_eq!(
        config.knowledge.skill_decay_high_usage_factor,
        episteme::skill::decay::HIGH_USAGE_DECAY_FACTOR
    );
    assert_eq!(
        config.knowledge.surprise_threshold,
        episteme::surprise::DEFAULT_THRESHOLD
    );
    assert_eq!(
        config.knowledge.surprise_ema_alpha,
        episteme::surprise::DEFAULT_EMA_ALPHA
    );
}

#[test]
fn mirrored_defaults_messaging_behavior() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.messaging.poll_interval_ms,
        agora::semeion::DEFAULT_POLL_INTERVAL.as_secs() * 1_000
    );
    assert_eq!(
        config.messaging.buffer_capacity,
        agora::semeion::DEFAULT_BUFFER_CAPACITY
    );
    assert_eq!(
        config.messaging.circuit_breaker_threshold,
        agora::semeion::CIRCUIT_BREAKER_THRESHOLD
    );
    assert_eq!(
        config.messaging.halted_health_check_interval_secs,
        agora::semeion::HALTED_HEALTH_CHECK_INTERVAL.as_secs()
    );
    assert_eq!(
        config.messaging.rpc_timeout_secs,
        agora::semeion::client::RPC_TIMEOUT.as_secs()
    );
    assert_eq!(
        config.messaging.health_timeout_secs,
        agora::semeion::client::HEALTH_TIMEOUT.as_secs()
    );
    assert_eq!(
        config.messaging.receive_timeout_secs,
        agora::semeion::client::RECEIVE_TIMEOUT.as_secs()
    );
    assert_eq!(
        config.messaging.agent_dispatch_timeout_secs,
        organon::builtins::agent::DEFAULT_TIMEOUT_SECS
    );
}

#[test]
fn mirrored_defaults_daemon_and_api() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.daemon_behavior.watchdog_backoff_base_secs,
        oikonomos::watchdog::BACKOFF_BASE.as_secs()
    );
    assert_eq!(
        config.daemon_behavior.watchdog_backoff_cap_secs,
        oikonomos::watchdog::BACKOFF_CAP.as_secs()
    );
    assert_eq!(
        config.api_limits.idempotency_ttl_secs,
        pylon::idempotency::DEFAULT_TTL.as_secs()
    );
    assert_eq!(
        config.api_limits.idempotency_capacity,
        pylon::idempotency::DEFAULT_CAPACITY
    );
}

#[test]
fn mirrored_defaults_tool_limits() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.tool_limits.max_pattern_length,
        organon::builtins::filesystem::MAX_PATTERN_LENGTH
    );
    assert_eq!(
        config.tool_limits.subprocess_timeout_secs,
        organon::builtins::filesystem::SUBPROCESS_TIMEOUT.as_secs()
    );
    assert_eq!(
        config.tool_limits.max_write_bytes,
        organon::builtins::workspace::MAX_WRITE_BYTES
    );
    assert_eq!(
        config.tool_limits.max_read_bytes,
        organon::builtins::workspace::MAX_READ_BYTES
    );
    assert_eq!(
        config.tool_limits.max_command_length,
        organon::builtins::workspace::MAX_COMMAND_LENGTH
    );
    assert_eq!(
        config.tool_limits.message_max_len,
        organon::builtins::communication::MESSAGE_MAX_LEN
    );
    assert_eq!(
        config.tool_limits.inter_session_max_message_len,
        organon::builtins::communication::INTER_SESSION_MAX_MESSAGE_LEN
    );
    assert_eq!(
        config.tool_limits.inter_session_max_timeout_secs,
        organon::builtins::communication::INTER_SESSION_MAX_TIMEOUT_SECS
    );
}

#[test]
#[expect(
    clippy::float_cmp,
    reason = "WHY(#5482): comparing compile-time constants that are by definition identical; exact equality is the invariant"
)]
fn mirrored_defaults_agent_behavior() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.agents.defaults.behavior.manifest_max_entries,
        episteme::manifest::MAX_MEMORY_ENTRIES
    );
    assert_eq!(
        config.agents.defaults.behavior.planning_max_iterations,
        dianoia::plan::DEFAULT_MAX_ITERATIONS
    );
    assert_eq!(
        config.agents.defaults.behavior.knowledge_surprise_threshold,
        episteme::surprise::DEFAULT_THRESHOLD
    );
    assert_eq!(
        config.agents.defaults.behavior.dream_min_hours,
        melete::dream::DEFAULT_MIN_HOURS
    );
    assert_eq!(
        config.agents.defaults.behavior.dream_min_sessions,
        melete::dream::DEFAULT_MIN_SESSIONS
    );
    assert_eq!(
        config.agents.defaults.behavior.dream_stale_threshold_secs,
        melete::dream::DEFAULT_STALE_THRESHOLD_SECS
    );
    assert_eq!(
        config
            .agents
            .defaults
            .behavior
            .tool_agent_dispatch_max_tasks,
        organon::builtins::agent::MAX_DISPATCH_TASKS
    );
    assert_eq!(
        config
            .agents
            .defaults
            .behavior
            .tool_datalog_default_row_limit,
        organon::builtins::memory::datalog::DEFAULT_ROW_LIMIT
    );
    assert_eq!(
        config
            .agents
            .defaults
            .behavior
            .tool_datalog_default_timeout_secs,
        organon::builtins::memory::datalog::DEFAULT_TIMEOUT_SECS
    );
    assert_eq!(
        config.agents.defaults.behavior.tool_max_image_bytes,
        organon::builtins::view_file::MAX_IMAGE_BYTES
    );
    assert_eq!(
        config.agents.defaults.behavior.tool_max_pdf_bytes,
        organon::builtins::view_file::MAX_PDF_BYTES
    );
    assert_eq!(
        config.credential.refresh_threshold_secs,
        symbolon::credential::REFRESH_THRESHOLD_SECS
    );
    assert_eq!(
        config.jwt.clock_skew_leeway_secs,
        symbolon::jwt::DEFAULT_CLOCK_SKEW_LEEWAY_SECS
    );
}
