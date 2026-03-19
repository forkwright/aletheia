//! Tests for individual scoring functions: vector similarity, FSRS decay, relevance, etc.
use super::super::*;

fn engine() -> RecallEngine {
    RecallEngine::new()
}

// --- Vector similarity ---

#[test]
fn vector_similarity_identical() {
    let e = engine();
    assert!(
        (e.score_vector_similarity(0.0) - 1.0).abs() < f64::EPSILON,
        "cosine distance 0.0 (identical vectors) should score 1.0"
    );
}

#[test]
fn vector_similarity_opposite() {
    let e = engine();
    assert!(
        (e.score_vector_similarity(2.0)).abs() < f64::EPSILON,
        "cosine distance 2.0 (opposite vectors) should score 0.0"
    );
}

#[test]
fn vector_similarity_midpoint() {
    let e = engine();
    assert!(
        (e.score_vector_similarity(1.0) - 0.5).abs() < f64::EPSILON,
        "cosine distance 1.0 (orthogonal vectors) should score 0.5"
    );
}

// --- FSRS Decay ---

#[test]
fn decay_at_zero_is_one() {
    let e = engine();
    // R(0) = 1.0 for any stability
    for ft in [
        FactType::Identity,
        FactType::Observation,
        FactType::Task,
        FactType::Event,
    ] {
        let score = e.score_decay(0.0, ft, EpistemicTier::Inferred, 0);
        assert!(
            (score - 1.0).abs() < f64::EPSILON,
            "R(0) should be 1.0 for {ft:?}, got {score}"
        );
    }
}

#[test]
fn decay_at_stability_approx_0_9() {
    // By FSRS design: R(S) = (1 + 19/81)^(-0.5) ≈ 0.9
    let e = engine();
    let ft = FactType::Event;
    let tier = EpistemicTier::Inferred;
    let s = compute_effective_stability(ft, tier, 0);
    let score = e.score_decay(s, ft, tier, 0);
    assert!(
        (score - 0.9).abs() < 0.01,
        "R(S) should be ~0.9, got {score}"
    );
}

#[test]
fn decay_identity_at_30_days_still_high() {
    let e = engine();
    let score = e.score_decay(720.0, FactType::Identity, EpistemicTier::Inferred, 0);
    assert!(
        score > 0.95,
        "Identity at 30 days should still be high, got {score}"
    );
}

#[test]
fn decay_observation_at_30_days_significantly_decayed() {
    let e = engine();
    let score = e.score_decay(720.0, FactType::Observation, EpistemicTier::Inferred, 0);
    assert!(
        score < 0.6,
        "Observation at 30 days should be significantly decayed, got {score}"
    );
    // Much lower than a fresh fact
    assert!(
        score < 0.9,
        "Observation at 30 days should be well below fresh, got {score}"
    );
}

#[test]
fn decay_verified_slower_than_inferred() {
    let e = engine();
    let age = 500.0;
    let ft = FactType::Event;
    let verified = e.score_decay(age, ft, EpistemicTier::Verified, 0);
    let inferred = e.score_decay(age, ft, EpistemicTier::Inferred, 0);
    assert!(
        verified > inferred,
        "Verified ({verified}) should decay slower than inferred ({inferred})"
    );
}

#[test]
fn decay_assumed_faster_than_inferred() {
    let e = engine();
    let age = 500.0;
    let ft = FactType::Event;
    let inferred = e.score_decay(age, ft, EpistemicTier::Inferred, 0);
    let assumed = e.score_decay(age, ft, EpistemicTier::Assumed, 0);
    assert!(
        inferred > assumed,
        "Inferred ({inferred}) should decay slower than assumed ({assumed})"
    );
}

#[test]
fn decay_high_access_slower() {
    let e = engine();
    let age = 500.0;
    let ft = FactType::Event;
    let tier = EpistemicTier::Inferred;
    let no_access = e.score_decay(age, ft, tier, 0);
    let high_access = e.score_decay(age, ft, tier, 100);
    assert!(
        high_access > no_access,
        "100-access ({high_access}) should decay slower than 0-access ({no_access})"
    );
}

