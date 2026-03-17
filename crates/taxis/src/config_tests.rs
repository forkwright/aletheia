#![expect(clippy::expect_used, reason = "test assertions")]

use super::*;

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "comprehensive config default assertions"
)]
fn defaults_are_sensible() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.agents.defaults.context_tokens, 200_000,
        "context_tokens should match 200_000"
    );
    assert_eq!(
        config.agents.defaults.max_output_tokens, 16_384,
        "max_output_tokens should match 16_384"
    );
    assert_eq!(
        config.agents.defaults.bootstrap_max_tokens, 40_000,
        "bootstrap_max_tokens should match 40_000"
    );
    assert_eq!(
        config.agents.defaults.model.primary, "claude-sonnet-4-6",
        "primary should equal expected value"
    );
    assert!(
        !config.agents.defaults.thinking_enabled,
        "thinking_enabled should be false"
    );
    assert_eq!(
        config.agents.defaults.thinking_budget, 10_000,
        "thinking_budget should match 10_000"
    );
    assert_eq!(
        config.agents.defaults.max_tool_iterations, 200,
        "max_tool_iterations should equal expected value"
    );
    assert_eq!(
        config.gateway.port, 18789,
        "port should equal expected value"
    );
    assert_eq!(
        config.gateway.bind, "localhost",
        "bind should equal expected value"
    );
    assert_eq!(
        config.gateway.auth.mode, "token",
        "mode should equal expected value"
    );
    assert!(!config.gateway.tls.enabled, "enabled should be false");
    assert!(
        config.gateway.tls.cert_path.is_none(),
        "cert_path should be none"
    );
    assert!(
        config.gateway.cors.allowed_origins.is_empty(),
        "allowed_origins should be empty"
    );
    assert_eq!(
        config.gateway.cors.max_age_secs, 3600,
        "max_age_secs should equal expected value"
    );
    assert_eq!(
        config.gateway.body_limit.max_bytes, 1_048_576,
        "max_bytes should match 1_048_576"
    );
    assert!(
        config.gateway.csrf.enabled,
        "assertion failed in defaults are sensible"
    );
    assert_eq!(
        config.gateway.csrf.header_name, "x-requested-with",
        "header_name should equal expected value"
    );
    assert_eq!(
        config.gateway.csrf.header_value, "aletheia",
        "header_value should equal expected value"
    );
    assert!(
        !config.gateway.rate_limit.enabled,
        "enabled should be false"
    );
    assert_eq!(
        config.gateway.rate_limit.requests_per_minute, 60,
        "requests_per_minute should equal expected value"
    );
    assert!(
        config.channels.signal.enabled,
        "assertion failed in defaults are sensible"
    );
    assert!(
        config.channels.signal.accounts.is_empty(),
        "accounts should be empty"
    );
    assert!(config.bindings.is_empty(), "bindings should be empty");
    assert_eq!(
        config.embedding.provider, "candle",
        "provider should equal expected value"
    );
    assert!(config.embedding.model.is_none(), "model should be none");
    assert_eq!(
        config.embedding.dimension, 384,
        "dimension should equal expected value"
    );
    assert!(
        config.maintenance.trace_rotation.enabled,
        "assertion failed in defaults are sensible"
    );
    assert_eq!(
        config.maintenance.trace_rotation.max_age_days, 14,
        "max_age_days should equal expected value"
    );
    assert!(
        config.maintenance.drift_detection.enabled,
        "assertion failed in defaults are sensible"
    );
    assert!(
        config.maintenance.db_monitoring.enabled,
        "assertion failed in defaults are sensible"
    );
    assert_eq!(
        config.maintenance.db_monitoring.warn_threshold_mb, 100,
        "warn_threshold_mb should equal expected value"
    );
    assert!(
        !config.maintenance.retention.enabled,
        "enabled should be false"
    );
    assert!(config.pricing.is_empty(), "pricing should be empty");
    assert!(
        config.mcp.rate_limit.enabled,
        "assertion failed in defaults are sensible"
    );
    assert_eq!(
        config.mcp.rate_limit.message_requests_per_minute, 60,
        "message_requests_per_minute should equal expected value"
    );
    assert_eq!(
        config.mcp.rate_limit.read_requests_per_minute, 300,
        "read_requests_per_minute should equal expected value"
    );
}

