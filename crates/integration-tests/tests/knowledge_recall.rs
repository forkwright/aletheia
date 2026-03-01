//! Integration: knowledge types flow into recall scoring.

use aletheia_mneme::knowledge::{EmbeddedChunk, Entity, EpistemicTier, Fact, RecallResult, Relationship};
use aletheia_mneme::recall::{FactorScores, RecallEngine, ScoredResult};

fn fact_to_scored(fact: &Fact, engine: &RecallEngine, query_nous: &str) -> ScoredResult {
    ScoredResult {
        content: fact.content.clone(),
        source_type: "fact".to_owned(),
        source_id: fact.id.clone(),
        nous_id: fact.nous_id.clone(),
        factors: FactorScores {
            vector_similarity: 0.8,
            recency: 0.5,
            relevance: engine.score_relevance(&fact.nous_id, query_nous),
            epistemic_tier: engine.score_epistemic_tier(fact.tier.as_str()),
            relationship_proximity: 0.5,
            access_frequency: 0.3,
        },
        score: 0.0,
    }
}

fn sample_fact(id: &str, nous_id: &str, tier: EpistemicTier) -> Fact {
    Fact {
        id: id.to_owned(),
        nous_id: nous_id.to_owned(),
        content: format!("fact from {id}"),
        confidence: 0.9,
        tier,
        valid_from: "2026-01-01".to_owned(),
        valid_to: "9999-12-31".to_owned(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: "2026-03-01T00:00:00Z".to_owned(),
    }
}

#[test]
fn facts_scored_and_ranked_by_tier() {
    let engine = RecallEngine::new();

    let verified = sample_fact("f-1", "syn", EpistemicTier::Verified);
    let inferred = sample_fact("f-2", "syn", EpistemicTier::Inferred);
    let assumed = sample_fact("f-3", "syn", EpistemicTier::Assumed);

    let candidates = vec![
        fact_to_scored(&assumed, &engine, "syn"),
        fact_to_scored(&verified, &engine, "syn"),
        fact_to_scored(&inferred, &engine, "syn"),
    ];

    let ranked = engine.rank(candidates);
    assert_eq!(ranked[0].source_id, "f-1");
    assert_eq!(ranked[1].source_id, "f-2");
    assert_eq!(ranked[2].source_id, "f-3");
}

#[test]
fn own_facts_ranked_above_shared() {
    let engine = RecallEngine::new();

    let own = sample_fact("f-own", "syn", EpistemicTier::Inferred);
    let shared = sample_fact("f-shared", "", EpistemicTier::Inferred);

    let candidates = vec![
        fact_to_scored(&shared, &engine, "syn"),
        fact_to_scored(&own, &engine, "syn"),
    ];

    let ranked = engine.rank(candidates);
    assert_eq!(ranked[0].source_id, "f-own");
}

#[test]
fn knowledge_types_all_serialize() {
    let fact = sample_fact("f-1", "syn", EpistemicTier::Verified);
    let entity = Entity {
        id: "e-1".to_owned(),
        name: "Alice".to_owned(),
        entity_type: "person".to_owned(),
        aliases: vec!["CK".to_owned()],
        created_at: "2026-01-01T00:00:00Z".to_owned(),
        updated_at: "2026-03-01T00:00:00Z".to_owned(),
    };
    let rel = Relationship {
        src: "e-1".to_owned(),
        dst: "e-2".to_owned(),
        relation: "works_on".to_owned(),
        weight: 0.9,
        created_at: "2026-01-01T00:00:00Z".to_owned(),
    };
    let chunk = EmbeddedChunk {
        id: "emb-1".to_owned(),
        content: "some text".to_owned(),
        source_type: "fact".to_owned(),
        source_id: "f-1".to_owned(),
        nous_id: "syn".to_owned(),
        embedding: vec![0.1, 0.2, 0.3],
        created_at: "2026-01-01T00:00:00Z".to_owned(),
    };
    let result = RecallResult {
        content: "some text".to_owned(),
        distance: 0.1,
        source_type: "fact".to_owned(),
        source_id: "f-1".to_owned(),
    };

    assert!(serde_json::to_string(&fact).is_ok());
    assert!(serde_json::to_string(&entity).is_ok());
    assert!(serde_json::to_string(&rel).is_ok());
    assert!(serde_json::to_string(&chunk).is_ok());
    assert!(serde_json::to_string(&result).is_ok());
}