#[test]
fn decay_negative_age_returns_one() {
    let e = engine();
    let score = e.score_decay(-10.0, FactType::Event, EpistemicTier::Inferred, 0);
    assert!(
        (score - 1.0).abs() < f64::EPSILON,
        "negative age should clamp to R=1.0, got {score}"
    );
}

#[test]
fn decay_very_old_near_zero() {
    let e = engine();
    let score = e.score_decay(
        1_000_000.0,
        FactType::Observation,
        EpistemicTier::Assumed,
        0,
    );
    assert!(
        score >= 0.0,
        "decay score must be non-negative, got {score}"
    );
    assert!(
        score < 0.02,
        "Very old observation should be near zero, got {score}"
    );
}

#[test]
fn decay_just_created_fact() {
    let e = engine();
    let score = e.score_decay(0.0, FactType::Task, EpistemicTier::Assumed, 0);
    assert!(
        (score - 1.0).abs() < f64::EPSILON,
        "just-created Task/Assumed fact should score R=1.0, got {score}"
    );
}

#[test]
fn decay_default_params_reasonable() {
    // With default Event/Inferred/0-access, should produce a reasonable curve
    let e = engine();
    let score_hour = e.score_decay(1.0, FactType::Event, EpistemicTier::Inferred, 0);
    let score_day = e.score_decay(24.0, FactType::Event, EpistemicTier::Inferred, 0);
    let score_week = e.score_decay(168.0, FactType::Event, EpistemicTier::Inferred, 0);
    assert!(
        score_hour > score_day,
        "1h score ({score_hour}) should be greater than 1d score ({score_day})"
    );
    assert!(
        score_day > score_week,
        "1d score ({score_day}) should be greater than 1w score ({score_week})"
    );
    assert!(
        score_hour > 0.9,
        "1h old Event should still be >0.9, got {score_hour}"
    );
    assert!(
        score_week > 0.5,
        "1w old Event should still be >0.5, got {score_week}"
    );
}

#[test]
fn decay_access_growth_logarithmic() {
    let ft = FactType::Event;
    let tier = EpistemicTier::Inferred;
    let s0 = compute_effective_stability(ft, tier, 0);
    let s10 = compute_effective_stability(ft, tier, 10);
    let s100 = compute_effective_stability(ft, tier, 100);
    let s1000 = compute_effective_stability(ft, tier, 1000);
    // Strictly increasing with access count
    assert!(
        s10 > s0,
        "stability with 10 accesses ({s10}) should exceed 0 accesses ({s0})"
    );
    assert!(
        s100 > s10,
        "stability with 100 accesses ({s100}) should exceed 10 accesses ({s10})"
    );
    assert!(
        s1000 > s100,
        "stability with 1000 accesses ({s1000}) should exceed 100 accesses ({s100})"
    );
    // Growth is bounded: even 1000 accesses doesn't double stability
    let growth_ratio = s1000 / s0;
    assert!(
        growth_ratio < 2.0,
        "1000-access growth ratio {growth_ratio} should be bounded below 2×"
    );
    // But it does grow meaningfully
    assert!(
        growth_ratio > 1.05,
        "1000-access growth ratio {growth_ratio} should be meaningful"
    );
}

// --- Relevance ---

#[test]
fn relevance_same_nous() {
    let e = engine();
    assert!(
        (e.score_relevance("syn", "syn") - 1.0).abs() < f64::EPSILON,
        "same nous_id should yield relevance 1.0"
    );
}

#[test]
fn relevance_shared() {
    let e = engine();
    assert!(
        (e.score_relevance("", "syn") - 0.5).abs() < f64::EPSILON,
        "shared (empty) memory nous_id should yield relevance 0.5"
    );
}

