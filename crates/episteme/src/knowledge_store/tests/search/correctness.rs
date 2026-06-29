#![expect(clippy::expect_used, reason = "test assertions")]

use crate::knowledge::{EmbeddedChunk, ForgetReason};
use crate::test_fixtures::{make_store, test_ts};

/// Shorthand: build a fact with a fixed `nous_id` for correctness tests.
fn fact(id: &str, content: &str) -> crate::knowledge::Fact {
    crate::test_fixtures::make_fact(id, "test-nous", content)
}

fn embedding(id: &str, source_id: &str, vec: Vec<f32>) -> EmbeddedChunk {
    EmbeddedChunk {
        id: crate::id::EmbeddingId::new(id).expect("valid test id"),
        content: format!("content for {id}"),
        source_type: "fact".to_owned(),
        source_id: source_id.to_owned(),
        nous_id: "test-nous".to_owned(),
        embedding: vec,
        created_at: test_ts("2026-03-01T00:00:00Z"),
    }
}

// WHY: Bug #1114: search_vectors must not return forgotten facts.
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
            &crate::id::FactId::new("sv-forgotten").expect("valid test id"),
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

#[test]
fn insert_embedding_rejects_wrong_dimension() {
    let store = make_store(); // DIM = 4
    let fact_data = fact("dim-check", "dimension validation fact");
    store.insert_fact(&fact_data).expect("insert fact");

    let wrong_dim = EmbeddedChunk {
        id: crate::id::EmbeddingId::new("dim-check").expect("valid test id"),
        content: "dimension validation fact".to_owned(),
        source_type: "fact".to_owned(),
        source_id: "dim-check".to_owned(),
        nous_id: "test-nous".to_owned(),
        // WHY: Wrong: 6 values instead of DIM=4
        embedding: vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
        created_at: test_ts("2026-03-01T00:00:00Z"),
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

#[test]
fn insert_embedding_accepts_correct_dimension() {
    let store = make_store(); // DIM = 4
    let fact_data = fact("dim-ok", "correct dimension fact");
    store.insert_fact(&fact_data).expect("insert fact");

    let correct = EmbeddedChunk {
        id: crate::id::EmbeddingId::new("dim-ok").expect("valid test id"),
        content: "correct dimension fact".to_owned(),
        source_type: "fact".to_owned(),
        source_id: "dim-ok".to_owned(),
        nous_id: "test-nous".to_owned(),
        embedding: vec![0.5, 0.5, 0.5, 0.5],
        created_at: test_ts("2026-03-01T00:00:00Z"),
    };
    store
        .insert_embedding(&correct)
        .expect("correct-dimension embedding must be accepted");
}
