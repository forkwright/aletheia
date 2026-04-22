//! (Split from `recall_tests.rs` — see parent mod.)

#![expect(clippy::indexing_slicing, reason = "test assertions")]
#![expect(clippy::expect_used, reason = "test assertions may panic on failure")]

use std::sync::Arc;

use mneme::knowledge::EmbeddedChunk;
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
    EmbeddedChunk {
        id: mneme::id::EmbeddingId::new(id).expect("valid test id"),
        content: content.to_owned(),
        source_type: "fact".to_owned(),
        source_id: format!("fact-{id}"),
        nous_id: String::new(),
        embedding,
        created_at: jiff::Timestamp::from_second(1_735_689_600).expect("valid epoch"),
    }
}

#[test]
fn empty_store_returns_empty_vec() {
    let store = make_store();
    let search = KnowledgeVectorSearch::new(store);
    let results = search
        .search_vectors(vec![0.0; DIM], 5, 10)
        .expect("search should not error on empty store");
    assert!(results.is_empty(), "empty store should return no results");
}

#[test]
fn returns_matching_results() {
    let store = make_store();
    let chunk = make_chunk("c1", "Rust is a systems language", vec![1.0, 0.0, 0.0, 0.0]);
    store.insert_embedding(&chunk).expect("insert embedding");

    let search = KnowledgeVectorSearch::new(Arc::clone(&store));
    let results = search
        .search_vectors(vec![1.0, 0.0, 0.0, 0.0], 5, 10)
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
    store.insert_embedding(&close).expect("insert close");
    store.insert_embedding(&far).expect("insert far");

    let search = KnowledgeVectorSearch::new(Arc::clone(&store));
    let results = search
        .search_vectors(vec![1.0, 0.0, 0.0, 0.0], 5, 10)
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
        store.insert_embedding(&chunk).expect("insert");
    }

    let search = KnowledgeVectorSearch::new(Arc::clone(&store));
    let results = search
        .search_vectors(vec![1.0, 0.0, 0.0, 0.0], 2, 10)
        .expect("search");
    assert!(results.len() <= 2, "should return at most k=2 results");
}
