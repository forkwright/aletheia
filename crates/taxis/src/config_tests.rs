#![expect(clippy::expect_used, reason = "test assertions")]

use super::*;

#[test]
fn defaults_are_sensible() {
    let config = AletheiaConfig::default();
    assert_eq!(config.agents.defaults.context_tokens, 200_000);
    assert_eq!(config.agents.defaults.max_output_tokens, 16_384);
    assert_eq!(config.agents.defaults.bootstrap_max_tokens, 40_000);
    assert_eq!(config.agents.defaults.model.primary, "claude-sonnet-4-6");
    assert!(!config.agents.defaults.thinking_enabled);
    assert_eq!(config.agents.defaults.thinking_budget, 10_000);
    assert_eq!(config.agents.defaults.max_tool_iterations, 50);
    assert_eq!(config.gateway.port, 18789);
    assert_eq!(config.gateway.bind, "localhost");
    assert_eq!(config.gateway.auth.mode, "token");
    // Security config defaults
    assert!(!config.gateway.tls.enabled);
    assert!(config.gateway.tls.cert_path.is_none());
    assert!(config.gateway.cors.allowed_origins.is_empty());
    assert_eq!(config.gateway.cors.max_age_secs, 3600);
    assert_eq!(config.gateway.body_limit.max_bytes, 1_048_576);
    assert!(config.gateway.csrf.enabled);
    assert_eq!(config.gateway.csrf.header_name, "x-requested-with");
    assert_eq!(config.gateway.csrf.header_value, "aletheia");
    assert!(!config.gateway.rate_limit.enabled);
    assert_eq!(config.gateway.rate_limit.requests_per_minute, 60);
    assert!(config.channels.signal.enabled);
    assert!(config.channels.signal.accounts.is_empty());
    assert!(config.bindings.is_empty());
    assert_eq!(config.embedding.provider, "candle");
    assert!(config.embedding.model.is_none());
    assert_eq!(config.embedding.dimension, 384);
    // Maintenance defaults
    assert!(config.maintenance.trace_rotation.enabled);
    assert_eq!(config.maintenance.trace_rotation.max_age_days, 14);
    assert!(config.maintenance.drift_detection.enabled);
    assert!(config.maintenance.db_monitoring.enabled);
    assert_eq!(config.maintenance.db_monitoring.warn_threshold_mb, 100);
    assert!(!config.maintenance.retention.enabled);
    assert!(config.pricing.is_empty());
}

#[test]
fn serde_roundtrip() {
    let config = AletheiaConfig::default();
    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.agents.defaults.context_tokens, 200_000);
    assert_eq!(back.gateway.port, 18789);
    assert!(back.channels.signal.enabled);
    assert_eq!(back.embedding.provider, "candle");
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
                "bootstrapMaxTokens": 20000
            },
            "list": []
        }
    }"#;
    let config: AletheiaConfig = serde_json::from_str(yaml).expect("parse camelCase");
    assert_eq!(config.agents.defaults.context_tokens, 100_000);
    assert_eq!(config.agents.defaults.max_output_tokens, 8192);
    assert_eq!(config.agents.defaults.bootstrap_max_tokens, 20_000);
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
        recall: None,
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
        recall: None,
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
        recall: None,
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
            "provider": "candle",
            "model": "BAAI/bge-small-en-v1.5",
            "dimension": 512
        }
    }"#;
    let config: AletheiaConfig = serde_json::from_str(json).expect("parse embedding");
    assert_eq!(config.embedding.provider, "candle");
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

#[test]
fn pricing_defaults_empty() {
    let config = AletheiaConfig::default();
    assert!(config.pricing.is_empty());
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
    assert_eq!(config.pricing.len(), 2);
    let opus = &config.pricing["claude-opus-4-6"];
    assert!((opus.input_cost_per_mtok - 15.0).abs() < f64::EPSILON);
    assert!((opus.output_cost_per_mtok - 75.0).abs() < f64::EPSILON);
    let sonnet = &config.pricing["claude-sonnet-4-6"];
    assert!((sonnet.input_cost_per_mtok - 3.0).abs() < f64::EPSILON);
    assert!((sonnet.output_cost_per_mtok - 15.0).abs() < f64::EPSILON);
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
