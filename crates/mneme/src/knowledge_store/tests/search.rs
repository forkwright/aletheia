#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;
use crate::knowledge::{EmbeddedChunk, EpistemicTier, Fact, ForgetReason};
use std::sync::Arc;

const DIM: usize = 4;

fn make_store() -> Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: DIM }).expect("open_mem")
}

fn test_ts(s: &str) -> jiff::Timestamp {
    crate::knowledge::parse_timestamp(s).expect("valid test timestamp in test helper")
}

fn make_fact(id: &str, nous_id: &str, content: &str) -> Fact {
    Fact {
        id: crate::id::FactId::new_unchecked(id),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        confidence: 0.9,
        tier: EpistemicTier::Inferred,
        valid_from: test_ts("2026-01-01"),
        valid_to: crate::knowledge::far_future(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: test_ts("2026-03-01T00:00:00Z"),
        access_count: 0,
        last_accessed_at: None,
        stability_hours: 720.0,
        fact_type: String::new(),
        is_forgotten: false,
        forgotten_at: None,
        forget_reason: None,
    }
}

fn make_embedding(id: &str, content: &str, source_id: &str, nous_id: &str) -> EmbeddedChunk {
    EmbeddedChunk {
        id: crate::id::EmbeddingId::new_unchecked(id),
        content: content.to_owned(),
        source_type: "fact".to_owned(),
        source_id: source_id.to_owned(),
        nous_id: nous_id.to_owned(),
        embedding: vec![0.5, 0.5, 0.5, 0.5],
        created_at: test_ts("2026-03-01T00:00:00Z"),
    }
}

#[test]
fn insert_embedding_and_search() {
    let store = make_store();
    let fact = make_fact("f1", "agent-a", "Rust memory safety");
    store.insert_fact(&fact).expect("insert fact");

    let mut chunk = make_embedding("emb1", "Rust memory safety", "f1", "agent-a");
    chunk.embedding = vec![0.9, 0.1, 0.0, 0.0];
    store.insert_embedding(&chunk).expect("insert embedding");

    let results = store
        .search_vectors(vec![0.9, 0.1, 0.0, 0.0], 5, 20)
        .expect("search vectors");
    assert!(!results.is_empty());
    assert_eq!(results[0].content, "Rust memory safety");
    assert_eq!(results[0].source_type, "fact");
    assert_eq!(results[0].source_id, "f1");
}

#[test]
fn search_vectors_empty_store_returns_empty() {
    let store = make_store();
    let results = store
        .search_vectors(vec![0.5, 0.5, 0.5, 0.5], 5, 20)
        .expect("search empty");
    assert!(results.is_empty());
}

#[test]
fn search_vectors_returns_nearest() {
    let store = make_store();

    let mut chunk_a = make_embedding("emb-a", "Rust programming", "f1", "agent-a");
    chunk_a.embedding = vec![1.0, 0.0, 0.0, 0.0];
    store.insert_embedding(&chunk_a).expect("insert emb-a");

    let mut chunk_b = make_embedding("emb-b", "Python scripting", "f2", "agent-a");
    chunk_b.embedding = vec![0.0, 1.0, 0.0, 0.0];
    store.insert_embedding(&chunk_b).expect("insert emb-b");

    // Query close to chunk_a
    let results = store
        .search_vectors(vec![0.9, 0.1, 0.0, 0.0], 1, 20)
        .expect("search nearest");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "Rust programming");
}

#[test]
fn search_vectors_respects_k() {
    let store = make_store();
    for i in 0..10 {
        let mut chunk = make_embedding(
            &format!("emb-{i}"),
            &format!("Content {i}"),
            &format!("f{i}"),
            "agent-a",
        );
        #[expect(
            clippy::cast_precision_loss,
            reason = "test data — i is 0..9, fits in f32"
        )]
        let component = i as f32 * 0.1;
        chunk.embedding = vec![component, 0.5, 0.3, 0.1];
        store.insert_embedding(&chunk).expect("insert");
    }

    let results = store
        .search_vectors(vec![0.5, 0.5, 0.3, 0.1], 3, 20)
        .expect("search k=3");
    assert_eq!(results.len(), 3);
}

#[test]
fn search_vectors_dimension_mismatch_errors() {
    let store = make_store();
    let wrong_dim = vec![0.1, 0.2, 0.3, 0.4, 0.5];
    let result = store.search_vectors(wrong_dim, 5, 16);
    assert!(result.is_err(), "search with wrong dimension should error");
}

#[tokio::test]
async fn search_vectors_async_works() {
    let store = make_store();
    let mut chunk = make_embedding("emb-async", "Async search content", "f1", "agent-a");
    chunk.embedding = vec![0.7, 0.3, 0.0, 0.0];
    store.insert_embedding(&chunk).expect("insert embedding");

    let results = store
        .search_vectors_async(vec![0.7, 0.3, 0.0, 0.0], 5, 20)
        .await
        .expect("async search");
    assert!(!results.is_empty());
}

