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

    #[test]
    fn weights_total_matches_sum(
        vs in 0.0_f64..=1.0,
        dec in 0.0_f64..=1.0,
        rel in 0.0_f64..=1.0,
        epi in 0.0_f64..=1.0,
        prox in 0.0_f64..=1.0,
        freq in 0.0_f64..=1.0,
    ) {
        let w = RecallWeights {
            vector_similarity: vs,
            decay: dec,
            relevance: rel,
            epistemic_tier: epi,
            relationship_proximity: prox,
            access_frequency: freq,
        };
        let expected = vs + dec + rel + epi + prox + freq;
        prop_assert!(
            (w.total() - expected).abs() < 1e-10,
            "total() {} != sum {} for weights {:?}",
            w.total(),
            expected,
            w,
        );
    }
}
