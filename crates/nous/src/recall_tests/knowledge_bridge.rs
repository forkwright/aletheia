#![expect(clippy::indexing_slicing, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions may panic on failure")]

use std::sync::Arc;

use mneme::knowledge::{
    EmbeddedChunk, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
    FactTemporal, Visibility, far_future,
};
use mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};

use super::super::*;
use crate::recall::KnowledgeVectorSearch;

const DIM: usize = 4;

fn make_store() -> Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig {
        dim: DIM,
        ..Default::default()
    })
    .expect("open in-memory store")
}

fn make_chunk(id: &str, content: &str, embedding: Vec<f32>) -> EmbeddedChunk {
    let source_id = format!("fact-{id}");
    EmbeddedChunk {
        id: mneme::id::EmbeddingId::new(id).expect("valid test id"),
        content: content.to_owned(),
        source_type: "fact".to_owned(),
        source_id,
        nous_id: "alice".to_owned(),
        embedding,
        created_at: jiff::Timestamp::from_second(1_735_689_600).expect("valid epoch"),
    }
}

fn make_fact(id: &str, content: &str) -> Fact {
    Fact {
        id: mneme::id::FactId::new(id).expect("valid test fact id"),
        nous_id: "alice".to_owned(),
        fact_type: "test".to_owned(),
        content: content.to_owned(),
        scope: None,
        project_id: None,
        sensitivity: FactSensitivity::Public,
        visibility: Visibility::Private,
        temporal: FactTemporal {
            valid_from: jiff::Timestamp::from_second(1_735_689_600).expect("valid epoch"),
            valid_to: far_future(),
            recorded_at: jiff::Timestamp::from_second(1_735_689_600).expect("valid epoch"),
        },
        provenance: FactProvenance {
            confidence: 1.0,
            tier: EpistemicTier::Verified,
            source_session_id: None,
            stability_hours: 720.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    }
}

fn insert_chunk_fact(store: &KnowledgeStore, chunk: &EmbeddedChunk) {
    store
        .insert_fact(&make_fact(&chunk.source_id, &chunk.content))
        .expect("insert fact");
    store.insert_embedding(chunk).expect("insert embedding");
}

#[test]
fn empty_store_returns_empty_vec() {
    let store = make_store();
    let search = KnowledgeVectorSearch::new(store);
    let results = search
        .search_vectors(vec![0.0; DIM], 5, 10, "alice")
        .expect("search should not error on empty store");
    assert!(results.is_empty(), "empty store should return no results");
}

#[test]
fn returns_matching_results() {
    let store = make_store();
    let chunk = make_chunk("c1", "Rust is a systems language", vec![1.0, 0.0, 0.0, 0.0]);
    insert_chunk_fact(&store, &chunk);

    let search = KnowledgeVectorSearch::new(Arc::clone(&store));
    let results = search
        .search_vectors(vec![1.0, 0.0, 0.0, 0.0], 5, 10, "alice")
        .expect("search");
    assert_eq!(results.len(), 1, "should find one matching result");
    assert_eq!(
        results[0].content, "Rust is a systems language",
        "content should match"
    );
    assert_eq!(results[0].source_type, "fact", "source_type should be fact");
}

#[test]
fn closer_vectors_rank_first() {
    let store = make_store();
    let close = make_chunk("c1", "close", vec![1.0, 0.0, 0.0, 0.0]);
    let far = make_chunk("c2", "far", vec![0.0, 0.0, 0.0, 1.0]);
    insert_chunk_fact(&store, &close);
    insert_chunk_fact(&store, &far);

    let search = KnowledgeVectorSearch::new(Arc::clone(&store));
    let results = search
        .search_vectors(vec![1.0, 0.0, 0.0, 0.0], 5, 10, "alice")
        .expect("search");
    assert_eq!(results.len(), 2, "should find both results");
    assert!(
        results[0].distance <= results[1].distance,
        "closer vector should have smaller distance"
    );
    assert_eq!(
        results[0].content, "close",
        "closest result should be first"
    );
}

#[test]
fn respects_k_limit() {
    let store = make_store();
    for i in 0..5 {
        let mut emb = vec![0.0; DIM];
        emb[i % DIM] = 1.0;
        let chunk = make_chunk(&format!("c{i}"), &format!("fact {i}"), emb);
        insert_chunk_fact(&store, &chunk);
    }

    let search = KnowledgeVectorSearch::new(Arc::clone(&store));
    let results = search
        .search_vectors(vec![1.0, 0.0, 0.0, 0.0], 2, 10, "alice")
        .expect("search");
    assert!(results.len() <= 2, "should return at most k=2 results");
}
