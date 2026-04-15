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
    assert!(spec.is_some(), "expected to find distillation context token trigger spec");
    let spec = spec.unwrap();
    assert_eq!(spec.tier, ParameterTier::SelfTuning);
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
        assert!(!spec.section.is_empty(), "spec {} has empty section", spec.key);
        assert!(!spec.description.is_empty(), "spec {} has empty description", spec.key);
        assert!(!spec.affects.is_empty(), "spec {} has empty affects", spec.key);
        assert!(!spec.outcome_signal.is_empty(), "spec {} has empty outcome_signal", spec.key);
        assert!(
            !spec.evidence_required.is_empty(),
            "spec {} has empty evidence_required",
            spec.key
        );
    }
}
