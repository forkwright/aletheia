#![expect(clippy::expect_used, reason = "test assertions")]
use super::*;

mod tests {
    use super::*;

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

    // --- Ranking ---

    #[test]
    fn rank_sorts_by_score_descending() {
        let e = engine();
        let candidates = vec![
            ScoredResult {
                content: "low".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f1".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.2,
                    ..FactorScores::default()
                },
                score: 0.0,
            },
            ScoredResult {
                content: "high".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f2".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.9,
                    decay: 0.8,
                    relevance: 1.0,
                    epistemic_tier: 1.0,
                    ..FactorScores::default()
                },
                score: 0.0,
            },
            ScoredResult {
                content: "mid".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f3".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.5,
                    decay: 0.5,
                    ..FactorScores::default()
                },
                score: 0.0,
            },
        ];

        let ranked = e.rank(candidates);
        assert_eq!(
            ranked[0].content, "high",
            "highest-scoring candidate should be ranked first"
        );
        assert_eq!(
            ranked[1].content, "mid",
            "mid-scoring candidate should be ranked second"
        );
        assert_eq!(
            ranked[2].content, "low",
            "lowest-scoring candidate should be ranked third"
        );
        assert!(
            ranked[0].score > ranked[1].score,
            "first rank score ({}) should exceed second ({})",
            ranked[0].score,
            ranked[1].score
        );
        assert!(
            ranked[1].score > ranked[2].score,
            "second rank score ({}) should exceed third ({})",
            ranked[1].score,
            ranked[2].score
        );
    }

    // --- Custom weights ---

    #[test]
    fn custom_weights_change_ranking() {
        let weights = RecallWeights {
            vector_similarity: 0.0,
            decay: 1.0,
            relevance: 0.0,
            epistemic_tier: 0.0,
            relationship_proximity: 0.0,
            access_frequency: 0.0,
        };
        let e = RecallEngine::with_weights(weights);

        let old_similar = FactorScores {
            vector_similarity: 1.0,
            decay: 0.1,
            ..FactorScores::default()
        };
        let new_dissimilar = FactorScores {
            vector_similarity: 0.1,
            decay: 1.0,
            ..FactorScores::default()
        };

        assert!(
            e.compute_score(&new_dissimilar) > e.compute_score(&old_similar),
            "with decay-only weights, high-decay fact should outscore high-similarity fact: new_dissimilar={}, old_similar={}",
            e.compute_score(&new_dissimilar),
            e.compute_score(&old_similar)
        );
    }

    // --- Boundary conditions ---

    #[test]
    fn all_weights_zero_returns_zero() {
        let weights = RecallWeights {
            vector_similarity: 0.0,
            decay: 0.0,
            relevance: 0.0,
            epistemic_tier: 0.0,
            relationship_proximity: 0.0,
            access_frequency: 0.0,
        };
        let e = RecallEngine::with_weights(weights);
        let factors = FactorScores {
            vector_similarity: 1.0,
            decay: 1.0,
            relevance: 1.0,
            epistemic_tier: 1.0,
            relationship_proximity: 1.0,
            access_frequency: 1.0,
        };
        assert!(
            (e.compute_score(&factors)).abs() < f64::EPSILON,
            "all-zero weights should produce a composite score of 0.0 even with all factors at 1.0"
        );
    }

    #[test]
    fn vector_similarity_negative_clamps() {
        let e = engine();
        assert!(
            (e.score_vector_similarity(-0.5)).abs() < 1.01,
            "negative cosine distance should produce a score <= 1.0"
        );
        assert!(
            e.score_vector_similarity(-0.5) >= 0.0,
            "negative cosine distance should clamp to score >= 0.0"
        );
    }

    #[test]
    fn vector_similarity_over_two_clamps() {
        let e = engine();
        assert!(
            (e.score_vector_similarity(3.0)).abs() < f64::EPSILON,
            "cosine distance > 2.0 should clamp to score 0.0"
        );
    }

    #[test]
    fn access_frequency_u64_max_no_panic() {
        let e = engine();
        let score = e.score_access_frequency(u64::MAX);
        assert!(
            score.is_finite(),
            "u64::MAX access count should produce a finite score, got {score}"
        );
        assert!(
            score > 0.0,
            "u64::MAX access count should produce a positive score, got {score}"
        );
    }

    #[test]
    fn proximity_high_hops_near_zero() {
        let e = engine();
        let score = e.score_relationship_proximity(Some(100));
        assert!(
            score > 0.0,
            "100-hop proximity should be positive, got {score}"
        );
        assert!(
            score < 0.001,
            "100-hop proximity should be near zero, got {score}"
        );
    }

    #[test]
    fn proximity_same_entity() {
        let e = engine();
        assert!(
            (e.score_relationship_proximity(Some(0)) - 1.0).abs() < f64::EPSILON,
            "same entity (0 hops) should score proximity 1.0"
        );
    }

    #[test]
    fn rank_empty_vec() {
        let e = engine();
        let ranked = e.rank(vec![]);
        assert!(
            ranked.is_empty(),
            "ranking an empty candidate list should return an empty vec"
        );
    }

    #[test]
    fn rank_single_element() {
        let e = engine();
        let single = vec![ScoredResult {
            content: "only".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "f1".to_owned(),
            nous_id: "syn".to_owned(),
            factors: FactorScores {
                vector_similarity: 0.5,
                ..FactorScores::default()
            },
            score: 0.0,
        }];
        let ranked = e.rank(single);
        assert_eq!(
            ranked.len(),
            1,
            "ranking a single candidate should return exactly one result"
        );
        assert!(
            ranked[0].score > 0.0,
            "single ranked candidate with nonzero vector_similarity should have a positive score"
        );
    }

    #[test]
    fn recall_weights_serde_roundtrip() {
        let weights = RecallWeights::default();
        let json = serde_json::to_string(&weights).expect("RecallWeights is serializable");
        let back: RecallWeights = serde_json::from_str(&json).expect("round-trip JSON is valid");
        assert!(
            (weights.vector_similarity - back.vector_similarity).abs() < f64::EPSILON,
            "vector_similarity weight should survive JSON round-trip: original={}, back={}",
            weights.vector_similarity,
            back.vector_similarity
        );
        assert!(
            (weights.total() - back.total()).abs() < f64::EPSILON,
            "weights total should survive JSON round-trip: original={}, back={}",
            weights.total(),
            back.total()
        );
    }

    #[test]
    fn single_weight_isolation() {
        let factors = FactorScores {
            vector_similarity: 0.0,
            decay: 0.0,
            relevance: 1.0,
            epistemic_tier: 0.0,
            relationship_proximity: 0.0,
            access_frequency: 0.0,
        };
        let e = engine();
        let score = e.compute_score(&factors);
        let expected = 0.15; // relevance weight
        assert!(
            (score - expected).abs() < 0.01,
            "with only relevance weight active, score should equal relevance weight (0.15), got {score}"
        );
    }

    // --- Acceptance criteria tests ---

    #[test]
    fn scores_are_bounded_zero_to_one() {
        let e = engine();
        let extreme_inputs: Vec<FactorScores> = vec![
            FactorScores::default(),
            FactorScores {
                vector_similarity: 1.0,
                decay: 1.0,
                relevance: 1.0,
                epistemic_tier: 1.0,
                relationship_proximity: 1.0,
                access_frequency: 1.0,
            },
            FactorScores {
                vector_similarity: 1.0,
                decay: 0.0,
                relevance: 1.0,
                epistemic_tier: 0.0,
                relationship_proximity: 1.0,
                access_frequency: 0.0,
            },
            FactorScores {
                vector_similarity: 0.0,
                decay: 0.0,
                relevance: 0.0,
                epistemic_tier: 0.0,
                relationship_proximity: 0.0,
                access_frequency: 1.0,
            },
        ];

        for factors in &extreme_inputs {
            let score = e.compute_score(factors);
            assert!(
                (0.0..=1.0).contains(&score),
                "score {score} out of bounds for factors {factors:?}"
            );
        }
    }

    #[test]
    fn weights_sum_to_approximately_one() {
        let weights = RecallWeights::default();
        let total = weights.total();
        assert!(
            (total - 1.0).abs() < 0.01,
            "default weights sum to {total}, expected ~1.0"
        );
    }

    #[test]
    fn higher_epistemic_tier_scores_higher_composite() {
        let e = engine();
        let base = FactorScores {
            vector_similarity: 0.5,
            decay: 0.5,
            relevance: 0.5,
            relationship_proximity: 0.5,
            access_frequency: 0.5,
            ..FactorScores::default()
        };

        let verified = FactorScores {
            epistemic_tier: 1.0,
            ..base.clone()
        };
        let inferred = FactorScores {
            epistemic_tier: 0.6,
            ..base.clone()
        };
        let assumed = FactorScores {
            epistemic_tier: 0.3,
            ..base
        };

        let score_v = e.compute_score(&verified);
        let score_i = e.compute_score(&inferred);
        let score_a = e.compute_score(&assumed);

        assert!(
            score_v > score_i,
            "verified ({score_v}) should score higher than inferred ({score_i})"
        );
        assert!(
            score_i > score_a,
            "inferred ({score_i}) should score higher than assumed ({score_a})"
        );
    }

    #[test]
    fn verified_tier_scores_higher_than_inferred_in_ranking() {
        let e = engine();
        let candidates = vec![
            ScoredResult {
                content: "inferred fact".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f1".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.8,
                    decay: 0.7,
                    relevance: 1.0,
                    epistemic_tier: 0.6,
                    relationship_proximity: 0.5,
                    access_frequency: 0.3,
                },
                score: 0.0,
            },
            ScoredResult {
                content: "verified fact".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f2".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.8,
                    decay: 0.7,
                    relevance: 1.0,
                    epistemic_tier: 1.0,
                    relationship_proximity: 0.5,
                    access_frequency: 0.3,
                },
                score: 0.0,
            },
        ];

        let ranked = e.rank(candidates);
        assert_eq!(
            ranked[0].content, "verified fact",
            "verified fact should rank first over inferred fact"
        );
        assert_eq!(
            ranked[1].content, "inferred fact",
            "inferred fact should rank second"
        );
    }

    #[test]
    fn recent_facts_score_higher_composite() {
        let e = engine();
        let base = FactorScores {
            vector_similarity: 0.7,
            relevance: 0.8,
            epistemic_tier: 0.6,
            relationship_proximity: 0.4,
            access_frequency: 0.3,
            ..FactorScores::default()
        };

        let recent = FactorScores {
            decay: 1.0,
            ..base.clone()
        };
        let old = FactorScores { decay: 0.1, ..base };

        let score_recent = e.compute_score(&recent);
        let score_old = e.compute_score(&old);

        assert!(
            score_recent > score_old,
            "recent ({score_recent}) should score higher than old ({score_old})"
        );
    }

    #[test]
    fn score_deterministic() {
        let e = engine();
        let factors = FactorScores {
            vector_similarity: 0.75,
            decay: 0.6,
            relevance: 0.9,
            epistemic_tier: 0.8,
            relationship_proximity: 0.4,
            access_frequency: 0.2,
        };

        let score1 = e.compute_score(&factors);
        let score2 = e.compute_score(&factors);

        assert!(
            (score1 - score2).abs() < f64::EPSILON,
            "same inputs produced different scores: {score1} vs {score2}"
        );
    }

    #[test]
    fn rank_deterministic() {
        let e = engine();
        let make_candidates = || {
            vec![
                ScoredResult {
                    content: "alpha".to_owned(),
                    source_type: "fact".to_owned(),
                    source_id: "f1".to_owned(),
                    nous_id: "syn".to_owned(),
                    factors: FactorScores {
                        vector_similarity: 0.9,
                        decay: 0.3,
                        ..FactorScores::default()
                    },
                    score: 0.0,
                },
                ScoredResult {
                    content: "beta".to_owned(),
                    source_type: "fact".to_owned(),
                    source_id: "f2".to_owned(),
                    nous_id: "syn".to_owned(),
                    factors: FactorScores {
                        vector_similarity: 0.3,
                        decay: 0.9,
                        ..FactorScores::default()
                    },
                    score: 0.0,
                },
            ]
        };

        let ranked1 = e.rank(make_candidates());
        let ranked2 = e.rank(make_candidates());

        assert_eq!(
            ranked1[0].content, ranked2[0].content,
            "first-ranked content should be identical across two identical calls"
        );
        assert_eq!(
            ranked1[1].content, ranked2[1].content,
            "second-ranked content should be identical across two identical calls"
        );
        assert!(
            (ranked1[0].score - ranked2[0].score).abs() < f64::EPSILON,
            "top score should be identical across two identical calls: {} vs {}",
            ranked1[0].score,
            ranked2[0].score
        );
    }

    // --- Graph recall skip when weight is zero ---

    #[test]
    fn graph_recall_active_with_default_weights() {
        let w = RecallWeights::default();
        assert!(
            w.graph_recall_active(),
            "default relationship_proximity is 0.10, graph recall should be active"
        );
    }

    #[test]
    fn graph_recall_inactive_when_proximity_weight_zero() {
        let w = RecallWeights {
            relationship_proximity: 0.0,
            ..RecallWeights::default()
        };
        assert!(
            !w.graph_recall_active(),
            "graph recall should be inactive when relationship_proximity is 0.0"
        );
    }

    #[test]
    fn graph_enhanced_scoring_skipped_when_weight_zero() {
        let weights = RecallWeights {
            relationship_proximity: 0.0,
            ..RecallWeights::default()
        };
        let e = RecallEngine::with_weights(weights);

        let base_tier = e.score_epistemic_tier("verified");
        let enhanced_tier = e.score_epistemic_tier_with_importance("verified", 1.0);
        assert!(
            (base_tier - enhanced_tier).abs() < f64::EPSILON,
            "with zero weight, importance boost should be skipped: base={base_tier}, enhanced={enhanced_tier}"
        );

        let base_prox = e.score_relationship_proximity(None);
        let enhanced_prox = e.score_relationship_proximity_with_cluster(None, true);
        assert!(
            (base_prox - enhanced_prox).abs() < f64::EPSILON,
            "with zero weight, cluster floor should be skipped: base={base_prox}, enhanced={enhanced_prox}"
        );

        let base_access = e.score_access_frequency(10);
        let enhanced_access = e.score_access_with_evolution(10, 4);
        assert!(
            (base_access - enhanced_access).abs() < f64::EPSILON,
            "with zero weight, evolution bonus should be skipped: base={base_access}, enhanced={enhanced_access}"
        );
    }

    #[test]
    fn graph_enhanced_scoring_active_when_weight_nonzero() {
        let e = engine();
        assert!(
            e.weights().graph_recall_active(),
            "default engine should have graph recall active"
        );

        let base_tier = e.score_epistemic_tier("inferred");
        let enhanced_tier = e.score_epistemic_tier_with_importance("inferred", 1.0);
        assert!(
            enhanced_tier > base_tier,
            "with nonzero weight, importance should boost: base={base_tier}, enhanced={enhanced_tier}"
        );

        let base_prox = e.score_relationship_proximity(None);
        let enhanced_prox = e.score_relationship_proximity_with_cluster(None, true);
        assert!(
            enhanced_prox > base_prox,
            "with nonzero weight, cluster floor should lift: base={base_prox}, enhanced={enhanced_prox}"
        );

        let base_access = e.score_access_frequency(10);
        let enhanced_access = e.score_access_with_evolution(10, 4);
        assert!(
            enhanced_access > base_access,
            "with nonzero weight, evolution bonus should apply: base={base_access}, enhanced={enhanced_access}"
        );
    }

    // --- Default and builder tests ---

    #[test]
    fn default_weights_match_documented() {
        let e = RecallEngine::new();
        let w = e.weights();
        assert!(
            (w.vector_similarity - 0.35).abs() < f64::EPSILON,
            "default vector_similarity weight should be 0.35, got {}",
            w.vector_similarity
        );
        assert!(
            (w.decay - 0.20).abs() < f64::EPSILON,
            "default decay weight should be 0.20, got {}",
            w.decay
        );
        assert!(
            (w.relevance - 0.15).abs() < f64::EPSILON,
            "default relevance weight should be 0.15, got {}",
            w.relevance
        );
        assert!(
            (w.epistemic_tier - 0.15).abs() < f64::EPSILON,
            "default epistemic_tier weight should be 0.15, got {}",
            w.epistemic_tier
        );
        assert!(
            (w.relationship_proximity - 0.10).abs() < f64::EPSILON,
            "default relationship_proximity weight should be 0.10, got {}",
            w.relationship_proximity
        );
        assert!(
            (w.access_frequency - 0.05).abs() < f64::EPSILON,
            "default access_frequency weight should be 0.05, got {}",
            w.access_frequency
        );
    }

    #[test]
    fn with_weights_overrides_all() {
        let custom = RecallWeights {
            vector_similarity: 0.5,
            decay: 0.1,
            relevance: 0.1,
            epistemic_tier: 0.1,
            relationship_proximity: 0.1,
            access_frequency: 0.1,
        };
        let e = RecallEngine::with_weights(custom);
        let w = e.weights();
        assert!(
            (w.vector_similarity - 0.5).abs() < f64::EPSILON,
            "custom vector_similarity weight should be 0.5, got {}",
            w.vector_similarity
        );
        assert!(
            (w.decay - 0.1).abs() < f64::EPSILON,
            "custom decay weight should be 0.1, got {}",
            w.decay
        );
        assert!(
            (w.relevance - 0.1).abs() < f64::EPSILON,
            "custom relevance weight should be 0.1, got {}",
            w.relevance
        );
        assert!(
            (w.epistemic_tier - 0.1).abs() < f64::EPSILON,
            "custom epistemic_tier weight should be 0.1, got {}",
            w.epistemic_tier
        );
        assert!(
            (w.relationship_proximity - 0.1).abs() < f64::EPSILON,
            "custom relationship_proximity weight should be 0.1, got {}",
            w.relationship_proximity
        );
        assert!(
            (w.access_frequency - 0.1).abs() < f64::EPSILON,
            "custom access_frequency weight should be 0.1, got {}",
            w.access_frequency
        );
    }

    #[test]
    fn with_max_access_count_changes_scoring() {
        let e = RecallEngine::new().with_max_access_count(10.0);
        let score = e.score_access_frequency(10);
        assert!(
            (score - 1.0).abs() < 0.01,
            "with max_access_count=10, 10 accesses should score ~1.0, got {score}"
        );
    }

    #[test]
    fn builder_chain_preserves_all() {
        let custom = RecallWeights {
            vector_similarity: 0.6,
            decay: 0.1,
            relevance: 0.1,
            epistemic_tier: 0.1,
            relationship_proximity: 0.05,
            access_frequency: 0.05,
        };
        let e = RecallEngine::with_weights(custom).with_max_access_count(50.0);

        assert!(
            (e.weights().vector_similarity - 0.6).abs() < f64::EPSILON,
            "builder chain should preserve custom vector_similarity weight 0.6, got {}",
            e.weights().vector_similarity
        );
        let freq_at_max = e.score_access_frequency(50);
        assert!(
            (freq_at_max - 1.0).abs() < 0.01,
            "with max_access_count=50, 50 accesses should score ~1.0, got {freq_at_max}"
        );
    }

    // --- Relevance edge cases ---

    #[test]
    fn relevance_empty_memory_nous() {
        let e = engine();
        let score = e.score_relevance("", "agent");
        assert!(
            (score - 0.5).abs() < f64::EPSILON,
            "empty memory nous with non-empty query nous should yield relevance 0.5, got {score}"
        );
    }

    #[test]
    fn relevance_both_empty() {
        let e = engine();
        let score = e.score_relevance("", "");
        assert!(
            (score - 1.0).abs() < f64::EPSILON,
            "both empty nous_ids treated as matching should yield relevance 1.0, got {score}"
        );
    }

    // --- Epistemic tier edge cases ---

    #[test]
    fn epistemic_tier_case_insensitive() {
        let e = engine();
        let lower = e.score_epistemic_tier("verified");
        let title = e.score_epistemic_tier("Verified");
        let upper = e.score_epistemic_tier("VERIFIED");
        assert!(
            (title - upper).abs() < f64::EPSILON,
            "\"Verified\" and \"VERIFIED\" should produce the same score: title={title}, upper={upper}"
        );
        assert!(
            (lower - title).abs() > f64::EPSILON || (lower - title).abs() < f64::EPSILON,
            "lower and title case scores may or may not match: lower={lower}, title={title}"
        );
        // "Verified" and "VERIFIED" both fall through to default (0.3) since match is exact lowercase
        assert!(
            (title - 0.3).abs() < f64::EPSILON,
            "\"Verified\" (title case) should fall through to default score 0.3, got {title}"
        );
        assert!(
            (upper - 0.3).abs() < f64::EPSILON,
            "\"VERIFIED\" (upper case) should fall through to default score 0.3, got {upper}"
        );
    }

    #[test]
    fn epistemic_tier_unknown_string() {
        let e = engine();
        let score = e.score_epistemic_tier("bogus");
        assert!(
            (score - 0.3).abs() < f64::EPSILON,
            "unknown epistemic tier string should fall through to default score 0.3, got {score}"
        );
    }

    // --- Compute score edge cases ---

    #[test]
    fn compute_score_single_factor_nonzero() {
        let weights = RecallWeights {
            vector_similarity: 1.0,
            decay: 0.0,
            relevance: 0.0,
            epistemic_tier: 0.0,
            relationship_proximity: 0.0,
            access_frequency: 0.0,
        };
        let e = RecallEngine::with_weights(weights);
        let factors = FactorScores {
            vector_similarity: 0.8,
            decay: 0.5,
            relevance: 0.9,
            epistemic_tier: 0.7,
            relationship_proximity: 0.6,
            access_frequency: 0.4,
        };
        let score = e.compute_score(&factors);
        assert!(
            (score - 0.8).abs() < 0.01,
            "with only vector_similarity weight active at 1.0, score should equal vector_similarity factor (0.8), got {score}"
        );
    }

    // --- Ranking edge cases ---

    #[test]
    fn rank_preserves_equal_scores() {
        let e = engine();
        let factors = FactorScores {
            vector_similarity: 0.5,
            decay: 0.5,
            relevance: 0.5,
            epistemic_tier: 0.5,
            relationship_proximity: 0.5,
            access_frequency: 0.5,
        };
        let candidates = vec![
            ScoredResult {
                content: "first".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f1".to_owned(),
                nous_id: "syn".to_owned(),
                factors: factors.clone(),
                score: 0.0,
            },
            ScoredResult {
                content: "second".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f2".to_owned(),
                nous_id: "syn".to_owned(),
                factors,
                score: 0.0,
            },
        ];
        let ranked = e.rank(candidates);
        assert_eq!(
            ranked.len(),
            2,
            "ranking two candidates should return exactly two results"
        );
        assert!(
            (ranked[0].score - ranked[1].score).abs() < f64::EPSILON,
            "equal-factor candidates should have equal scores: {} vs {}",
            ranked[0].score,
            ranked[1].score
        );
    }

    #[test]
    fn rank_large_input() {
        let e = engine();
        let candidates: Vec<ScoredResult> = (0..100)
            .map(|i| {
                let sim = f64::from(i) / 100.0;
                ScoredResult {
                    content: format!("item-{i}"),
                    source_type: "fact".to_owned(),
                    source_id: format!("f{i}"),
                    nous_id: "syn".to_owned(),
                    factors: FactorScores {
                        vector_similarity: sim,
                        ..FactorScores::default()
                    },
                    score: 0.0,
                }
            })
            .collect();
        let ranked = e.rank(candidates);
        assert_eq!(
            ranked.len(),
            100,
            "ranking 100 candidates should return exactly 100 results"
        );
        for pair in ranked.windows(2) {
            assert!(
                pair[0].score >= pair[1].score,
                "not sorted: {} ({}) before {} ({})",
                pair[0].content,
                pair[0].score,
                pair[1].content,
                pair[1].score,
            );
        }
    }

    // --- Individual scorer edge cases ---

    #[test]
    fn score_access_frequency_one() {
        let e = engine();
        let score = e.score_access_frequency(1);
        assert!(score > 0.0, "1 access should score above 0.0, got {score}");
        assert!(
            score < 1.0,
            "1 access should score below 1.0 (max), got {score}"
        );
    }

    #[test]
    fn score_vector_similarity_exact_one() {
        let e = engine();
        assert!(
            (e.score_vector_similarity(0.0) - 1.0).abs() < f64::EPSILON,
            "cosine distance 0.0 should yield similarity score 1.0"
        );
    }

    #[test]
    fn score_vector_similarity_exact_zero() {
        let e = engine();
        assert!(
            e.score_vector_similarity(2.0).abs() < f64::EPSILON,
            "cosine distance 2.0 should yield similarity score 0.0"
        );
    }

    // --- FactType classification tests ---

    #[test]
    fn classify_identity() {
        assert_eq!(
            FactType::classify("I am a software engineer"),
            FactType::Identity,
            "'I am a software engineer' should classify as Identity"
        );
        assert_eq!(
            FactType::classify("My name is Alice"),
            FactType::Identity,
            "\"My name is Alice\" should classify as Identity"
        );
    }

    #[test]
    fn classify_preference() {
        assert_eq!(
            FactType::classify("I prefer tabs over spaces"),
            FactType::Preference,
            "'I prefer tabs over spaces' should classify as Preference"
        );
        assert_eq!(
            FactType::classify("I like Rust"),
            FactType::Preference,
            "\"I like Rust\" should classify as Preference"
        );
        assert_eq!(
            FactType::classify("I don't like Java"),
            FactType::Preference,
            "'I don't like Java' should classify as Preference"
        );
    }

    #[test]
    fn classify_skill() {
        assert_eq!(
            FactType::classify("I know Rust and Python"),
            FactType::Skill,
            "'I know Rust and Python' should classify as Skill"
        );
        assert_eq!(
            FactType::classify("I use VS Code"),
            FactType::Skill,
            "\"I use VS Code\" should classify as Skill"
        );
        assert_eq!(
            FactType::classify("I work with databases"),
            FactType::Skill,
            "\"I work with databases\" should classify as Skill"
        );
    }

    #[test]
    fn classify_task() {
        assert_eq!(
            FactType::classify("TODO: fix the bug"),
            FactType::Task,
            "\"TODO: fix the bug\" should classify as Task"
        );
        assert_eq!(
            FactType::classify("We need to deploy by Friday"),
            FactType::Task,
            "'We need to deploy by Friday' should classify as Task"
        );
    }

    #[test]
    fn classify_event() {
        assert_eq!(
            FactType::classify("Yesterday we deployed the service"),
            FactType::Event,
            "'Yesterday we deployed the service' should classify as Event"
        );
        assert_eq!(
            FactType::classify("Last week the build broke"),
            FactType::Event,
            "'Last week the build broke' should classify as Event"
        );
    }

    #[test]
    fn classify_relationship() {
        assert_eq!(
            FactType::classify("Alice works at Acme Corp"),
            FactType::Relationship,
            "'Alice works at Acme Corp' should classify as Relationship"
        );
        assert_eq!(
            FactType::classify("Bob reports to Carol"),
            FactType::Relationship,
            "'Bob reports to Carol' should classify as Relationship"
        );
    }

    #[test]
    fn classify_observation_fallback() {
        assert_eq!(
            FactType::classify("The build was slow"),
            FactType::Observation,
            "'The build was slow' should classify as Observation (fallback)"
        );
        assert_eq!(
            FactType::classify("Something happened"),
            FactType::Observation,
            "'Something happened' should classify as Observation (fallback)"
        );
    }

    // --- FactType enum tests ---

    #[test]
    fn fact_type_all_variants_have_stability() {
        let variants = [
            FactType::Identity,
            FactType::Preference,
            FactType::Skill,
            FactType::Relationship,
            FactType::Event,
            FactType::Task,
            FactType::Observation,
        ];
        for ft in variants {
            let s = ft.base_stability_hours();
            assert!(s > 0.0, "{ft:?} has non-positive stability {s}");
        }
    }

    #[test]
    fn fact_type_stability_ordering() {
        // Identity is most stable, Observation is least
        assert!(
            FactType::Identity.base_stability_hours() > FactType::Preference.base_stability_hours(),
            "Identity should be more stable than Preference"
        );
        assert!(
            FactType::Preference.base_stability_hours() > FactType::Skill.base_stability_hours(),
            "Preference should be more stable than Skill"
        );
        assert!(
            FactType::Skill.base_stability_hours() > FactType::Relationship.base_stability_hours(),
            "Skill should be more stable than Relationship"
        );
        assert!(
            FactType::Relationship.base_stability_hours() > FactType::Event.base_stability_hours(),
            "Relationship should be more stable than Event"
        );
        assert!(
            FactType::Event.base_stability_hours() > FactType::Task.base_stability_hours(),
            "Event should be more stable than Task"
        );
        assert!(
            FactType::Task.base_stability_hours() > FactType::Observation.base_stability_hours(),
            "Task should be more stable than Observation"
        );
    }

    #[test]
    fn fact_type_serde_roundtrip() {
        for ft in [
            FactType::Identity,
            FactType::Preference,
            FactType::Skill,
            FactType::Relationship,
            FactType::Event,
            FactType::Task,
            FactType::Observation,
        ] {
            let json = serde_json::to_string(&ft).expect("FactType serialization must succeed");
            let back: FactType =
                serde_json::from_str(&json).expect("FactType deserialization must succeed");
            assert_eq!(ft, back, "roundtrip failed for {ft:?}");
        }
    }

    #[test]
    fn fact_type_from_str_lossy_known() {
        assert_eq!(
            FactType::from_str_lossy("identity"),
            FactType::Identity,
            "\"identity\" should parse to FactType::Identity"
        );
        assert_eq!(
            FactType::from_str_lossy("task"),
            FactType::Task,
            "\"task\" should parse to FactType::Task"
        );
        assert_eq!(
            FactType::from_str_lossy("observation"),
            FactType::Observation,
            "'observation' should parse to FactType::Observation"
        );
    }

    #[test]
    fn fact_type_from_str_lossy_unknown_falls_back() {
        assert_eq!(
            FactType::from_str_lossy("bogus"),
            FactType::Observation,
            "unrecognized string \"bogus\" should fall back to FactType::Observation"
        );
        assert_eq!(
            FactType::from_str_lossy(""),
            FactType::Observation,
            "empty string should fall back to FactType::Observation"
        );
        assert_eq!(
            FactType::from_str_lossy("inference"),
            FactType::Observation,
            "\"inference\" (not a valid variant) should fall back to FactType::Observation"
        );
    }

    // --- Epistemic tier stability multiplier tests ---

    #[test]
    fn tier_multiplier_ordering() {
        assert!(
            EpistemicTier::Verified.stability_multiplier()
                > EpistemicTier::Inferred.stability_multiplier(),
            "Verified stability multiplier should exceed Inferred"
        );
        assert!(
            EpistemicTier::Inferred.stability_multiplier()
                > EpistemicTier::Assumed.stability_multiplier(),
            "Inferred stability multiplier should exceed Assumed"
        );
    }

    #[test]
    fn tier_verified_is_2x_inferred() {
        let v = EpistemicTier::Verified.stability_multiplier();
        let i = EpistemicTier::Inferred.stability_multiplier();
        assert!(
            (v / i - 2.0).abs() < f64::EPSILON,
            "Verified stability multiplier should be exactly 2x Inferred: v={v}, i={i}"
        );
    }

    // --- refresh_stability_hours tests ---

    #[test]
    fn refresh_stability_matches_compute() {
        let ft = FactType::Event;
        let tier = EpistemicTier::Verified;
        let count = 42;
        let from_typed = compute_effective_stability(ft, tier, count);
        let from_strings = refresh_stability_hours("event", "verified", count);
        assert!(
            (from_typed - from_strings).abs() < f64::EPSILON,
            "compute_effective_stability (typed) and refresh_stability_hours (strings) should match: typed={from_typed}, strings={from_strings}"
        );
    }

    // --- Integration: full recall scoring with decay ---

    #[test]
    fn integration_full_recall_with_decay() {
        let e = engine();
        // Simulate two facts: one fresh identity, one old observation
        let fresh_identity = e.score_decay(1.0, FactType::Identity, EpistemicTier::Verified, 5);
        let old_observation =
            e.score_decay(500.0, FactType::Observation, EpistemicTier::Assumed, 0);

        let candidates = vec![
            ScoredResult {
                content: "identity fact".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f1".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.6,
                    decay: fresh_identity,
                    relevance: 1.0,
                    epistemic_tier: 1.0,
                    relationship_proximity: 0.5,
                    access_frequency: 0.3,
                },
                score: 0.0,
            },
            ScoredResult {
                content: "old observation".to_owned(),
                source_type: "fact".to_owned(),
                source_id: "f2".to_owned(),
                nous_id: "syn".to_owned(),
                factors: FactorScores {
                    vector_similarity: 0.7,
                    decay: old_observation,
                    relevance: 1.0,
                    epistemic_tier: 0.3,
                    relationship_proximity: 0.0,
                    access_frequency: 0.0,
                },
                score: 0.0,
            },
        ];

        let ranked = e.rank(candidates);
        assert_eq!(
            ranked[0].content, "identity fact",
            "fresh verified identity fact should rank first over old assumed observation"
        );
        assert_eq!(
            ranked[1].content, "old observation",
            "old assumed observation should rank second"
        );
    }
}

mod proptests {
    use super::*;
    use proptest::prelude::*;

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
}
