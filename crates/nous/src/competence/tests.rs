use super::*;

fn tracker() -> CompetenceTracker {
    CompetenceTracker::open_in_memory().unwrap()
}

#[test]
fn default_score_is_half() {
    let t = tracker();
    let score = t.score("syn", "coding").unwrap();
    assert!(
        (score - 0.5).abs() < f64::EPSILON,
        "default score should be 0.5, got {score}"
    );
}

#[test]
fn success_increases_score() {
    let t = tracker();
    t.record_outcome("syn", "coding", TaskOutcome::Success)
        .unwrap();
    let score = t.score("syn", "coding").unwrap();
    assert!(
        score > DEFAULT_SCORE,
        "score after success should exceed default, got {score}"
    );
    assert!(
        (score - (DEFAULT_SCORE + SUCCESS_BONUS)).abs() < f64::EPSILON,
        "score should equal default + bonus"
    );
}

#[test]
fn failure_decreases_score() {
    let t = tracker();
    t.record_outcome("syn", "coding", TaskOutcome::Failure)
        .unwrap();
    let score = t.score("syn", "coding").unwrap();
    assert!(
        score < DEFAULT_SCORE,
        "score after failure should be below default, got {score}"
    );
}

#[test]
fn partial_does_not_change_score() {
    let t = tracker();
    t.record_outcome("syn", "coding", TaskOutcome::Partial)
        .unwrap();
    let score = t.score("syn", "coding").unwrap();
    assert!(
        (score - DEFAULT_SCORE).abs() < f64::EPSILON,
        "partial outcome should not change score"
    );
}

#[test]
fn score_clamps_at_minimum() {
    let t = tracker();
    for _ in 0..20 {
        t.record_outcome("syn", "coding", TaskOutcome::Failure)
            .unwrap();
    }
    let score = t.score("syn", "coding").unwrap();
    assert!(
        (score - MIN_SCORE).abs() < f64::EPSILON,
        "score should clamp at minimum {MIN_SCORE}, got {score}"
    );
}

#[test]
fn score_clamps_at_maximum() {
    let t = tracker();
    for _ in 0..50 {
        t.record_outcome("syn", "coding", TaskOutcome::Success)
            .unwrap();
    }
    let score = t.score("syn", "coding").unwrap();
    assert!(
        (score - MAX_SCORE).abs() < f64::EPSILON,
        "score should clamp at maximum {MAX_SCORE}, got {score}"
    );
}

#[test]
fn correction_decreases_score() {
    let t = tracker();
    t.record_outcome("syn", "coding", TaskOutcome::Success)
        .unwrap();
    let before = t.score("syn", "coding").unwrap();
    t.record_correction("syn", "coding").unwrap();
    let after = t.score("syn", "coding").unwrap();
    assert!(
        after < before,
        "correction should decrease score: {before} -> {after}"
    );
}

#[test]
fn disagreement_decreases_score() {
    let t = tracker();
    t.record_outcome("syn", "coding", TaskOutcome::Success)
        .unwrap();
    let before = t.score("syn", "coding").unwrap();
    t.record_disagreement("syn", "coding").unwrap();
    let after = t.score("syn", "coding").unwrap();
    assert!(
        after < before,
        "disagreement should decrease score: {before} -> {after}"
    );
}

#[test]
fn agent_competence_returns_all_domains() {
    let t = tracker();
    t.record_outcome("syn", "coding", TaskOutcome::Success)
        .unwrap();
    t.record_outcome("syn", "research", TaskOutcome::Failure)
        .unwrap();
    t.record_outcome("syn", "planning", TaskOutcome::Partial)
        .unwrap();

    let comp = t.agent_competence("syn").unwrap();
    assert_eq!(comp.domains.len(), 3, "should have 3 domains");
    assert_eq!(comp.nous_id, "syn");

    let coding = comp.domains.iter().find(|d| d.domain == "coding").unwrap();
    assert_eq!(coding.successes, 1);
    assert_eq!(coding.failures, 0);
}

#[test]
fn overall_score_averages_domains() {
    let t = tracker();
    t.record_outcome("syn", "coding", TaskOutcome::Success)
        .unwrap();
    t.record_outcome("syn", "research", TaskOutcome::Success)
        .unwrap();

    let comp = t.agent_competence("syn").unwrap();
    let expected = (DEFAULT_SCORE + SUCCESS_BONUS + DEFAULT_SCORE + SUCCESS_BONUS) / 2.0;
    assert!(
        (comp.overall_score - expected).abs() < f64::EPSILON,
        "overall score should average domains: expected {expected}, got {}",
        comp.overall_score
    );
}

