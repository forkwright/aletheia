//! Integration tests for `KnowledgeStore`: fact round-trip (TEST-02) and HNSW vector search (TEST-03).
#![cfg(feature = "engine-tests")]

use aletheia_mneme::embedding::{EmbeddingProvider, MockEmbeddingProvider};
use aletheia_mneme::knowledge::{EmbeddedChunk, EpistemicTier, Fact};
use aletheia_mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};
use std::collections::BTreeMap;

fn make_fact(id: &str, nous_id: &str, content: &str, confidence: f64, tier: EpistemicTier) -> Fact {
    Fact {
        id: id.to_owned(),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        confidence,
        tier,
        valid_from: "2026-01-01".to_owned(),
        valid_to: "9999-12-31".to_owned(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: "2026-03-01T00:00:00Z".to_owned(),
    }
}

// TEST-02: Fact inserted via insert_fact is queryable and returns identical data.
#[test]
fn fact_round_trip() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 })
        .expect("open_mem");

    let fact = Fact {
        id: "f-1".to_owned(),
        nous_id: "syn".to_owned(),
        content: "Alice lives in Springfield".to_owned(),
        confidence: 0.95,
        tier: EpistemicTier::Verified,
        valid_from: "2026-01-01".to_owned(),
        valid_to: "9999-12-31".to_owned(),
        superseded_by: None,
        source_session_id: Some("ses-abc".to_owned()),
        recorded_at: "2026-03-01T00:00:00Z".to_owned(),
    };

    store.insert_fact(&fact).expect("insert");
    let results = store.query_facts("syn", "2026-06-01", 10).expect("query");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "f-1");
    assert_eq!(results[0].content, "Alice lives in Springfield");
    assert!((results[0].confidence - 0.95).abs() < f64::EPSILON);
    assert_eq!(results[0].tier, EpistemicTier::Verified);
}

// TEST-03: HNSW kNN returns the known-similar vector before dissimilar ones.
#[test]
fn hnsw_vector_search() {
    let dim = 16;
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim })
        .expect("open_mem");
    // MockEmbeddingProvider is deterministic by text input — satisfies the
    // "deterministic random vectors" constraint from CONTEXT.md.
    let provider = MockEmbeddingProvider::new(dim);

    let texts = ["apple", "banana", "cherry", "database", "engine", "function"];
    for (i, text) in texts.iter().enumerate() {
        let embedding = provider.embed(text).expect("embed");
        let chunk = EmbeddedChunk {
            id: format!("emb-{i}"),
            content: text.to_string(),
            source_type: "test".to_owned(),
            source_id: format!("src-{i}"),
            nous_id: "syn".to_owned(),
            embedding,
            created_at: "2026-03-01T00:00:00Z".to_owned(),
        };
        store.insert_embedding(&chunk).expect("insert embedding");
    }

    // Query with the "apple" vector — should find "apple" as nearest neighbor.
    let query_vec = provider.embed("apple").expect("embed query");
    let results = store.search_vectors(query_vec, 3, 20).expect("search");

    assert!(!results.is_empty(), "should return results");
    assert_eq!(results[0].content, "apple", "nearest neighbor should be the exact match");
    assert_eq!(results[0].source_type, "test");
    // Cosine distance to identical vector should be ~0.
    assert!(
        results[0].distance < 0.01,
        "self-distance should be near zero, got {}",
        results[0].distance
    );
}

// INTG-05: Schema version is queryable and returns 1.
#[test]
fn schema_version_queryable() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 })
        .expect("open_mem");
    let version = store.schema_version().expect("version");
    assert_eq!(version, 1);
}

// Verify ordering of multiple facts by confidence (descending).
#[test]
fn multiple_facts_ordered_by_confidence() {
    let store = KnowledgeStore::open_mem().expect("open_mem");

    for (i, conf) in [0.5f64, 0.9, 0.7].iter().enumerate() {
        let fact = make_fact(
            &format!("f-{i}"),
            "syn",
            &format!("fact {i}"),
            *conf,
            EpistemicTier::Inferred,
        );
        store.insert_fact(&fact).expect("insert");
    }

    let results = store.query_facts("syn", "2026-06-01", 10).expect("query");
    assert_eq!(results.len(), 3);
    // FULL_CURRENT_FACTS orders by -confidence
    assert!((results[0].confidence - 0.9).abs() < f64::EPSILON);
    assert!((results[1].confidence - 0.7).abs() < f64::EPSILON);
    assert!((results[2].confidence - 0.5).abs() < f64::EPSILON);
}

// INTG-04: spawn_blocking async wrappers work from #[tokio::test] context.
#[tokio::test]
async fn async_spawn_blocking_wrapper() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 })
        .expect("open_mem");

    let fact = Fact {
        id: "f-async".to_owned(),
        nous_id: "syn".to_owned(),
        content: "async test fact".to_owned(),
        confidence: 0.8,
        tier: EpistemicTier::Assumed,
        valid_from: "2026-01-01".to_owned(),
        valid_to: "9999-12-31".to_owned(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: "2026-03-01T00:00:00Z".to_owned(),
    };

    store.insert_fact_async(fact).await.expect("async insert");
    let results = store
        .query_facts_async("syn".to_owned(), "2026-06-01".to_owned(), 10)
        .await
        .expect("async query");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "async test fact");
}

// Raw query escape hatch returns the expected relations.
#[test]
fn raw_query_escape_hatch() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 })
        .expect("open_mem");

    let rows = store.run_query("::relations", BTreeMap::new()).expect("relations");
    // Should have at least: facts, entities, relationships, embeddings, _schema_version
    assert!(
        rows.rows.len() >= 5,
        "expected at least 5 relations, got {}",
        rows.rows.len()
    );
}
