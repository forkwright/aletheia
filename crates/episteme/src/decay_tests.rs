#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test code with known-valid indices"
)]

use super::*;

fn default_factors() -> DecayFactors {
    DecayFactors {
        age_hours: 0.0,
        fact_type: FactType::Observation,
        tier: EpistemicTier::Inferred,
        access_count: 0,
        reinforcement_count: 0,
        distinct_agent_count: 1,
        volatility: 0.5,
    }
}

#[test]
fn fresh_fact_has_high_decay_score() {
    let result = compute_decay(&DecayConfig::default(), &default_factors());
    // WHY: 0.47 -- recency is perfect but frequency/reinforcement are zero
    assert!(
        result.score > 0.4,
        "fresh fact should have moderate-high decay score, got {}",
        result.score
    );
    assert_eq!(
        result.stage,
        KnowledgeStage::Fading,
        "fresh unaccessed fact should be Fading"
    );
}

#[test]
fn old_untouched_fact_decays_to_low_score() {
    let factors = DecayFactors {
        age_hours: 5000.0,
        ..default_factors()
    };
    let result = compute_decay(&DecayConfig::default(), &factors);
    assert!(
        result.score < 0.5,
        "old fact should have low decay score, got {}",
        result.score
    );
}

#[test]
fn reinforcement_slows_decay() {
    let base = DecayFactors {
        age_hours: 500.0,
        ..default_factors()
    };
    let reinforced = DecayFactors {
        reinforcement_count: 20,
        ..base.clone()
    };
    let base_result = compute_decay(&DecayConfig::default(), &base);
    let reinforced_result = compute_decay(&DecayConfig::default(), &reinforced);
    assert!(
        reinforced_result.score > base_result.score,
        "reinforced fact should decay slower: {} vs {}",
        reinforced_result.score,
        base_result.score
    );
}

#[test]
fn cross_agent_access_slows_decay() {
    let single = DecayFactors {
        age_hours: 500.0,
        ..default_factors()
    };
    let multi = DecayFactors {
        distinct_agent_count: 4,
        ..single.clone()
    };
    let single_result = compute_decay(&DecayConfig::default(), &single);
    let multi_result = compute_decay(&DecayConfig::default(), &multi);
    assert!(
        multi_result.score > single_result.score,
        "multi-agent access should slow decay: {} vs {}",
        multi_result.score,
        single_result.score
    );
}

#[test]
fn cross_agent_multiplier_bounds() {
    assert!(
        (cross_agent_multiplier(
            0,
            DEFAULT_CROSS_AGENT_BONUS_PER_AGENT,
            DEFAULT_MAX_CROSS_AGENT_MULTIPLIER
        ) - 1.0)
            .abs()
            < f64::EPSILON,
        "zero agents should give 1.0 multiplier"
    );
    assert!(
        (cross_agent_multiplier(
            1,
            DEFAULT_CROSS_AGENT_BONUS_PER_AGENT,
            DEFAULT_MAX_CROSS_AGENT_MULTIPLIER
        ) - 1.0)
            .abs()
            < f64::EPSILON,
        "single agent should give 1.0 multiplier"
    );
    let two = cross_agent_multiplier(
        2,
        DEFAULT_CROSS_AGENT_BONUS_PER_AGENT,
        DEFAULT_MAX_CROSS_AGENT_MULTIPLIER,
    );
    assert!(
        (two - 1.15).abs() < f64::EPSILON,
        "two agents should give 1.15, got {two}"
    );
    let capped = cross_agent_multiplier(
        100,
        DEFAULT_CROSS_AGENT_BONUS_PER_AGENT,
        DEFAULT_MAX_CROSS_AGENT_MULTIPLIER,
    );
    assert!(
        (capped - DEFAULT_MAX_CROSS_AGENT_MULTIPLIER).abs() < f64::EPSILON,
        "should cap at {DEFAULT_MAX_CROSS_AGENT_MULTIPLIER}, got {capped}"
    );
}

