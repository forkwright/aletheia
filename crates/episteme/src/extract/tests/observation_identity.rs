//! Regression tests for observation identity (#5305).
//!
//! Extraction fact ids are content-addressed over (nous, source session,
//! subject, predicate, object): identical triples from different sessions are
//! distinct observations, while re-persisting one session's extraction is a
//! retry-safe upsert onto the same id.
#![cfg_attr(
    feature = "mneme-engine",
    expect(clippy::expect_used, reason = "test assertions")
)]

#[cfg(feature = "mneme-engine")]
use super::super::*;

#[cfg(feature = "mneme-engine")]
fn triple(subject: &str, predicate: &str, object: &str) -> ExtractedFact {
    ExtractedFact {
        subject: subject.to_owned(),
        predicate: predicate.to_owned(),
        object: object.to_owned(),
        confidence: 0.9,
        fact_type: None,
        is_correction: false,
    }
}

#[cfg(feature = "mneme-engine")]
fn single_fact_extraction(fact: ExtractedFact) -> Extraction {
    Extraction {
        entities: vec![],
        relationships: vec![],
        facts: vec![fact],
    }
}

#[cfg(feature = "mneme-engine")]
fn stored_fact_ids(store: &crate::knowledge_store::KnowledgeStore) -> Vec<String> {
    let mut ids: Vec<String> = store
        .list_all_facts(1000)
        .expect("list facts")
        .iter()
        .map(|f| f.id.as_str().to_owned())
        .collect();
    ids.sort_unstable();
    ids
}

/// Two sessions extracting the same triple must produce two distinct
/// observations, not one collapsed row.
#[cfg(feature = "mneme-engine")]
#[test]
fn same_triple_from_two_sessions_yields_distinct_observation_ids() {
    let store = crate::knowledge_store::KnowledgeStore::open_mem()
        .expect("in-memory knowledge store should open successfully");
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let extraction = single_fact_extraction(triple("Alice", "prefers", "Rust"));

    engine
        .persist(&extraction, &store, "session:a", "syn")
        .expect("first persist should succeed");
    engine
        .persist(&extraction, &store, "session:b", "syn")
        .expect("second persist should succeed");

    let id_a = super::super::engine::observation_fact_id("syn", "session:a", "Alice", "prefers", "Rust")
        .expect("valid fact id");
    let id_b = super::super::engine::observation_fact_id("syn", "session:b", "Alice", "prefers", "Rust")
        .expect("valid fact id");
    assert_ne!(
        id_a, id_b,
        "identical triples from different sessions must not share an observation id"
    );

    let stored = stored_fact_ids(&store);
    assert!(
        stored.iter().any(|id| id == id_a.as_str()),
        "first session observation persisted: {stored:?}"
    );
    assert!(
        stored.iter().any(|id| id == id_b.as_str()),
        "second session observation persisted: {stored:?}"
    );
}

/// Re-persisting one session's extraction must converge on the same
/// observation id (retry-safe), never fork a new identity.
#[cfg(feature = "mneme-engine")]
#[test]
fn re_persisting_same_session_is_retry_safe() {
    let store = crate::knowledge_store::KnowledgeStore::open_mem()
        .expect("in-memory knowledge store should open successfully");
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let extraction = single_fact_extraction(triple("Alice", "prefers", "Rust"));

    engine
        .persist(&extraction, &store, "session:a", "syn")
        .expect("first persist should succeed");
    engine
        .persist(&extraction, &store, "session:a", "syn")
        .expect("retry persist should succeed");

    let expected =
        super::super::engine::observation_fact_id("syn", "session:a", "Alice", "prefers", "Rust")
            .expect("valid fact id");
    let stored = stored_fact_ids(&store);
    assert!(
        stored.iter().all(|id| id == expected.as_str()),
        "every persisted row for this observation carries the same id: {stored:?}"
    );
}

/// Two triples sharing subject and predicate but differing in object are
/// distinct observations even within one extraction.
#[cfg(feature = "mneme-engine")]
#[test]
fn same_subject_predicate_different_object_yields_distinct_ids() {
    let id_rust = super::super::engine::observation_fact_id("syn", "session:a", "Alice", "prefers", "Rust")
        .expect("valid fact id");
    let id_go = super::super::engine::observation_fact_id("syn", "session:a", "Alice", "prefers", "Go")
        .expect("valid fact id");
    assert_ne!(id_rust, id_go);
}

/// The observation id stays within the id-newtype length bound even for
/// long subject text, since only bounded slug prefixes precede the hash.
#[cfg(feature = "mneme-engine")]
#[test]
fn observation_id_is_bounded_for_long_inputs() {
    let long_subject = "x".repeat(10_000);
    let id = super::super::engine::observation_fact_id("syn", "session:a", &long_subject, "prefers", "Rust")
        .expect("long inputs still produce a valid fact id");
    assert!(id.as_str().len() <= 256);
}
