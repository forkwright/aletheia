//! Tests for ranking, custom weights, boundary conditions, and acceptance criteria.
#![expect(clippy::expect_used, reason = "test assertions")]
use super::super::*;

fn engine() -> RecallEngine {
    RecallEngine::new()
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
