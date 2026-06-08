//! Tests for speculative episteme surfaces wired into the recall pipeline:
//! Bayesian surprise and evidence-gap coverage.
use std::collections::HashMap;

use super::super::*;

fn engine_with_surprise_weight(w: f64) -> RecallEngine {
    RecallEngine::with_weights(RecallWeights {
        surprise: w,
        serendipity: 0.0,
        ..RecallWeights::default()
    })
}

fn engine_with_evidence_weight(w: f64) -> RecallEngine {
    RecallEngine::with_weights(RecallWeights {
        evidence_coverage: w,
        serendipity: 0.0,
        ..RecallWeights::default()
    })
}

// ── surprise scoring ────────────────────────────────────────────────────────

/// When `RecallWeights::surprise` is zero (default), `score_surprise` must
/// return 0.0 regardless of the raw KL-divergence value so the inert default
/// is enforced by the engine rather than relying on callers to opt out.
#[test]
fn surprise_inert_when_weight_zero() {
    let e = RecallEngine::new();
    // Any surprise_nats value should produce 0.0 when the weight is 0.
    for nats in [0.0_f64, 1.0, 2.0, 5.0, 10.0] {
        let score = e.score_surprise(nats, 2.0);
        assert!(
            score.abs() < f64::EPSILON,
            "surprise should be 0.0 with default weight 0.0, got {score} for nats={nats}"
        );
    }
}

/// `score_surprise` is monotone: higher KL-divergence → higher score when
/// weight is non-zero.
#[test]
fn surprise_monotone_with_nonzero_weight() {
    let e = engine_with_surprise_weight(0.1);
    let low = e.score_surprise(0.5, 2.0);
    let mid = e.score_surprise(2.0, 2.0);
    let high = e.score_surprise(5.0, 2.0);
    assert!(
        low < mid,
        "score at 0.5 nats ({low}) should be < at 2.0 ({mid})"
    );
    assert!(
        mid < high,
        "score at 2.0 nats ({mid}) should be < at 5.0 ({high})"
    );
}

/// At the midpoint the sigmoid returns 0.5 — the "neutral" value.
#[test]
fn surprise_at_midpoint_is_half() {
    let e = engine_with_surprise_weight(0.1);
    let score = e.score_surprise(2.0, 2.0);
    assert!(
        (score - 0.5).abs() < 1e-9,
        "surprise at midpoint should be 0.5, got {score}"
    );
}

/// A non-zero surprise weight makes a high-surprise candidate rank above an
/// identical candidate with no surprise signal.
#[test]
fn surprise_weight_affects_final_rank() {
    let e = engine_with_surprise_weight(0.5);

    let base_factors = FactorScores {
        vector_similarity: 0.6,
        decay: 0.8,
        relevance: 1.0,
        serendipity: 0.0,
        ..FactorScores::default()
    };

    let score_no_surprise = e.compute_score(&FactorScores {
        surprise: 0.0,
        ..base_factors.clone()
    });
    let score_high_surprise = e.compute_score(&FactorScores {
        surprise: e.score_surprise(6.0, 2.0),
        ..base_factors.clone()
    });

    assert!(
        score_high_surprise > score_no_surprise,
        "high-surprise candidate ({score_high_surprise}) should outscore no-surprise ({score_no_surprise})"
    );
}

/// Verifies the zero-weight fast path for surprise is active at default.
#[test]
fn surprise_default_weight_is_zero() {
    let w = RecallWeights::default();
    assert!(
        w.surprise.abs() < f64::EPSILON,
        "default surprise weight should be 0.0, preserving existing behaviour"
    );
}

// ── evidence_coverage scoring ───────────────────────────────────────────────

/// When `RecallWeights::evidence_coverage` is zero (default), `score_evidence_coverage`
/// returns 0.0 regardless of the answered-ids map so the inert default is enforced
/// by the engine rather than relying on callers to opt out.
#[test]
fn evidence_coverage_inert_when_weight_zero() {
    let e = RecallEngine::new();
    let mut answered = HashMap::new();
    answered.insert("fact-001".to_owned(), 0.95_f64);

    let score = e.score_evidence_coverage("fact-001", &answered);
    assert!(
        score.abs() < f64::EPSILON,
        "evidence_coverage should be 0.0 with default weight 0.0, got {score}"
    );
}

/// A candidate whose `source_id` is in the answered map scores the stored
/// confidence when the weight is non-zero.
#[test]
fn evidence_coverage_hit_returns_confidence() {
    let e = engine_with_evidence_weight(0.2);
    let mut answered = HashMap::new();
    answered.insert("fact-001".to_owned(), 0.85_f64);

    let score = e.score_evidence_coverage("fact-001", &answered);
    assert!(
        (score - 0.85).abs() < f64::EPSILON,
        "evidence_coverage hit should return stored confidence 0.85, got {score}"
    );
}

/// A candidate absent from the answered map scores 0.0.
#[test]
fn evidence_coverage_miss_returns_zero() {
    let e = engine_with_evidence_weight(0.2);
    let mut answered = HashMap::new();
    answered.insert("fact-001".to_owned(), 0.9_f64);

    let score = e.score_evidence_coverage("fact-999", &answered);
    assert!(
        score.abs() < f64::EPSILON,
        "evidence_coverage miss should score 0.0, got {score}"
    );
}

/// An evidence-covered candidate ranks above an uncovered candidate when the
/// weight is non-zero and all other factors are equal.
#[test]
fn evidence_coverage_weight_affects_final_rank() {
    let e = engine_with_evidence_weight(0.5);

    let base_factors = FactorScores {
        vector_similarity: 0.7,
        decay: 0.9,
        relevance: 1.0,
        serendipity: 0.0,
        ..FactorScores::default()
    };

    let score_uncovered = e.compute_score(&FactorScores {
        evidence_coverage: 0.0,
        ..base_factors.clone()
    });
    let score_covered = e.compute_score(&FactorScores {
        evidence_coverage: 0.9,
        ..base_factors.clone()
    });

    assert!(
        score_covered > score_uncovered,
        "covered ({score_covered}) should outscore uncovered ({score_uncovered})"
    );
}

/// Verifies the zero-weight fast path for `evidence_coverage` is active at default.
#[test]
fn evidence_coverage_default_weight_is_zero() {
    let w = RecallWeights::default();
    assert!(
        w.evidence_coverage.abs() < f64::EPSILON,
        "default evidence_coverage weight should be 0.0, preserving existing behaviour"
    );
}

/// Confidence values out of [0.0, 1.0] are clamped.
#[test]
fn evidence_coverage_clamps_out_of_range() {
    let e = engine_with_evidence_weight(0.2);
    let mut answered = HashMap::new();
    answered.insert("fact-001".to_owned(), 5.0_f64); // way out of range

    let score = e.score_evidence_coverage("fact-001", &answered);
    assert!(
        (score - 1.0).abs() < f64::EPSILON,
        "confidence above 1.0 should clamp to 1.0, got {score}"
    );
}

// ── combined weight total ────────────────────────────────────────────────────

/// `RecallWeights::total()` includes `surprise` and `evidence_coverage` weights.
#[test]
fn total_includes_speculative_weights() {
    let w = RecallWeights {
        surprise: 0.05,
        evidence_coverage: 0.05,
        serendipity: 0.0,
        ..RecallWeights::default()
    };
    let base = RecallWeights::default().total();
    let augmented = w.total();
    assert!(
        (augmented - (base + 0.10)).abs() < 1e-9,
        "total should increase by 0.10 when surprise=0.05 and evidence_coverage=0.05"
    );
}