#[test]
fn relevance_other_nous() {
    let e = engine();
    assert!(
        (e.score_relevance("demiurge", "syn") - 0.3).abs() < f64::EPSILON,
        "different nous_id should yield relevance 0.3"
    );
}

// --- Epistemic tier ---

#[test]
fn epistemic_verified_highest() {
    let e = engine();
    let v = e.score_epistemic_tier("verified");
    let i = e.score_epistemic_tier("inferred");
    let a = e.score_epistemic_tier("assumed");
    assert!(
        v > i,
        "verified ({v}) should score higher than inferred ({i})"
    );
    assert!(
        i > a,
        "inferred ({i}) should score higher than assumed ({a})"
    );
}

// --- Relationship proximity ---

#[test]
fn proximity_direct_neighbor() {
    let e = engine();
    assert!(
        (e.score_relationship_proximity(Some(1)) - 1.0).abs() < f64::EPSILON,
        "direct neighbor (1 hop) should score proximity 1.0"
    );
}

#[test]
fn proximity_two_hops() {
    let e = engine();
    assert!(
        (e.score_relationship_proximity(Some(2)) - 0.5).abs() < f64::EPSILON,
        "two hops should score proximity 0.5"
    );
}

#[test]
fn proximity_no_connection() {
    let e = engine();
    assert!(
        (e.score_relationship_proximity(None)).abs() < f64::EPSILON,
        "no connection (None) should score proximity 0.0"
    );
}

// --- Access frequency ---

#[test]
fn access_frequency_zero() {
    let e = engine();
    assert!(
        (e.score_access_frequency(0)).abs() < 0.01,
        "zero accesses should score near 0.0"
    );
}

#[test]
fn access_frequency_max() {
    let e = engine();
    assert!(
        (e.score_access_frequency(100) - 1.0).abs() < 0.01,
        "100 accesses (max) should score near 1.0"
    );
}

#[test]
fn access_frequency_logarithmic() {
    let e = engine();
    let s10 = e.score_access_frequency(10);
    let s50 = e.score_access_frequency(50);
    let s100 = e.score_access_frequency(100);
    // Logarithmic: each doubling adds less
    assert!(
        s50 > s10,
        "score at 50 accesses ({s50}) should exceed score at 10 ({s10})"
    );
    assert!(
        s100 > s50,
        "score at 100 accesses ({s100}) should exceed score at 50 ({s50})"
    );
    assert!(
        s50 - s10 > s100 - s50,
        "logarithmic growth: gain from 10→50 ({}) should exceed gain from 50→100 ({})",
        s50 - s10,
        s100 - s50
    );
}

// --- Composite scoring ---

#[test]
fn perfect_score() {
    let e = engine();
    let factors = FactorScores {
        vector_similarity: 1.0,
        decay: 1.0,
        relevance: 1.0,
        epistemic_tier: 1.0,
        relationship_proximity: 1.0,
        access_frequency: 1.0,
    };
    assert!(
        (e.compute_score(&factors) - 1.0).abs() < 0.01,
        "all factors at 1.0 should produce a composite score of ~1.0"
    );
}

#[test]
fn zero_score() {
    let e = engine();
    let factors = FactorScores::default();
    assert!(
        (e.compute_score(&factors)).abs() < f64::EPSILON,
        "all factors at 0.0 (default) should produce a composite score of 0.0"
    );
}

#[test]
fn vector_similarity_dominates() {
    let e = engine();
    let high_vec = FactorScores {
        vector_similarity: 1.0,
        ..FactorScores::default()
    };
    let high_decay = FactorScores {
        decay: 1.0,
        ..FactorScores::default()
    };
    assert!(
        e.compute_score(&high_vec) > e.compute_score(&high_decay),
        "vector_similarity weight (0.35) should dominate decay weight (0.20): high_vec={}, high_decay={}",
        e.compute_score(&high_vec),
        e.compute_score(&high_decay)
    );
}
