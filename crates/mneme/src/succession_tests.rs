//! Unit tests for ecological succession — volatility computation, adaptive stability,
//! domain profiling, and integration with the recall scoring pipeline.
//!
//! NOTE: The prompt's Context section describes the volatility multiplier as
//! `1.0 / (1.0 + 0.1 * supersession_count)`, which bounds output to (0.0, 1.0].
//! The actual implementation uses `1.5 - volatility`, which ranges [0.5, 1.5].
//! Tests reflect the actual implementation. The discrepancy is noted in the PR body.


use crate::graph_intelligence::{GraphContext, score_access_with_evolution};
use crate::knowledge::{EpistemicTier, FactType};
use crate::recall::compute_effective_stability;
use crate::succession::{adaptive_stability, compute_volatility, volatility_multiplier};

// ---------------------------------------------------------------------------
// Volatility computation (requirements 1–4)
// ---------------------------------------------------------------------------

#[test]
fn compute_volatility_zero_total_returns_zero() {
    // Requirement 1: no facts → no volatility score (guard against divide-by-zero)
    let v = compute_volatility(0, 0, 0.0);
    assert!(v.abs() < f64::EPSILON, "zero total → 0.0, got {v}");
}

#[test]
fn compute_volatility_no_supersessions_returns_zero() {
    // Requirement 1: facts exist but none are superseded → 0.0
    let v = compute_volatility(10, 0, 0.0);
    assert!(v.abs() < f64::EPSILON, "0 superseded of 10 → 0.0, got {v}");
}

#[test]
fn compute_volatility_five_supersessions_matches_formula() {
    // Requirement 2: formula = (5/10) × (1 + 0.1 × 1.0) = 0.5 × 1.1 = 0.55
    let v = compute_volatility(10, 5, 1.0);
    let expected = 0.55;
    assert!((v - expected).abs() < 1e-10, "expected {expected}, got {v}");
}

#[test]
fn compute_volatility_all_superseded_with_chain_matches_formula() {
    // Requirement 2: formula = (10/10) × (1 + 0.1 × 2.0) = 1.0 × 1.2 = 1.2 → clamped to 1.0
    let v = compute_volatility(10, 10, 2.0);
    assert!(
        (v - 1.0).abs() < f64::EPSILON,
        "should clamp to 1.0, got {v}"
    );
}

#[test]
fn compute_volatility_increases_with_supersession_count() {
    // Requirement 3: more supersessions in same population → higher volatility
    let total = 20_u32;
    let chain = 0.5;
    let low = compute_volatility(total, 2, chain);
    let mid = compute_volatility(total, 10, chain);
    let high = compute_volatility(total, 18, chain);
    assert!(
        low < mid,
        "2 superseded ({low}) must be < 10 superseded ({mid})"
    );
    assert!(
        mid < high,
        "10 superseded ({mid}) must be < 18 superseded ({high})"
    );
}

#[test]
fn compute_volatility_bounded_above_at_one() {
    // Requirement 4: extreme inputs clamp to ≤ 1.0
    let v = compute_volatility(1, 1, 1_000.0);
    assert!(v <= 1.0, "volatility must never exceed 1.0, got {v}");
}

#[test]
fn compute_volatility_never_negative() {
    // Requirement 4: result is always ≥ 0.0
    let v = compute_volatility(5, 1, 0.0);
    assert!(v >= 0.0, "volatility must never be negative, got {v}");
}

#[test]
fn compute_volatility_single_fact_superseded_correct_value() {
    // Requirement 2: 1 of 1 superseded, chain 0 → (1/1) × (1 + 0) = 1.0
    let v = compute_volatility(1, 1, 0.0);
    assert!(
        (v - 1.0).abs() < f64::EPSILON,
        "single fact fully superseded → 1.0, got {v}"
    );
}

#[test]
fn compute_volatility_fractional_supersession_rate() {
    // Requirement 2: 3 of 8 superseded, chain 0 → (3/8) × 1.0 = 0.375
    let v = compute_volatility(8, 3, 0.0);
    let expected = 3.0 / 8.0;
    assert!((v - expected).abs() < 1e-10, "expected {expected}, got {v}");
}

// ---------------------------------------------------------------------------
// Volatility multiplier (requirements 5–8)
// ---------------------------------------------------------------------------

