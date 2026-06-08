//! Property-based tests for recall scoring.
use proptest::prelude::*;

use super::super::*;
proptest! {
    #[test]
    fn recall_scores_always_bounded(
        vec_sim in 0.0_f64..=1.0,
        decay in 0.0_f64..=1.0,
        relevance in 0.0_f64..=1.0,
        tier in 0.0_f64..=1.0,
        proximity in 0.0_f64..=1.0,
        freq in 0.0_f64..=1.0,
    ) {
        let e = RecallEngine::new();
        let factors = FactorScores {
            vector_similarity: vec_sim,
            decay,
            relevance,
            epistemic_tier: tier,
            relationship_proximity: proximity,
            access_frequency: freq,
            graph_importance: 0.0,
            serendipity: 0.0,
            ..FactorScores::default()
        };
        let score = e.compute_score(&factors);
        prop_assert!(
            (0.0..=1.0).contains(&score),
            "score {score} out of [0.0, 1.0] for factors {factors:?}"
        );
    }

    #[test]
    fn individual_scorers_bounded(
        cosine_dist in 0.0_f64..=2.0,
        age_hours in 0.0_f64..=100_000.0,
        access_count in 0_u64..=10_000,
        hops in proptest::option::of(0_u32..=50),
    ) {
        let e = RecallEngine::new();

        let vs = e.score_vector_similarity(cosine_dist);
        prop_assert!((0.0..=1.0).contains(&vs), "vector_similarity {vs} out of bounds");

        let ds = e.score_decay(
            age_hours,
            FactType::Event,
            EpistemicTier::Inferred,
            0,
        );
        prop_assert!((0.0..=1.0).contains(&ds), "decay {ds} out of bounds");

        let af = e.score_access_frequency(access_count);
        prop_assert!(af >= 0.0, "access_frequency {af} below 0");
        prop_assert!(af.is_finite(), "access_frequency {af} not finite");

        let rp = e.score_relationship_proximity(hops);
        prop_assert!((0.0..=1.0).contains(&rp), "relationship_proximity {rp} out of bounds");
    }

    /// WHY (#3392): For any finite `age_hours`, including negative values
    /// (clock jumped backward) or zero, the decay must remain in [0, 1].
    /// Previously, negative `age_hours` short-circuited to 1.0 silently;
    /// now it is clamped to 0 and still returns 1.0, but the property
    /// holds for the whole finite domain (including pre-clamp inputs).
    #[test]
    fn decay_bounded_under_clock_jump(
        age_hours in any::<f64>().prop_filter("finite", |x| x.is_finite()),
        access_count in 0_u32..=10_000,
    ) {
        let e = RecallEngine::new();
        let ds = e.score_decay(
            age_hours,
            FactType::Event,
            EpistemicTier::Inferred,
            access_count,
        );
        prop_assert!(
            (0.0..=1.0).contains(&ds),
            "decay {ds} out of [0.0, 1.0] for age_hours={age_hours}"
        );
    }

    #[test]
    fn weights_total_matches_sum(
        vs in 0.0_f64..=1.0,
        dec in 0.0_f64..=1.0,
        rel in 0.0_f64..=1.0,
        epi in 0.0_f64..=1.0,
        prox in 0.0_f64..=1.0,
        freq in 0.0_f64..=1.0,
        g_imp in 0.0_f64..=1.0,
    ) {
        let w = RecallWeights {
            vector_similarity: vs,
            decay: dec,
            relevance: rel,
            epistemic_tier: epi,
            relationship_proximity: prox,
            access_frequency: freq,
            graph_importance: g_imp,
            serendipity: 0.0,
            ..RecallWeights::default()
        };
        // NOTE: surprise, evidence_coverage, and serendipity default to 0.0 so the total
        // matches the explicit seven fields.
        let expected = vs + dec + rel + epi + prox + freq + g_imp;
        prop_assert!(
            (w.total() - expected).abs() < 1e-10,
            "total() {} != sum {} for weights {:?}",
            w.total(),
            expected,
            w,
        );
    }
}