#[test]
fn lifecycle_stage_from_decay_score() {
    assert_eq!(
        KnowledgeStage::from_decay_score(1.0),
        KnowledgeStage::Active
    );
    assert_eq!(
        KnowledgeStage::from_decay_score(0.7),
        KnowledgeStage::Active
    );
    assert_eq!(
        KnowledgeStage::from_decay_score(0.69),
        KnowledgeStage::Fading
    );
    assert_eq!(
        KnowledgeStage::from_decay_score(0.3),
        KnowledgeStage::Fading
    );
    assert_eq!(
        KnowledgeStage::from_decay_score(0.29),
        KnowledgeStage::Dormant
    );
    assert_eq!(
        KnowledgeStage::from_decay_score(0.1),
        KnowledgeStage::Dormant
    );
    assert_eq!(
        KnowledgeStage::from_decay_score(0.09),
        KnowledgeStage::Archived
    );
    assert_eq!(
        KnowledgeStage::from_decay_score(0.0),
        KnowledgeStage::Archived
    );
}

#[test]
fn stage_pruning_eligibility() {
    assert!(
        !KnowledgeStage::Active.is_prunable(),
        "Active should not be prunable"
    );
    assert!(
        !KnowledgeStage::Fading.is_prunable(),
        "Fading should not be prunable"
    );
    assert!(
        !KnowledgeStage::Dormant.is_prunable(),
        "Dormant should not be prunable"
    );
    assert!(
        KnowledgeStage::Archived.is_prunable(),
        "Archived should be prunable"
    );
}

#[test]
fn stage_default_recall_inclusion() {
    assert!(
        KnowledgeStage::Active.in_default_recall(),
        "Active should be in default recall"
    );
    assert!(
        KnowledgeStage::Fading.in_default_recall(),
        "Fading should be in default recall"
    );
    assert!(
        !KnowledgeStage::Dormant.in_default_recall(),
        "Dormant should not be in default recall"
    );
    assert!(
        !KnowledgeStage::Archived.in_default_recall(),
        "Archived should not be in default recall"
    );
}

#[test]
fn evaluate_transitions_detects_stage_change() {
    let fact_id = crate::id::FactId::new("f-1").expect("valid test id");
    let facts = vec![(fact_id.clone(), KnowledgeStage::Active, 0.2)];
    let transitions = evaluate_transitions(&facts);
    assert_eq!(transitions.len(), 1, "should detect one transition");
    assert_eq!(transitions[0].from, KnowledgeStage::Active);
    assert_eq!(transitions[0].to, KnowledgeStage::Dormant);
}

#[test]
fn evaluate_transitions_no_change() {
    let fact_id = crate::id::FactId::new("f-1").expect("valid test id");
    let facts = vec![(fact_id, KnowledgeStage::Active, 0.9)];
    let transitions = evaluate_transitions(&facts);
    assert!(transitions.is_empty(), "should detect no transitions");
}

#[test]
fn graduated_pruning_respects_time() {
    let fact_id = crate::id::FactId::new("f-1").expect("valid test id");
    let archived = vec![(fact_id.clone(), 0.05, 100.0)];
    let candidates = pruning_candidates(&archived, 720.0);
    assert!(
        candidates.is_empty(),
        "should not prune fact archived for less than min hours"
    );

    let long_archived = vec![(fact_id, 0.05, 800.0)];
    let candidates = pruning_candidates(&long_archived, 720.0);
    assert_eq!(
        candidates.len(),
        1,
        "should prune fact archived long enough"
    );
}

#[test]
fn verified_fact_decays_slower_than_assumed() {
    let verified = DecayFactors {
        age_hours: 500.0,
        tier: EpistemicTier::Verified,
        ..default_factors()
    };
    let assumed = DecayFactors {
        age_hours: 500.0,
        tier: EpistemicTier::Assumed,
        ..default_factors()
    };
    let v_result = compute_decay(&DecayConfig::default(), &verified);
    let a_result = compute_decay(&DecayConfig::default(), &assumed);
    assert!(
        v_result.score > a_result.score,
        "verified should decay slower: {} vs {}",
        v_result.score,
        a_result.score
    );
}