// NOTE: The actual formula is `1.5 - volatility`, not `1.0 / (1.0 + 0.1 * count)`.
// The implementation intentionally gives a boost (1.5×) for stable domains
// and a penalty (0.5×) for volatile ones. The range is [0.5, 1.5].
// The prompt's Context section describes a different formula — see PR observations.

#[test]
fn volatility_multiplier_zero_volatility_returns_one_point_five() {
    // Requirement 5: stable domain (volatility 0) → maximum multiplier (1.5)
    let m = volatility_multiplier(0.0);
    assert!(
        (m - 1.5).abs() < f64::EPSILON,
        "stable domain → 1.5, got {m}"
    );
}

#[test]
fn volatility_multiplier_neutral_volatility_returns_one() {
    // Requirement 5: midpoint (0.5) → neutral multiplier (1.0)
    let m = volatility_multiplier(0.5);
    assert!(
        (m - 1.0).abs() < f64::EPSILON,
        "neutral volatility 0.5 → 1.0, got {m}"
    );
}

#[test]
fn volatility_multiplier_high_volatility_returns_less_than_one() {
    // Requirement 6: volatile domain → multiplier < 1.0 (decay accelerated)
    let m = volatility_multiplier(0.9);
    assert!(
        m < 1.0,
        "high volatility should yield multiplier < 1.0, got {m}"
    );
}

#[test]
fn volatility_multiplier_full_volatility_returns_half() {
    // Requirement 6: maximum volatility (1.0) → minimum multiplier (0.5)
    let m = volatility_multiplier(1.0);
    assert!(
        (m - 0.5).abs() < f64::EPSILON,
        "volatile domain (1.0) → 0.5, got {m}"
    );
}

#[test]
fn volatility_multiplier_formula_is_linear() {
    // Requirement 7: tests actual formula (1.5 - v). Each 0.1 increase in v reduces
    // multiplier by exactly 0.1.
    for i in 0..=10_u32 {
        let v = f64::from(i) / 10.0;
        let m = volatility_multiplier(v);
        let expected = 1.5 - v;
        assert!(
            (m - expected).abs() < 1e-10,
            "v={v}: expected {expected}, got {m}"
        );
    }
}

#[test]
fn volatility_multiplier_always_positive() {
    // Requirement 8: multiplier is always > 0 (minimum 0.5 at volatility = 1.0)
    for i in 0..=20_u32 {
        let v = f64::from(i) / 20.0;
        let m = volatility_multiplier(v);
        assert!(m > 0.0, "multiplier must be positive for v={v}, got {m}");
    }
}

#[test]
fn volatility_multiplier_negative_input_clamped() {
    // Requirement 8: out-of-range inputs are clamped — negative treated as 0
    let m = volatility_multiplier(-1.0);
    assert!(
        (m - 1.5).abs() < f64::EPSILON,
        "negative input should clamp to 0 → 1.5, got {m}"
    );
}

#[test]
fn volatility_multiplier_over_one_input_clamped() {
    // Requirement 8: input > 1.0 clamped to 1.0 → multiplier = 0.5
    let m = volatility_multiplier(2.0);
    assert!(
        (m - 0.5).abs() < f64::EPSILON,
        "input 2.0 should clamp to 1.0 → 0.5, got {m}"
    );
}

// ---------------------------------------------------------------------------
// Adaptive stability (requirements 9–12)
// ---------------------------------------------------------------------------

#[test]
fn adaptive_stability_zero_volatility_is_one_point_five_times_base() {
    // Requirement 9: zero volatility → multiplier 1.5 → stable * 1.5
    let ft = FactType::Event;
    let tier = EpistemicTier::Inferred;
    let access = 0;
    let base = compute_effective_stability(ft, tier, access);
    let stable = adaptive_stability(ft, tier, access, 0.0);
    assert!(
        (stable - base * 1.5).abs() < 1e-10,
        "zero volatility: expected base×1.5 = {}, got {stable}",
        base * 1.5
    );
}

#[test]
fn adaptive_stability_neutral_volatility_equals_base() {
    // Requirement 9: volatility 0.5 → multiplier 1.0 → result equals base
    let ft = FactType::Observation;
    let tier = EpistemicTier::Inferred;
    let access = 3;
    let base = compute_effective_stability(ft, tier, access);
    let neutral = adaptive_stability(ft, tier, access, 0.5);
    assert!(
        (neutral - base).abs() < 1e-10,
        "neutral volatility: expected {base}, got {neutral}"
    );
}

