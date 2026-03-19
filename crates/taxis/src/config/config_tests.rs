#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: map/vec indexing with keys/indices asserted present by surrounding context"
)]

use super::*;

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "comprehensive config default validation"
)]
fn defaults_are_sensible() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.agents.defaults.context_tokens, 200_000,
        "default context tokens should be 200k"
    );
    assert_eq!(
        config.agents.defaults.max_output_tokens, 16_384,
        "default max output tokens should be 16384"
    );
    assert_eq!(
        config.agents.defaults.bootstrap_max_tokens, 40_000,
        "default bootstrap max tokens should be 40k"
    );
    assert_eq!(
        config.agents.defaults.model.primary, "claude-sonnet-4-6",
        "default primary model should be sonnet"
    );
    assert!(
        !config.agents.defaults.thinking_enabled,
        "thinking should be disabled by default"
    );
    assert_eq!(
        config.agents.defaults.thinking_budget, 10_000,
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
    assert_eq!(
        config.gateway.csrf.header_name, "x-requested-with",
        "default csrf header name should be x-requested-with"
    );
    assert_eq!(
        config.gateway.csrf.header_value, "aletheia",
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
        config.channels.signal.enabled,
        "signal channel should be enabled by default"
    );
    assert!(
        config.channels.signal.accounts.is_empty(),
        "signal accounts should be empty by default"
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
}

#[test]
fn serde_roundtrip() {
    let config = AletheiaConfig::default();
    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back.agents.defaults.context_tokens, 200_000,
        "context tokens should survive serde roundtrip"
    );
    assert_eq!(
        back.gateway.port, 18789,
        "gateway port should survive serde roundtrip"
    );
    assert!(
        back.channels.signal.enabled,
        "signal channel enabled should survive serde roundtrip"
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
fn minimal_yaml_parses() {
    let yaml = r#"{"agents": {"list": []}}"#;
    let config: AletheiaConfig = serde_json::from_str(yaml).expect("parse minimal");
    assert_eq!(
        config.agents.defaults.context_tokens, 200_000,
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
        config.agents.defaults.context_tokens, 100_000,
        "camelCase contextTokens should be accepted"
    );
    assert_eq!(
        config.agents.defaults.max_output_tokens, 8192,
        "camelCase maxOutputTokens should be accepted"
    );
    assert_eq!(
        config.agents.defaults.bootstrap_max_tokens, 20_000,
        "camelCase bootstrapMaxTokens should be accepted"
    );
}

#[test]
fn resolve_uses_defaults_for_unknown_agent() {
    let config = AletheiaConfig::default();
    let resolved = resolve_nous(&config, "unknown-agent");
    assert_eq!(
        resolved.id, "unknown-agent",
        "resolved id should match requested agent id"
    );
    assert_eq!(
        resolved.model.primary, "claude-sonnet-4-6",
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
        recall: None,
    });

    let resolved = resolve_nous(&config, "syn");
    assert_eq!(
        resolved.model.primary, "claude-opus-4-6",
        "agent override should replace primary model"
    );
    assert_eq!(
        resolved.model.fallbacks,
        vec!["claude-sonnet-4-6"],
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
        recall: None,
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
        recall: None,
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
        account.enabled,
        "signal account should be enabled by default"
    );
    assert_eq!(
        account.http_host, "localhost",
        "signal account default host should be localhost"
    );
    assert_eq!(
        account.http_port, 8080,
        "signal account default port should be 8080"
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
            "dimension": 512
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
fn agency_default_is_standard() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.agents.defaults.agency,
        AgencyLevel::Standard,
        "default agency level should be standard"
    );
}

#[test]
fn agency_serde_roundtrip() {
    let json = serde_json::to_string(&AgencyLevel::Unrestricted).expect("serialize");
    assert_eq!(
        json, "\"unrestricted\"",
        "unrestricted agency should serialize to string 'unrestricted'"
    );
    let back: AgencyLevel = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back,
        AgencyLevel::Unrestricted,
        "unrestricted agency should roundtrip through serde"
    );

    let json = serde_json::to_string(&AgencyLevel::Restricted).expect("serialize");
    assert_eq!(
        json, "\"restricted\"",
        "restricted agency should serialize to string 'restricted'"
    );
}