#[test]
fn serde_roundtrip() {
    let config = AletheiaConfig::default();
    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back.agents.defaults.context_tokens, 200_000,
        "context_tokens should match 200_000"
    );
    assert_eq!(back.gateway.port, 18789, "port should equal expected value");
    assert!(
        back.channels.signal.enabled,
        "assertion failed in serde roundtrip"
    );
    assert_eq!(
        back.embedding.provider, "candle",
        "provider should equal expected value"
    );
    assert_eq!(
        back.embedding.dimension, 384,
        "dimension should equal expected value"
    );
}

#[test]
fn minimal_yaml_parses() {
    let yaml = r#"{"agents": {"list": []}}"#;
    let config: AletheiaConfig = serde_json::from_str(yaml).expect("parse minimal");
    assert_eq!(
        config.agents.defaults.context_tokens, 200_000,
        "context_tokens should match 200_000"
    );
    assert!(config.agents.list.is_empty(), "list should be empty");
    assert_eq!(
        config.gateway.port, 18789,
        "port should equal expected value"
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
        "context_tokens should match 100_000"
    );
    assert_eq!(
        config.agents.defaults.max_output_tokens, 8192,
        "max_output_tokens should equal expected value"
    );
    assert_eq!(
        config.agents.defaults.bootstrap_max_tokens, 20_000,
        "bootstrap_max_tokens should match 20_000"
    );
}

#[test]
fn resolve_uses_defaults_for_unknown_agent() {
    let config = AletheiaConfig::default();
    let resolved = resolve_nous(&config, "unknown-agent");
    assert_eq!(
        resolved.id, "unknown-agent",
        "id should equal expected value"
    );
    assert_eq!(
        resolved.model.primary, "claude-sonnet-4-6",
        "primary should equal expected value"
    );
    assert_eq!(
        resolved.limits.context_tokens, 200_000,
        "context_tokens should match 200_000"
    );
    assert!(
        !resolved.capabilities.thinking_enabled,
        "thinking_enabled should be false"
    );
    assert_eq!(
        resolved.workspace, "instance/nous/unknown-agent",
        "workspace should equal expected value"
    );
    assert!(resolved.domains.is_empty(), "domains should be empty");
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
        "primary should equal expected value"
    );
    assert_eq!(
        resolved.model.fallbacks,
        vec!["claude-sonnet-4-6"],
        "fallbacks should match vec![\"claude-sonnet-4-6\"]"
    );
    assert_eq!(
        resolved.name,
        Some("Synthetic".to_owned()),
        "name should match to_owned("
    );
    assert_eq!(
        resolved.workspace, "/home/user/nous/syn",
        "workspace should equal expected value"
    );
    assert_eq!(
        resolved.domains,
        vec!["code"],
        "domains should match vec![\"code\"]"
    );
    assert!(
        !resolved.capabilities.thinking_enabled,
        "thinking_enabled should be false"
    );
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Standard,
        "agency should equal expected value"
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
        "allowed_roots should match vec![\"/shared\", \"/extra\"]"
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
        "assertion failed in resolve thinking override"
    );
}

#[test]
fn signal_account_defaults() {
    let account = SignalAccountConfig::default();
    assert!(
        account.enabled,
        "assertion failed in signal account defaults"
    );
    assert_eq!(
        account.http_host, "localhost",
        "http_host should equal expected value"
    );
    assert_eq!(
        account.http_port, 8080,
        "http_port should equal expected value"
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
        "channel should equal expected value"
    );
    assert_eq!(
        binding.source, "+1234567890",
        "source should equal expected value"
    );
    assert_eq!(
        binding.nous_id, "syn",
        "nous_id should equal expected value"
    );
    assert_eq!(
        binding.session_key, "{source}",
        "session_key should equal expected value"
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
        "provider should equal expected value"
    );
    assert_eq!(
        config.embedding.model,
        Some("BAAI/bge-small-en-v1.5".to_owned()),
        "model should match to_owned(",
    );
    assert_eq!(
        config.embedding.dimension, 512,
        "dimension should equal expected value"
    );
}