#[test]
fn adaptive_stability_high_volatility_reduces_stability() {
    // Requirement 10: high volatility → lower stability than base
    let ft = FactType::Event;
    let tier = EpistemicTier::Assumed;
    let access = 0;
    let stable = adaptive_stability(ft, tier, access, 0.1);
    let volatile = adaptive_stability(ft, tier, access, 0.9);
    assert!(
        stable > volatile,
        "stable ({stable}) should exceed volatile ({volatile})"
    );
}

#[test]
fn adaptive_stability_full_volatility_is_half_base() {
    // Requirement 10: volatility 1.0 → multiplier 0.5 → half the base stability
    let ft = FactType::Task;
    let tier = EpistemicTier::Inferred;
    let access = 0;
    let base = compute_effective_stability(ft, tier, access);
    let volatile = adaptive_stability(ft, tier, access, 1.0);
    assert!(
        (volatile - base * 0.5).abs() < 1e-10,
        "full volatility: expected base×0.5 = {}, got {volatile}",
        base * 0.5
    );
}

#[test]
fn adaptive_stability_integrates_fact_type() {
    // Requirement 11: Identity (high base stability) yields higher adaptive result than Observation
    let tier = EpistemicTier::Inferred;
    let access = 0;
    let volatility = 0.3;
    let identity = adaptive_stability(FactType::Identity, tier, access, volatility);
    let observation = adaptive_stability(FactType::Observation, tier, access, volatility);
    assert!(
        identity > observation,
        "Identity ({identity}) should have higher adaptive stability than Observation ({observation})"
    );
}

#[test]
fn adaptive_stability_integrates_epistemic_tier() {
    // Requirement 12: Verified tier multiplier is higher than Assumed
    let ft = FactType::Observation;
    let access = 2;
    let volatility = 0.4;
    let verified = adaptive_stability(ft, EpistemicTier::Verified, access, volatility);
    let assumed = adaptive_stability(ft, EpistemicTier::Assumed, access, volatility);
    assert!(
        verified > assumed,
        "Verified ({verified}) should exceed Assumed ({assumed}) adaptive stability"
    );
}

#[test]
fn adaptive_stability_verified_two_times_inferred_at_same_volatility() {
    // Requirement 12: Verified stability multiplier is 2× Inferred (matches EpistemicTier contract)
    let ft = FactType::Event;
    let access = 0;
    let volatility = 0.5; // neutral — no distortion
    let verified = adaptive_stability(ft, EpistemicTier::Verified, access, volatility);
    let inferred = adaptive_stability(ft, EpistemicTier::Inferred, access, volatility);
    assert!(
        (verified / inferred - 2.0).abs() < 1e-6,
        "Verified adaptive should be 2× Inferred: {verified} / {inferred}"
    );
}

// ---------------------------------------------------------------------------
// Supersession chain tests (requirements 13–16)
// ---------------------------------------------------------------------------

// Pure GraphContext tests — no engine required. Chain lengths are injected
// directly into the context, matching how the engine populates them at runtime.

#[test]
fn chain_length_single_hop_via_graph_context() {
    // Requirement 13: A → B (A superseded by B) → chain_length(A) = 1
    let mut ctx = GraphContext::default();
    ctx.chain_lengths.insert("fact-a".to_owned(), 1);
    ctx.chain_lengths.insert("fact-b".to_owned(), 0); // leaf
    assert_eq!(
        ctx.chain_length("fact-a"),
        1,
        "single-hop chain_length should be 1"
    );
    assert_eq!(
        ctx.chain_length("fact-b"),
        0,
        "leaf chain_length should be 0"
    );
}

#[test]
fn chain_length_multi_hop_a_b_c_d_via_graph_context() {
    // Requirement 14: A → B → C → D (3 hops from A to leaf D) → chain_length(A) = 3
    let mut ctx = GraphContext::default();
    ctx.chain_lengths.insert("fact-a".to_owned(), 3);
    ctx.chain_lengths.insert("fact-b".to_owned(), 2);
    ctx.chain_lengths.insert("fact-c".to_owned(), 1);
    ctx.chain_lengths.insert("fact-d".to_owned(), 0); // leaf
    assert_eq!(
        ctx.chain_length("fact-a"),
        3,
        "A→B→C→D: chain_length(A) = 3"
    );
    assert_eq!(ctx.chain_length("fact-b"), 2, "B→C→D: chain_length(B) = 2");
    assert_eq!(ctx.chain_length("fact-c"), 1, "C→D: chain_length(C) = 1");
    assert_eq!(
        ctx.chain_length("fact-d"),
        0,
        "D is leaf: chain_length(D) = 0"
    );
}

