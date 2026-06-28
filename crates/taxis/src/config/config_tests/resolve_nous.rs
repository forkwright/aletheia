#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;

#[test]
fn default_config_validates() {
    let config = AletheiaConfig::default();
    assert!(
        crate::validate::validate_config(&config).is_ok(),
        "default config must validate cleanly with new sections"
    );
}

#[test]
fn resolve_nous_uses_defaults_when_no_override() {
    let config = AletheiaConfig::default();
    let resolved = resolve_nous(&config, "test-agent");
    assert_eq!(resolved.recall_profile, RecallProfile::Default);
    assert_eq!(
        resolved.extraction.provider,
        BookkeepingProviderKind::Llm,
        "unknown agents should default to LLM extraction"
    );
    assert_eq!(
        resolved.behavior.knowledge_extraction_provider,
        BookkeepingProviderKind::Llm,
        "resolved behavior should carry the default extraction provider"
    );
    assert!(
        !resolved.private,
        "unknown agents should default to public discovery"
    );
    assert_eq!(
        resolved.episteme_cohort.as_ref(),
        "shared",
        "unknown agents should use the shared episteme cohort"
    );
    assert_eq!(
        resolved.behavior.distillation_context_token_trigger, 120_000,
        "default behavior should be used when no per-agent override is set"
    );
    assert_eq!(
        resolved.behavior.compaction_strategy,
        CompactionStrategyKind::UniformTail,
        "default behavior should preserve the uniform-tail compaction strategy"
    );
    assert!(
        (resolved.behavior.competence_correction_penalty - 0.05).abs() < f64::EPSILON,
        "default competence_correction_penalty must come from AgentBehaviorDefaults"
    );
}

#[test]
fn resolve_nous_per_agent_override_wins() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "custom".to_owned(),
        name: None,
        model: None,
        workspace: "/tmp/nous/custom".to_owned(),
        thinking_enabled: None,
        agency: None,
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        private: false,
        episteme_cohort: None,
        recall: None,
        recall_profile: None,
        behavior: Some(AgentBehaviorDefaults {
            compaction_strategy: CompactionStrategyKind::StepPositional,
            competence_correction_penalty: 0.10,
            ..Default::default()
        }),
        ..Default::default()
    });
    let resolved = resolve_nous(&config, "custom");
    assert!(
        (resolved.behavior.competence_correction_penalty - 0.10).abs() < f64::EPSILON,
        "per-agent behavior override must win over defaults"
    );
    assert_eq!(
        resolved.behavior.compaction_strategy,
        CompactionStrategyKind::StepPositional,
        "per-agent compaction strategy override must win over defaults"
    );
    // All other fields should remain at default
    assert_eq!(
        resolved.behavior.distillation_context_token_trigger, 120_000,
        "non-overridden fields must retain default values"
    );
}

#[test]
fn resolve_nous_non_overriding_agent_uses_defaults() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "plain".to_owned(),
        name: None,
        model: None,
        workspace: "/tmp/nous/plain".to_owned(),
        thinking_enabled: None,
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
    let resolved = resolve_nous(&config, "plain");
    assert_eq!(
        resolved.behavior.corrections_max_corrections, 50,
        "agent without behavior override should use shared defaults"
    );
    assert_eq!(
        resolved.behavior.compaction_strategy,
        CompactionStrategyKind::UniformTail,
        "agent without behavior override should use the default compaction strategy"
    );
}

#[test]
fn resolve_nous_recall_profile_override_wins() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "identity".to_owned(),
        name: None,
        model: None,
        workspace: "/tmp/nous/identity".to_owned(),
        thinking_enabled: None,
        agency: None,
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        private: true,
        episteme_cohort: None,
        recall: None,
        recall_profile: Some(RecallProfile::IdentityContinuity),
        behavior: None,
        ..Default::default()
    });

    let resolved = resolve_nous(&config, "identity");

    assert_eq!(resolved.recall_profile, RecallProfile::IdentityContinuity);
    assert!(
        resolved.private,
        "per-agent private flag should propagate into resolved config"
    );
}