mod correctness {
    use super::super::super::*;
    use crate::knowledge::{EmbeddedChunk, EpistemicTier, Fact, ForgetReason};

    const DIM: usize = 4;

    fn make_store() -> std::sync::Arc<KnowledgeStore> {
        KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: DIM })
            .expect("open in-memory store")
    }

    fn ts(s: &str) -> jiff::Timestamp {
        crate::knowledge::parse_timestamp(s).expect("valid test timestamp")
    }

    fn fact(id: &str, content: &str) -> Fact {
        Fact {
            id: crate::id::FactId::new_unchecked(id),
            nous_id: "test-nous".to_owned(),
            content: content.to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: ts("2026-01-01"),
            valid_to: crate::knowledge::far_future(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: ts("2026-03-01T00:00:00Z"),
            access_count: 0,
            last_accessed_at: None,
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        }
    }

    fn embedding(id: &str, source_id: &str, vec: Vec<f32>) -> EmbeddedChunk {
        EmbeddedChunk {
            id: crate::id::EmbeddingId::new_unchecked(id),
            content: format!("content for {id}"),
            source_type: "fact".to_owned(),
            source_id: source_id.to_owned(),
            nous_id: "test-nous".to_owned(),
            embedding: vec,
            created_at: ts("2026-03-01T00:00:00Z"),
        }
    }

    // Bug #1114: search_vectors must not return forgotten facts.
    #[test]
    fn search_vectors_excludes_forgotten_facts() {
        let store = make_store();

        let f_live = fact("sv-live", "live banjo fact");
        let f_forgotten = fact("sv-forgotten", "forgotten banjo fact");
        store.insert_fact(&f_live).expect("insert live fact");
        store
            .insert_fact(&f_forgotten)
            .expect("insert forgotten fact");

        store
            .insert_embedding(&embedding("sv-live", "sv-live", vec![1.0, 0.0, 0.0, 0.0]))
            .expect("insert live embedding");
        store
            .insert_embedding(&embedding(
                "sv-forgotten",
                "sv-forgotten",
                vec![1.0, 0.0, 0.0, 0.0],
            ))
            .expect("insert forgotten embedding");

        store
            .forget_fact(
                &crate::id::FactId::new_unchecked("sv-forgotten"),
                ForgetReason::UserRequested,
            )
            .expect("forget fact");

        let results = store
            .search_vectors(vec![1.0, 0.0, 0.0, 0.0], 10, 20)
            .expect("search_vectors");

        let ids: Vec<&str> = results.iter().map(|r| r.source_id.as_str()).collect();
        assert!(
            !ids.contains(&"sv-forgotten"),
            "forgotten fact must not appear in search_vectors results: {ids:?}"
        );
        assert!(
            ids.contains(&"sv-live"),
            "live fact must still appear in results: {ids:?}"
        );
    }

    // Bug #1117: insert_embedding must reject vectors with wrong dimension.
    #[test]
    fn insert_embedding_rejects_wrong_dimension() {
        let store = make_store(); // DIM = 4
        let fact_data = fact("dim-check", "dimension validation fact");
        store.insert_fact(&fact_data).expect("insert fact");

        let wrong_dim = EmbeddedChunk {
            id: crate::id::EmbeddingId::new_unchecked("dim-check"),
            content: "dimension validation fact".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "dim-check".to_owned(),
            nous_id: "test-nous".to_owned(),
            // Wrong: 6 values instead of DIM=4
            embedding: vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
            created_at: ts("2026-03-01T00:00:00Z"),
        };

        let err = store
            .insert_embedding(&wrong_dim)
            .expect_err("must reject wrong-dimension embedding");
        assert!(
            matches!(
                err,
                crate::error::Error::EmbeddingDimensionMismatch {
                    expected: 4,
                    actual: 6,
                    ..
                }
            ),
            "expected EmbeddingDimensionMismatch, got: {err}"
        );
    }

    // Bug #1117: insert_embedding must accept vectors with correct dimension.
    #[test]
    fn insert_embedding_accepts_correct_dimension() {
        let store = make_store(); // DIM = 4
        let fact_data = fact("dim-ok", "correct dimension fact");
        store.insert_fact(&fact_data).expect("insert fact");

        let correct = EmbeddedChunk {
            id: crate::id::EmbeddingId::new_unchecked("dim-ok"),
            content: "correct dimension fact".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "dim-ok".to_owned(),
            nous_id: "test-nous".to_owned(),
            embedding: vec![0.5, 0.5, 0.5, 0.5],
            created_at: ts("2026-03-01T00:00:00Z"),
        };
        store
            .insert_embedding(&correct)
            .expect("correct-dimension embedding must be accepted");
    }
}