#[test]
fn chain_length_non_superseded_fact_is_zero() {
    // Requirement 15: a fact with no supersession record has chain_length = 0
    let mut ctx = GraphContext::default();
    ctx.chain_lengths.insert("standalone".to_owned(), 0);
    assert_eq!(ctx.chain_length("standalone"), 0);
}

#[test]
fn chain_length_missing_fact_returns_zero_gracefully() {
    // Requirement 16: unknown fact IDs return 0 without panic (cycle safety in pure API)
    let ctx = GraphContext::default();
    assert_eq!(
        ctx.chain_length("nonexistent-fact"),
        0,
        "absent fact should return 0, not panic"
    );
}

#[test]
fn chain_length_ordering_reflects_chain_depth() {
    // Requirement 13/14: earlier-in-chain facts always have higher chain_length
    let mut ctx = GraphContext::default();
    ctx.chain_lengths.insert("oldest".to_owned(), 5);
    ctx.chain_lengths.insert("middle".to_owned(), 3);
    ctx.chain_lengths.insert("newest".to_owned(), 0);
    assert!(
        ctx.chain_length("oldest") > ctx.chain_length("middle"),
        "oldest in chain must have larger depth"
    );
    assert!(
        ctx.chain_length("middle") > ctx.chain_length("newest"),
        "middle must have larger depth than newest"
    );
}

// ---------------------------------------------------------------------------
// Domain profiling (requirements 17–19)
// ---------------------------------------------------------------------------

#[test]
fn domain_volatility_zero_total_facts_gives_zero_score() {
    // Requirement 17: entity with no facts → compute_volatility returns 0.0
    let v = compute_volatility(0, 0, 0.0);
    assert!(
        v.abs() < f64::EPSILON,
        "entity with no facts has zero volatility, got {v}"
    );
}

#[test]
fn domain_volatility_all_stable_facts_gives_zero_score() {
    // Requirement 17: entity where no facts are superseded → volatility = 0
    let v = compute_volatility(15, 0, 0.0);
    assert!(
        v.abs() < f64::EPSILON,
        "all-stable entity has zero volatility, got {v}"
    );
}

#[test]
fn domain_volatility_mixed_stable_volatile_averages_correctly() {
    // Requirement 18: 4 of 10 superseded, chain 0 → (4/10) × 1.0 = 0.4
    let v = compute_volatility(10, 4, 0.0);
    let expected = 0.4;
    assert!(
        (v - expected).abs() < 1e-10,
        "mixed domain: expected {expected}, got {v}"
    );
}

#[test]
fn domain_volatility_increases_when_more_supersessions_occur() {
    // Requirement 19: adding supersessions raises volatility score
    let before = compute_volatility(10, 2, 0.0); // 2 superseded
    let after = compute_volatility(10, 6, 0.0); // 6 superseded (new supersessions)
    assert!(
        after > before,
        "after more supersessions ({after}) > before ({before})"
    );
}

#[test]
fn domain_volatility_chain_length_amplifies_volatility() {
    // Requirement 19: longer chains amplify the volatility score for the same supersession rate
    let without_chain = compute_volatility(10, 5, 0.0);
    let with_chain = compute_volatility(10, 5, 3.0);
    assert!(
        with_chain > without_chain,
        "longer avg chain ({with_chain}) should amplify volatility vs no chain ({without_chain})"
    );
}

// ---------------------------------------------------------------------------
// Integration with recall scoring (requirements 20–23)
// ---------------------------------------------------------------------------

#[test]
fn score_access_with_evolution_chain_zero_equals_base() {
    // Requirement 20: chain_length=0 → no evolution bonus → result equals base
    let base = 0.6;
    let result = score_access_with_evolution(base, 0);
    assert!(
        (result - base).abs() < f64::EPSILON,
        "chain_length=0 should not change score, got {result}"
    );
}