#[test]
fn resolve_agency_inherits_global_default() {
    let config = AletheiaConfig::default();
    let resolved = resolve_nous(&config, "any");
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Standard,
        "global default agency should be standard"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 200,
        "standard agency should use default max tool iterations"
    );
}

#[test]
fn resolve_agency_unrestricted_sets_high_iterations() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "free".to_owned(),
        name: None,
        model: None,
        workspace: "/home/user/nous/free".to_owned(),
        thinking_enabled: None,
        agency: Some(AgencyLevel::Unrestricted),
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
    });

    let resolved = resolve_nous(&config, "free");
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Unrestricted,
        "agent agency override should be unrestricted"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 10_000,
        "unrestricted agency should set max tool iterations to 10k"
    );
}

#[test]
fn resolve_agency_restricted_uses_old_defaults() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "safe".to_owned(),
        name: None,
        model: None,
        workspace: "/home/user/nous/safe".to_owned(),
        thinking_enabled: None,
        agency: Some(AgencyLevel::Restricted),
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
    });

    let resolved = resolve_nous(&config, "safe");
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Restricted,
        "agent agency override should be restricted"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 50,
        "restricted agency should use low max tool iterations"
    );
}

#[test]
fn resolve_agency_per_agent_overrides_global() {
    let mut config = AletheiaConfig::default();
    config.agents.defaults.agency = AgencyLevel::Restricted;
    config.agents.list.push(NousDefinition {
        id: "override".to_owned(),
        name: None,
        model: None,
        workspace: "/home/user/nous/override".to_owned(),
        thinking_enabled: None,
        agency: Some(AgencyLevel::Unrestricted),
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        recall: None,
    });

    let resolved = resolve_nous(&config, "override");
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Unrestricted,
        "per-agent unrestricted should override global restricted"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 10_000,
        "per-agent unrestricted override should set iterations to 10k"
    );

    let other = resolve_nous(&config, "other");
    assert_eq!(
        other.capabilities.agency,
        AgencyLevel::Restricted,
        "agent without override should use global restricted agency"
    );
    assert_eq!(
        other.capabilities.max_tool_iterations, 50,
        "agent using global restricted should get low max tool iterations"
    );
}

#[test]
fn agency_from_json() {
    let json = r#"{
        "agents": {
            "defaults": {
                "agency": "unrestricted"
            },
            "list": [{
                "id": "restricted-agent",
                "workspace": "/tmp/ws",
                "agency": "restricted"
            }]
        }
    }"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse agency");
    assert_eq!(
        config.agents.defaults.agency,
        AgencyLevel::Unrestricted,
        "global agency override from json should be unrestricted"
    );
    assert_eq!(
        config.agents.list[0].agency,
        Some(AgencyLevel::Restricted),
        "per-agent restricted override from json should be applied"
    );
}

mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_channel_binding() -> impl Strategy<Value = ChannelBinding> {
        (
            "[a-z]{3,10}",
            "[a-zA-Z0-9+*]{1,20}",
            "[a-z]{2,8}",
            proptest::option::of("[a-z{}]{1,20}"),
        )
            .prop_map(|(channel, source, nous_id, session_key)| ChannelBinding {
                channel,
                source,
                nous_id,
                session_key: session_key.unwrap_or_else(default_session_pattern),
            })
    }

    proptest! {
        #[test]
        fn channel_binding_roundtrip(binding in arb_channel_binding()) {
            let json = serde_json::to_string(&binding).expect("serialize");
            let back: ChannelBinding = serde_json::from_str(&json).expect("deserialize");
            prop_assert_eq!(&binding.channel, &back.channel);
            prop_assert_eq!(&binding.source, &back.source);
            prop_assert_eq!(&binding.nous_id, &back.nous_id);
            prop_assert_eq!(&binding.session_key, &back.session_key);
        }
    }
}