#[test]
fn identity_fact_decays_slower_than_observation() {
    let identity = DecayFactors {
        age_hours: 2000.0,
        fact_type: FactType::Identity,
        ..default_factors()
    };
    let observation = DecayFactors {
        age_hours: 2000.0,
        fact_type: FactType::Observation,
        ..default_factors()
    };
    let i_result = compute_decay(&DecayConfig::default(), &identity);
    let o_result = compute_decay(&DecayConfig::default(), &observation);
    assert!(
        i_result.score > o_result.score,
        "identity should decay slower: {} vs {}",
        i_result.score,
        o_result.score
    );
}

#[test]
fn volatile_domain_decays_faster() {
    let stable = DecayFactors {
        age_hours: 500.0,
        volatility: 0.0,
        ..default_factors()
    };
    let volatile = DecayFactors {
        age_hours: 500.0,
        volatility: 1.0,
        ..default_factors()
    };
    let s_result = compute_decay(&DecayConfig::default(), &stable);
    let v_result = compute_decay(&DecayConfig::default(), &volatile);
    assert!(
        s_result.score > v_result.score,
        "stable domain should decay slower: {} vs {}",
        s_result.score,
        v_result.score
    );
}

#[test]
fn score_recency_at_zero_is_one() {
    let s = score_recency(0.0, 720.0);
    assert!(
        (s - 1.0).abs() < f64::EPSILON,
        "recency at t=0 should be 1.0, got {s}"
    );
}

/// WHY (#3392): clock-jump-backward (negative age) is clamped to 0 so
/// decay returns 1.0 ("just now") instead of crashing or inflating
/// scores through downstream multipliers.
#[test]
fn score_recency_negative_age_clamps_to_fresh() {
    let s = score_recency(-12.5, 720.0);
    assert!(
        (s - 1.0).abs() < f64::EPSILON,
        "negative age should clamp to 1.0 (just-now), got {s}"
    );
}

#[test]
fn score_recency_nan_age_clamps_to_fresh() {
    let s = score_recency(f64::NAN, 720.0);
    assert!(
        (s - 1.0).abs() < f64::EPSILON,
        "NaN age should clamp to 1.0 (just-now), got {s}"
    );
}

#[test]
fn sanitize_age_hours_passes_through_non_negative_finite() {
    assert!((sanitize_age_hours(0.0) - 0.0).abs() < f64::EPSILON);
    assert!((sanitize_age_hours(42.0) - 42.0).abs() < f64::EPSILON);
}

#[test]
fn sanitize_age_hours_clamps_negative() {
    assert!((sanitize_age_hours(-1.0) - 0.0).abs() < f64::EPSILON);
    assert!((sanitize_age_hours(-1e12) - 0.0).abs() < f64::EPSILON);
}

#[test]
fn sanitize_age_hours_clamps_nan() {
    assert!((sanitize_age_hours(f64::NAN) - 0.0).abs() < f64::EPSILON);
}

#[test]
fn score_recency_at_stability_is_about_090() {
    let s = score_recency(720.0, 720.0);
    assert!(
        (s - 0.9).abs() < 0.01,
        "recency at t=S should be ~0.9, got {s}"
    );
}

#[test]
fn score_frequency_increases_with_access() {
    let low = score_frequency(1);
    let high = score_frequency(50);
    assert!(
        high > low,
        "higher access should give higher frequency score: {high} vs {low}"
    );
}

#[test]
fn score_reinforcement_caps() {
    let capped = score_reinforcement(
        100,
        DEFAULT_REINFORCEMENT_BOOST,
        DEFAULT_MAX_REINFORCEMENT_BONUS,
    );
    assert!(
        (capped - DEFAULT_MAX_REINFORCEMENT_BONUS).abs() < f64::EPSILON,
        "reinforcement should cap at {DEFAULT_MAX_REINFORCEMENT_BONUS}, got {capped}"
    );
}