#[test]
fn bindings_in_config_default_empty() {
    let config = AletheiaConfig::default();
    assert!(config.bindings.is_empty(), "bindings should be empty");
}

#[test]
fn pricing_defaults_empty() {
    let config = AletheiaConfig::default();
    assert!(config.pricing.is_empty(), "pricing should be empty");
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
        "pricing length should equal expected value"
    );
    let opus = &config.pricing["claude-opus-4-6"];
    assert!(
        (opus.input_cost_per_mtok - 15.0).abs() < f64::EPSILON,
        "assertion failed in pricing from json"
    );
    assert!(
        (opus.output_cost_per_mtok - 75.0).abs() < f64::EPSILON,
        "assertion failed in pricing from json"
    );
    let sonnet = &config.pricing["claude-sonnet-4-6"];
    assert!(
        (sonnet.input_cost_per_mtok - 3.0).abs() < f64::EPSILON,
        "assertion failed in pricing from json"
    );
    assert!(
        (sonnet.output_cost_per_mtok - 15.0).abs() < f64::EPSILON,
        "assertion failed in pricing from json"
    );
}

#[test]
fn agency_default_is_standard() {
    let config = AletheiaConfig::default();
    assert_eq!(
        config.agents.defaults.agency,
        AgencyLevel::Standard,
        "agency should equal expected value"
    );
}

#[test]
fn agency_serde_roundtrip() {
    let json = serde_json::to_string(&AgencyLevel::Unrestricted).expect("serialize");
    assert_eq!(json, "\"unrestricted\"", "json should equal expected value");
    let back: AgencyLevel = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back,
        AgencyLevel::Unrestricted,
        "back should equal expected value"
    );

    let json = serde_json::to_string(&AgencyLevel::Restricted).expect("serialize");
    assert_eq!(json, "\"restricted\"", "json should equal expected value");
}

#[test]
fn resolve_agency_inherits_global_default() {
    let config = AletheiaConfig::default();
    let resolved = resolve_nous(&config, "any");
    assert_eq!(
        resolved.capabilities.agency,
        AgencyLevel::Standard,
        "agency should equal expected value"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 200,
        "max_tool_iterations should equal expected value"
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
        "agency should equal expected value"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 10_000,
        "max_tool_iterations should match 10_000"
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
        "agency should equal expected value"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 50,
        "max_tool_iterations should equal expected value"
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
        "agency should equal expected value"
    );
    assert_eq!(
        resolved.capabilities.max_tool_iterations, 10_000,
        "max_tool_iterations should match 10_000"
    );

    // Agent without override should use global
    let other = resolve_nous(&config, "other");
    assert_eq!(
        other.capabilities.agency,
        AgencyLevel::Restricted,
        "agency should equal expected value"
    );
    assert_eq!(
        other.capabilities.max_tool_iterations, 50,
        "max_tool_iterations should equal expected value"
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
        "agency should equal expected value"
    );
    assert_eq!(
        config.agents.list[0].agency,
        Some(AgencyLevel::Restricted),
        "agency should match Some(AgencyLevel::Restricted)"
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
            prop_assert_eq!(&binding.channel, &back.channel, "channel should match channel");
            prop_assert_eq!(&binding.source, &back.source, "source should match source");
            prop_assert_eq!(&binding.nous_id, &back.nous_id, "nous_id should match nous_id");
            prop_assert_eq!(&binding.session_key, &back.session_key, "session_key should match session_key");
        }
    }
}