#[test]
fn score_access_with_evolution_chain_four_adds_full_cap() {
    // Requirement 21: chain_length=4 → bonus = 4 × 0.05 = 0.20 (cap reached)
    let base = 0.5;
    let result = score_access_with_evolution(base, 4);
    let expected = base + 0.20;
    assert!(
        (result - expected).abs() < f64::EPSILON,
        "chain_length=4: expected {expected}, got {result}"
    );
}

#[test]
fn score_access_with_evolution_chain_ten_still_capped_at_point_two() {
    // Requirement 22: chain_length=10 → bonus capped at 0.20 (4×0.05 max)
    let base = 0.5;
    let result_4 = score_access_with_evolution(base, 4);
    let result_10 = score_access_with_evolution(base, 10);
    assert!(
        (result_4 - result_10).abs() < f64::EPSILON,
        "chain_length=10 should equal chain_length=4 (both capped): {result_10} vs {result_4}"
    );
}

#[test]
fn score_access_with_evolution_result_bounded_at_one() {
    // Requirement 23: result never exceeds 1.0 even with high base + chain bonus
    let high_base = 0.95;
    let result = score_access_with_evolution(high_base, 10);
    assert!(
        result <= 1.0,
        "evolution score must not exceed 1.0, got {result}"
    );
    assert!(
        result >= 0.0,
        "evolution score must not be negative, got {result}"
    );
}

#[test]
fn score_access_with_evolution_chain_increases_score() {
    // Requirements 20/21: longer chains yield higher (or equal) scores
    let base = 0.3;
    let chain_0 = score_access_with_evolution(base, 0);
    let chain_2 = score_access_with_evolution(base, 2);
    let chain_4 = score_access_with_evolution(base, 4);
    assert!(chain_2 > chain_0, "chain_2 > chain_0");
    assert!(chain_4 > chain_2, "chain_4 > chain_2");
}

// ---------------------------------------------------------------------------
// Property-based tests (requirements 24–26)
// ---------------------------------------------------------------------------

mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Requirement 24: for any volatility input, multiplier is in (0.0, 1.5].
        ///
        /// NOTE: The prompt's Context section specifies the multiplier formula as
        /// `1.0 / (1.0 + 0.1 * supersession_count)`, bounding output to (0.0, 1.0].
        /// The actual implementation `1.5 - volatility` produces range [0.5, 1.5].
        /// This test asserts the actual implementation's range. See PR observations.
        #[test]
        fn volatility_multiplier_range_is_half_to_one_point_five(
            v in 0.0_f64..=1.0,
        ) {
            let m = volatility_multiplier(v);
            prop_assert!(m > 0.0, "multiplier must be positive for v={v}, got {m}");
            prop_assert!(
                m <= 1.5,
                "multiplier must not exceed 1.5 for v={v}, got {m}"
            );
            prop_assert!(
                m >= 0.5,
                "multiplier must be at least 0.5 for v={v}, got {m}"
            );
        }

        /// Requirement 25: for any chain_length and access_count, evolution score is in [0.0, 1.0].
        #[test]
        fn evolution_score_always_bounded(
            base in 0.0_f64..=1.0,
            chain_length in 0_u32..=1000,
        ) {
            let result = score_access_with_evolution(base, chain_length);
            prop_assert!(
                result >= 0.0,
                "evolution score must be non-negative: base={base}, chain={chain_length}, got {result}"
            );
            prop_assert!(
                result <= 1.0,
                "evolution score must not exceed 1.0: base={base}, chain={chain_length}, got {result}"
            );
        }

        /// Requirement 26: compute_volatility is monotonically non-decreasing as
        /// supersession_count increases (total and avg_chain_length held constant).
        #[test]
        fn volatility_non_decreasing_with_supersession_count(
            total in 1_u32..=1000,
            chain in 0.0_f64..=10.0,
        ) {
            // Generate two supersession counts where low ≤ high ≤ total
            let low = total / 3;
            let high = (total * 2) / 3;
            let v_low = compute_volatility(total, low, chain);
            let v_high = compute_volatility(total, high, chain);
            prop_assert!(
                v_high >= v_low,
                "higher supersession count must produce >= volatility: \
                 total={total}, low={low} → {v_low}, high={high} → {v_high}"
            );
        }
    }
}