#[test]
fn rolling_stats_respects_window() {
    let t = tracker();
    for _ in 0..10 {
        t.record_outcome("syn", "coding", TaskOutcome::Success)
            .unwrap();
    }
    for _ in 0..5 {
        t.record_outcome("syn", "coding", TaskOutcome::Failure)
            .unwrap();
    }

    let stats = t.rolling_stats("syn", "coding", 5).unwrap();
    assert_eq!(stats.total, 5, "window should contain 5 outcomes");
    assert_eq!(stats.failures, 5, "last 5 should all be failures");
    assert_eq!(stats.successes, 0);
}

#[test]
fn rolling_stats_empty_domain() {
    let t = tracker();
    let stats = t.rolling_stats("syn", "coding", 10).unwrap();
    assert_eq!(stats.total, 0);
    assert!((stats.failure_rate()).abs() < f64::EPSILON);
}

#[test]
fn escalation_recommended_on_high_failure_rate() {
    let t = tracker();
    for _ in 0..3 {
        t.record_outcome("syn", "debugging", TaskOutcome::Success)
            .unwrap();
    }
    for _ in 0..7 {
        t.record_outcome("syn", "debugging", TaskOutcome::Failure)
            .unwrap();
    }

    let rec = t.escalation_recommendation("syn", "debugging").unwrap();
    assert!(
        rec.should_escalate,
        "should recommend escalation with 70% failure rate"
    );
    assert!(rec.failure_rate > ESCALATION_FAILURE_THRESHOLD);
}

#[test]
fn no_escalation_with_few_samples() {
    let t = tracker();
    t.record_outcome("syn", "writing", TaskOutcome::Failure)
        .unwrap();
    t.record_outcome("syn", "writing", TaskOutcome::Failure)
        .unwrap();

    let rec = t.escalation_recommendation("syn", "writing").unwrap();
    assert!(
        !rec.should_escalate,
        "should not recommend escalation with fewer than {ESCALATION_MIN_SAMPLES} samples"
    );
}

#[test]
fn no_escalation_with_low_failure_rate() {
    let t = tracker();
    for _ in 0..8 {
        t.record_outcome("syn", "coding", TaskOutcome::Success)
            .unwrap();
    }
    for _ in 0..2 {
        t.record_outcome("syn", "coding", TaskOutcome::Failure)
            .unwrap();
    }

    let rec = t.escalation_recommendation("syn", "coding").unwrap();
    assert!(
        !rec.should_escalate,
        "should not recommend escalation with 20% failure rate"
    );
}

#[test]
fn domains_isolated_between_agents() {
    let t = tracker();
    t.record_outcome("syn", "coding", TaskOutcome::Success)
        .unwrap();
    t.record_outcome("demiurge", "coding", TaskOutcome::Failure)
        .unwrap();

    let syn_score = t.score("syn", "coding").unwrap();
    let demiurge_score = t.score("demiurge", "coding").unwrap();
    assert!(
        syn_score > demiurge_score,
        "agents should have independent scores"
    );
}

#[test]
fn task_outcome_roundtrip() {
    for outcome in [
        TaskOutcome::Success,
        TaskOutcome::Partial,
        TaskOutcome::Failure,
    ] {
        let s = outcome.as_str();
        let back = TaskOutcome::from_str(s);
        assert_eq!(back, Some(outcome), "roundtrip failed for {s}");
    }
}

#[test]
fn rolling_stats_rates() {
    let stats = RollingStats {
        window_size: 10,
        total: 10,
        successes: 7,
        partials: 1,
        failures: 2,
    };
    assert!((stats.success_rate() - 0.7).abs() < f64::EPSILON);
    assert!((stats.failure_rate() - 0.2).abs() < f64::EPSILON);
}

#[test]
fn agent_competence_empty_returns_default() {
    let t = tracker();
    let comp = t.agent_competence("nonexistent").unwrap();
    assert!(comp.domains.is_empty());
    assert!(
        (comp.overall_score - DEFAULT_SCORE).abs() < f64::EPSILON,
        "empty agent should have default overall score"
    );
}

#[test]
fn correction_increments_counter() {
    let t = tracker();
    t.record_outcome("syn", "coding", TaskOutcome::Success)
        .unwrap();
    t.record_correction("syn", "coding").unwrap();
    t.record_correction("syn", "coding").unwrap();

    let comp = t.agent_competence("syn").unwrap();
    let coding = comp.domains.iter().find(|d| d.domain == "coding").unwrap();
    assert_eq!(coding.corrections, 2, "should have recorded 2 corrections");
}
