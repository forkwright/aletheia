#![expect(clippy::unwrap_used, reason = "test assertions")]

use super::*;

#[test]
fn registry_is_non_empty() {
    let specs = all_specs();
    assert!(
        specs.len() >= 30,
        "expected at least 30 parameter specs, got {}",
        specs.len()
    );
}

#[test]
fn all_keys_are_unique() {
    let specs = all_specs();
    let mut seen = std::collections::HashSet::new();
    for spec in specs {
        assert!(
            seen.insert(spec.key),
            "duplicate key in registry: {}",
            spec.key
        );
    }
}

#[test]
fn spec_by_key_finds_known_parameter() {
    let spec = spec_by_key("agents.defaults.behavior.distillationContextTokenTrigger");
    assert!(
        spec.is_some(),
        "expected to find distillation context token trigger spec"
    );
    let spec = spec.unwrap();
    assert_eq!(spec.tier, ParameterTier::SelfTuning);
}

#[test]
fn spec_by_key_finds_compaction_strategy() {
    let spec = spec_by_key("agents.defaults.behavior.compactionStrategy");
    assert!(spec.is_some(), "expected to find compaction strategy spec");
    let spec = spec.unwrap();
    assert_eq!(spec.tier, ParameterTier::PerAgent);
    assert_eq!(spec.default.to_string(), "uniform_tail");
}

#[test]
fn spec_by_key_returns_none_for_unknown() {
    assert!(spec_by_key("nonexistent.key").is_none());
}

#[test]
fn specs_by_section_filters_correctly() {
    let specs = specs_by_section("knowledge");
    assert!(
        !specs.is_empty(),
        "expected at least one knowledge section spec"
    );
    for spec in &specs {
        assert_eq!(spec.section, "knowledge");
    }
}

#[test]
fn specs_affecting_filters_correctly() {
    let specs = specs_affecting("distillation");
    assert!(
        !specs.is_empty(),
        "expected at least one spec affecting distillation"
    );
    for spec in &specs {
        assert!(
            spec.affects.contains("distillation"),
            "spec {} does not affect distillation",
            spec.key
        );
    }
}

#[test]
fn bounds_are_valid_where_present() {
    for spec in all_specs() {
        if let Some((min, max)) = spec.bounds {
            assert!(
                min <= max,
                "spec {}: bounds min ({}) > max ({})",
                spec.key,
                min,
                max
            );
        }
    }
}

#[test]
fn all_specs_have_non_empty_fields() {
    for spec in all_specs() {
        assert!(!spec.key.is_empty(), "spec has empty key");
        assert!(
            !spec.section.is_empty(),
            "spec {} has empty section",
            spec.key
        );
        assert!(
            !spec.description.is_empty(),
            "spec {} has empty description",
            spec.key
        );
        assert!(
            !spec.affects.is_empty(),
            "spec {} has empty affects",
            spec.key
        );
        assert!(
            !spec.outcome_signal.is_empty(),
            "spec {} has empty outcome_signal",
            spec.key
        );
        assert!(
            !spec.evidence_required.is_empty(),
            "spec {} has empty evidence_required",
            spec.key
        );
    }
}

#[test]
fn registry_exposes_validated_behavior_fields() {
    const VALIDATED_FIELDS: &[&str] = &[
        "nousBehavior.degradedPanicThreshold",
        "nousBehavior.degradedWindowSecs",
        "nousBehavior.inboxRecvTimeoutSecs",
        "nousBehavior.maxSpawnedTasks",
        "nousBehavior.cycleDetectionMaxLen",
        "nousBehavior.selfAuditEventThreshold",
        "nousBehavior.managerPingTimeoutSecs",
        "nousBehavior.managerMaxRestartBackoffSecs",
        "nousBehavior.managerRestartDrainTimeoutSecs",
        "nousBehavior.managerRestartDecayWindowSecs",
        "nousBehavior.shutdownTimeoutSecs",
        "messaging.bufferCapacity",
        "messaging.haltedHealthCheckIntervalSecs",
        "messaging.rpcTimeoutSecs",
        "messaging.healthTimeoutSecs",
        "messaging.receiveTimeoutSecs",
        "messaging.agentDispatchTimeoutSecs",
        "messaging.maxConcurrentHandlers",
        "apiLimits.maxSessionNameLen",
        "apiLimits.maxIdentifierBytes",
        "apiLimits.maxFactsLimit",
        "apiLimits.maxSearchLimit",
        "apiLimits.idempotencyMaxKeyLength",
        "providerBehavior.sseDefaultRetryMs",
        "providerBehavior.concurrencyEwmaAlpha",
        "providerBehavior.concurrencyLatencyThresholdSecs",
        "daemonBehavior.prosocheAnomalySampleSize",
        "daemonBehavior.runnerOutputBriefHeadLines",
        "daemonBehavior.runnerOutputBriefTailLines",
        "agents.defaults.behavior.compactionStrategy",
    ];

    for key in VALIDATED_FIELDS {
        assert!(
            spec_by_key(key).is_some(),
            "validated field {key} must be present in registry"
        );
    }
}

#[test]
fn registry_bounds_match_validator_ranges_for_drifted_fields() {
    let cases = [
        ("messaging.circuitBreakerThreshold", (1.0, 100.0)),
        ("apiLimits.maxHistoryLimit", (1.0, 100_000.0)),
        ("apiLimits.idempotencyCapacity", (100.0, 10_000_000.0)),
    ];

    for (key, bounds) in cases {
        assert_eq!(
            spec_by_key(key).map(|spec| spec.bounds),
            Some(Some(bounds)),
            "bounds drift for {key}"
        );
    }
}
