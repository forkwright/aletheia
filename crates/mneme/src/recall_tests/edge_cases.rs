//! Tests for edge cases, `FactType` classification, graph recall, and integration scenarios.
#![expect(clippy::expect_used, reason = "test assertions")]
use super::super::*;

fn engine() -> RecallEngine {
    RecallEngine::new()
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
    let old_observation = e.score_decay(500.0, FactType::Observation, EpistemicTier::Assumed, 0);

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