#[test]
fn resolve_nous_episteme_cohort_override_wins() {
    let mut config = AletheiaConfig::default();
    config.agents.list.push(NousDefinition {
        id: "identity".to_owned(),
        name: None,
        model: None,
        workspace: "/tmp/nous/identity".to_owned(),
        thinking_enabled: None,
        agency: None,
        allowed_roots: Vec::new(),
        domains: Vec::new(),
        default: false,
        private: false,
        episteme_cohort: Some("identity".to_owned()),
        recall: None,
        recall_profile: None,
        behavior: None,
        ..Default::default()
    });

    let resolved = resolve_nous(&config, "identity");

    assert_eq!(resolved.episteme_cohort.as_ref(), "identity");
}

#[test]
fn new_deployment_sections_survive_serde_roundtrip() {
    let mut config = AletheiaConfig::default();
    config.nous_behavior.degraded_panic_threshold = 10;
    config.knowledge.conflict_max_candidates = 8;
    config.knowledge.extraction.provider = BookkeepingProviderKind::Gliner;
    config.provider_behavior.complexity_routing_enabled = true;
    config.provider_behavior.complexity_low_threshold = 25;
    config.api_limits.max_history_limit = 500;
    config.daemon_behavior.prosoche_anomaly_sample_size = 20;
    config.tool_limits.subprocess_timeout_secs = 120;
    config.messaging.circuit_breaker_threshold = 3;

    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(
        back.nous_behavior.degraded_panic_threshold, 10,
        "nous_behavior survives roundtrip"
    );
    assert_eq!(
        back.knowledge.conflict_max_candidates, 8,
        "knowledge survives roundtrip"
    );
    assert_eq!(
        back.knowledge.extraction.provider,
        BookkeepingProviderKind::Gliner,
        "knowledge extraction provider survives roundtrip"
    );
    assert!(
        back.provider_behavior.complexity_routing_enabled,
        "provider_behavior routing toggle survives roundtrip"
    );
    assert_eq!(
        back.provider_behavior.complexity_low_threshold, 25,
        "provider_behavior survives roundtrip"
    );
    assert_eq!(
        back.api_limits.max_history_limit, 500,
        "api_limits survives roundtrip"
    );
    assert_eq!(
        back.daemon_behavior.prosoche_anomaly_sample_size, 20,
        "daemon_behavior survives roundtrip"
    );
    assert_eq!(
        back.tool_limits.subprocess_timeout_secs, 120,
        "tool_limits survives roundtrip"
    );
    assert_eq!(
        back.messaging.circuit_breaker_threshold, 3,
        "messaging survives roundtrip"
    );
}

#[test]
fn agent_behavior_defaults_survive_serde_roundtrip() {
    let mut config = AletheiaConfig::default();
    config
        .agents
        .defaults
        .behavior
        .distillation_context_token_trigger = 80_000;
    config
        .agents
        .defaults
        .behavior
        .competence_correction_penalty = 0.08;
    config.agents.defaults.behavior.compaction_strategy = CompactionStrategyKind::StepPositional;

    let json = serde_json::to_string(&config).expect("serialize");
    let back: AletheiaConfig = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(
        back.agents
            .defaults
            .behavior
            .distillation_context_token_trigger,
        80_000,
        "agent behavior default survives serde roundtrip"
    );
    assert!(
        (back.agents.defaults.behavior.competence_correction_penalty - 0.08).abs() < f64::EPSILON,
        "competence_correction_penalty survives serde roundtrip"
    );
    assert_eq!(
        back.agents.defaults.behavior.compaction_strategy,
        CompactionStrategyKind::StepPositional,
        "compaction_strategy survives serde roundtrip"
    );
}

#[test]
fn new_deployment_sections_are_absent_in_default_toml() {
    // WHY: operators must be able to omit all new sections from aletheia.toml
    // and still get identical behaviour. This confirms `#[serde(default)]` works.
    let json = r"{}";

    let config: AletheiaConfig = serde_json::from_str(json).expect("parse empty json");
    assert_eq!(
        config.nous_behavior.degraded_panic_threshold, 5,
        "omitted nousBehavior section should use defaults"
    );
    assert_eq!(
        config.api_limits.idempotency_capacity, 10_000,
        "omitted apiLimits section should use defaults"
    );
    assert_eq!(
        config.messaging.poll_interval_ms, 2_000,
        "omitted messaging section should use defaults"
    );
    assert_eq!(
        config.agents.defaults.behavior.compaction_strategy,
        CompactionStrategyKind::UniformTail,
        "omitted compaction strategy should use the uniform-tail default"
    );
}

mod proptests {
    use proptest::prelude::*;

    use super::*;

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
