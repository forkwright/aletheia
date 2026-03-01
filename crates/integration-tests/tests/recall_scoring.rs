//! Cross-crate tests for mneme recall scoring engine.

use aletheia_mneme::knowledge::EpistemicTier;
use aletheia_mneme::recall::{FactorScores, RecallEngine, RecallWeights, ScoredResult};

fn make_result(
    content: &str,
    nous_id: &str,
    factors: FactorScores,
) -> ScoredResult {
    ScoredResult {
        content: content.to_owned(),
        source_type: "fact".to_owned(),
        source_id: format!("f-{content}"),
        nous_id: nous_id.to_owned(),
        factors,
        score: 0.0,
    }
}

#[test]
fn verified_fact_scores_higher_than_assumed() {
    let engine = RecallEngine::new();

    let verified = make_result("verified-fact", "syn", FactorScores {
        vector_similarity: 0.8,
        recency: 0.5,
        relevance: 1.0,
        epistemic_tier: engine.score_epistemic_tier(EpistemicTier::Verified.as_str()),
        relationship_proximity: 0.5,
        access_frequency: 0.3,
    });
    let assumed = make_result("assumed-fact", "syn", FactorScores {
        vector_similarity: 0.8,
        recency: 0.5,
        relevance: 1.0,
        epistemic_tier: engine.score_epistemic_tier(EpistemicTier::Assumed.as_str()),
        relationship_proximity: 0.5,
        access_frequency: 0.3,
    });

    let ranked = engine.rank(vec![assumed, verified]);
    assert_eq!(ranked[0].content, "verified-fact");
}

#[test]
fn own_fact_outranks_other_agent() {
    let engine = RecallEngine::new();

    let own = make_result("own-fact", "syn", FactorScores {
        vector_similarity: 0.7,
        recency: 0.5,
        relevance: engine.score_relevance("syn", "syn"),
        epistemic_tier: engine.score_epistemic_tier("inferred"),
        relationship_proximity: 0.5,
        access_frequency: 0.3,
    });
    let other = make_result("other-fact", "demiurge", FactorScores {
        vector_similarity: 0.7,
        recency: 0.5,
        relevance: engine.score_relevance("demiurge", "syn"),
        epistemic_tier: engine.score_epistemic_tier("inferred"),
        relationship_proximity: 0.5,
        access_frequency: 0.3,
    });

    let ranked = engine.rank(vec![other, own]);
    assert_eq!(ranked[0].content, "own-fact");
}

#[test]
fn recent_fact_outranks_old() {
    let engine = RecallEngine::new();

    let recent = make_result("recent", "syn", FactorScores {
        vector_similarity: 0.7,
        recency: engine.score_recency(1.0),
        relevance: 1.0,
        epistemic_tier: 0.6,
        relationship_proximity: 0.5,
        access_frequency: 0.3,
    });
    let old = make_result("old", "syn", FactorScores {
        vector_similarity: 0.7,
        recency: engine.score_recency(720.0),
        relevance: 1.0,
        epistemic_tier: 0.6,
        relationship_proximity: 0.5,
        access_frequency: 0.3,
    });

    let ranked = engine.rank(vec![old, recent]);
    assert_eq!(ranked[0].content, "recent");
}

#[test]
fn custom_weights_shift_ranking() {
    // Only epistemic_tier weight matters
    let weights = RecallWeights {
        vector_similarity: 0.0,
        recency: 0.0,
        relevance: 0.0,
        epistemic_tier: 1.0,
        relationship_proximity: 0.0,
        access_frequency: 0.0,
    };
    let engine = RecallEngine::with_weights(weights);

    // Verified from other agent should beat assumed from self
    let verified_other = make_result("verified-other", "demiurge", FactorScores {
        vector_similarity: 0.1,
        recency: 0.1,
        relevance: engine.score_relevance("demiurge", "syn"),
        epistemic_tier: engine.score_epistemic_tier("verified"),
        relationship_proximity: 0.0,
        access_frequency: 0.0,
    });
    let assumed_self = make_result("assumed-self", "syn", FactorScores {
        vector_similarity: 0.9,
        recency: 0.9,
        relevance: engine.score_relevance("syn", "syn"),
        epistemic_tier: engine.score_epistemic_tier("assumed"),
        relationship_proximity: 1.0,
        access_frequency: 1.0,
    });

    let ranked = engine.rank(vec![assumed_self, verified_other]);
    assert_eq!(ranked[0].content, "verified-other");
}

#[test]
fn epistemic_tier_as_str_roundtrips() {
    let engine = RecallEngine::new();

    for tier in [EpistemicTier::Verified, EpistemicTier::Inferred, EpistemicTier::Assumed] {
        let s = tier.as_str();
        let score = engine.score_epistemic_tier(s);
        assert!(score > 0.0);
        assert!(score <= 1.0);
    }

    // Verify ordering: verified > inferred > assumed
    let v = engine.score_epistemic_tier(EpistemicTier::Verified.as_str());
    let i = engine.score_epistemic_tier(EpistemicTier::Inferred.as_str());
    let a = engine.score_epistemic_tier(EpistemicTier::Assumed.as_str());
    assert!(v > i);
    assert!(i > a);
}
