use super::*;

#[test]
fn canary_provider_returns_scenarios() {
    let provider = CanaryProvider;
    let scenarios = provider.provide();
    assert!(
        !scenarios.is_empty(),
        "canary provider should return scenarios"
    );
    assert_eq!(provider.name(), "canary");
}

#[test]
fn canary_scenarios_have_unique_ids() {
    let scenarios = canary_scenarios();
    let mut ids: Vec<&str> = scenarios.iter().map(|s| s.meta().id).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "duplicate canary scenario IDs detected");
}

#[test]
fn canary_scenarios_count() {
    let scenarios = canary_scenarios();
    assert_eq!(scenarios.len(), 25, "expected 25 canary scenarios");
}

#[test]
fn canary_scenarios_have_valid_categories() {
    let scenarios = canary_scenarios();
    let valid_categories = [
        "canary-recall",
        "canary-tool",
        "canary-session",
        "canary-knowledge",
        "canary-conflict",
    ];
    for s in &scenarios {
        let meta = s.meta();
        assert!(
            valid_categories.contains(&meta.category),
            "scenario {} has invalid category: {}",
            meta.id,
            meta.category
        );
    }
}