/// WHY (#5869): Training facts are permanent and must not be affected by
/// domain volatility. In a stable domain the volatility multiplier would
/// otherwise inflate stability to 6x base instead of the documented 4x.
#[test]
fn training_tier_ignores_volatility_multiplier() {
    let stability = compute_effective_stability_with_reinforcement(
        FactType::Observation,
        EpistemicTier::Training,
        0,
        0,
        0.0, // stable domain: volatility_multiplier returns 1.5
        DEFAULT_REINFORCEMENT_BOOST,
        DEFAULT_MAX_REINFORCEMENT_BONUS,
    );
    // Observation base stability (72) * Training tier multiplier (4.0).
    let expected = 72.0 * 4.0;
    assert!(
        (stability - expected).abs() < f64::EPSILON,
        "Training tier should ignore volatility multiplier: {stability} != {expected}"
    );
}

/// WHY (#5866): every epistemic tier must have an explicit confidence score
/// so that Reflected and Training facts are not silently lumped with Assumed.
#[test]
fn score_confidence_respects_all_tiers() {
    assert!(
        (score_confidence(EpistemicTier::Verified) - 1.0).abs() < f64::EPSILON,
        "Verified should score 1.0"
    );
    assert!(
        (score_confidence(EpistemicTier::Reflected) - 0.9).abs() < f64::EPSILON,
        "Reflected should score 0.9"
    );
    assert!(
        (score_confidence(EpistemicTier::Inferred) - 0.6).abs() < f64::EPSILON,
        "Inferred should score 0.6"
    );
    assert!(
        (score_confidence(EpistemicTier::Assumed) - 0.3).abs() < f64::EPSILON,
        "Assumed should score 0.3"
    );
    assert!(
        (score_confidence(EpistemicTier::Training) - 1.0).abs() < f64::EPSILON,
        "Training should score 1.0"
    );
}

/// WHY (#5866): Training facts are permanent records of session outcomes and
/// must not drift into Fading/Archived just because of moderate age.
#[test]
fn training_fact_remains_active_at_moderate_age() {
    let factors = DecayFactors {
        age_hours: 500.0,
        fact_type: FactType::Observation,
        tier: EpistemicTier::Training,
        access_count: 50,
        reinforcement_count: 1,
        distinct_agent_count: 1,
        volatility: 0.5,
    };
    let result = compute_decay(&DecayConfig::default(), &factors);
    assert!(
        result.score > 0.7,
        "Training fact should retain an Active score, got {}",
        result.score
    );
    assert_eq!(
        result.stage,
        KnowledgeStage::Active,
        "Training fact with moderate age should remain Active"
    );
}

#[test]
fn knowledge_stage_serde_roundtrip() {
    for stage in [
        KnowledgeStage::Active,
        KnowledgeStage::Fading,
        KnowledgeStage::Dormant,
        KnowledgeStage::Archived,
    ] {
        let json = serde_json::to_string(&stage).expect("KnowledgeStage serialization");
        let back: KnowledgeStage =
            serde_json::from_str(&json).expect("KnowledgeStage deserialization");
        assert_eq!(stage, back, "KnowledgeStage should survive roundtrip");
    }
}

#[test]
fn knowledge_stage_from_str_roundtrip() {
    for stage in [
        KnowledgeStage::Active,
        KnowledgeStage::Fading,
        KnowledgeStage::Dormant,
        KnowledgeStage::Archived,
    ] {
        let parsed: KnowledgeStage = stage
            .as_str()
            .parse::<KnowledgeStage>()
            .expect("KnowledgeStage should parse from as_str");
        assert_eq!(stage, parsed, "KnowledgeStage roundtrip failed for {stage}");
    }
}
