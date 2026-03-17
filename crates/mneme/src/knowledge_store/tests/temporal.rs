#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;
use crate::knowledge::{EmbeddedChunk, EpistemicTier, Fact};
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

fn make_temporal_fact(
    id: &str,
    nous_id: &str,
    content: &str,
    valid_from: &str,
    valid_to: &str,
) -> Fact {
    Fact {
        id: crate::id::FactId::new_unchecked(id),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        confidence: 0.9,
        tier: EpistemicTier::Inferred,
        valid_from: crate::knowledge::parse_timestamp(valid_from)
            .expect("valid_from timestamp in make_temporal_fact"),
        valid_to: crate::knowledge::parse_timestamp(valid_to)
            .expect("valid_to timestamp in make_temporal_fact"),
        superseded_by: None,
        source_session_id: None,
        recorded_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
            .expect("recorded_at timestamp in make_temporal_fact"),
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

#[cfg(feature = "mneme-engine")]
#[test]
fn temporal_query_point_in_time() {
    let store = make_store();
    store
        .insert_fact(&make_temporal_fact(
            "t1",
            "agent",
            "Rust is fast",
            "2026-01-01",
            "2026-06-01",
        ))
        .expect("insert t1");
    store
        .insert_fact(&make_temporal_fact(
            "t2",
            "agent",
            "Python is dynamic",
            "2026-03-01",
            "9999-12-31",
        ))
        .expect("insert t2");
    let at_feb = store
        .query_facts_temporal("agent", "2026-02-01", None)
        .expect("query feb");
    assert_eq!(at_feb.len(), 1);
    assert_eq!(at_feb[0].id.as_str(), "t1");
    let at_apr = store
        .query_facts_temporal("agent", "2026-04-01", None)
        .expect("query apr");
    assert_eq!(at_apr.len(), 2);
    let at_jul = store
        .query_facts_temporal("agent", "2026-07-01", None)
        .expect("query jul");
    assert_eq!(at_jul.len(), 1);
    assert_eq!(at_jul[0].id.as_str(), "t2");
}

#[cfg(feature = "mneme-engine")]
#[test]
fn temporal_query_before_any_facts_returns_empty() {
    let store = make_store();
    store
        .insert_fact(&make_temporal_fact(
            "t1",
            "agent",
            "fact",
            "2026-06-01",
            "9999-12-31",
        ))
        .expect("insert");
    let results = store
        .query_facts_temporal("agent", "2026-01-01", None)
        .expect("query");
    assert!(results.is_empty());
}

#[cfg(feature = "mneme-engine")]
#[test]
fn temporal_query_boundary_inclusion() {
    let store = make_store();
    store
        .insert_fact(&make_temporal_fact(
            "t1",
            "agent",
            "boundary fact",
            "2026-03-01",
            "2026-06-01",
        ))
        .expect("insert");
    let at_start = store
        .query_facts_temporal("agent", "2026-03-01T00:00:00Z", None)
        .expect("at valid_from");
    assert_eq!(at_start.len(), 1, "valid_from boundary is inclusive");
    let at_end = store
        .query_facts_temporal("agent", "2026-06-01T00:00:00Z", None)
        .expect("at valid_to");
    assert!(at_end.is_empty(), "valid_to boundary is exclusive");
}

#[cfg(feature = "mneme-engine")]
#[test]
fn temporal_query_with_content_filter() {
    let store = make_store();
    store
        .insert_fact(&make_temporal_fact(
            "t1",
            "agent",
            "Rust is fast",
            "2026-01-01",
            "9999-12-31",
        ))
        .expect("insert t1");
    store
        .insert_fact(&make_temporal_fact(
            "t2",
            "agent",
            "Python is dynamic",
            "2026-01-01",
            "9999-12-31",
        ))
        .expect("insert t2");
    let filtered = store
        .query_facts_temporal("agent", "2026-03-01", Some("Rust"))
        .expect("filtered query");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id.as_str(), "t1");
}

#[cfg(feature = "mneme-engine")]
#[test]
fn temporal_diff_added_and_removed() {
    let store = make_store();
    store
        .insert_fact(&make_temporal_fact(
            "old",
            "agent",
            "old knowledge",
            "2026-01-01",
            "2026-03-01",
        ))
        .expect("insert old");
    store
        .insert_fact(&make_temporal_fact(
            "new",
            "agent",
            "new knowledge",
            "2026-02-15",
            "9999-12-31",
        ))
        .expect("insert new");
    let diff = store
        .query_facts_diff("agent", "2026-02-01", "2026-04-01")
        .expect("diff");
    assert_eq!(diff.added.len(), 1, "one fact added in interval");
    assert_eq!(diff.added[0].id.as_str(), "new");
    assert_eq!(diff.removed.len(), 1, "one fact removed in interval");
    assert_eq!(diff.removed[0].id.as_str(), "old");
}

#[cfg(feature = "mneme-engine")]
#[test]
fn temporal_diff_supersession_chain() {
    let store = make_store();
    let mut fact_a = make_temporal_fact("a", "agent", "version 1", "2026-01-01", "2026-03-01");
    fact_a.superseded_by = Some(crate::id::FactId::new_unchecked("b"));
    store.insert_fact(&fact_a).expect("insert a");
    store
        .insert_fact(&make_temporal_fact(
            "b",
            "agent",
            "version 2",
            "2026-03-01",
            "9999-12-31",
        ))
        .expect("insert b");
    let diff = store
        .query_facts_diff("agent", "2026-02-01", "2026-04-01")
        .expect("diff");
    assert_eq!(diff.modified.len(), 1, "one modified pair");
    assert_eq!(diff.modified[0].0.id.as_str(), "a");
    assert_eq!(diff.modified[0].1.id.as_str(), "b");
    assert!(diff.added.is_empty(), "superseded new is not in pure added");
    assert!(
        diff.removed.is_empty(),
        "superseding old is not in pure removed"
    );
}

#[cfg(feature = "mneme-engine")]
#[test]
fn temporal_query_isolates_nous_ids() {
    let store = make_store();
    store
        .insert_fact(&make_temporal_fact(
            "t1",
            "alice",
            "Alice knows Rust",
            "2026-01-01",
            "9999-12-31",
        ))
        .expect("insert alice");
    store
        .insert_fact(&make_temporal_fact(
            "t2",
            "bob",
            "Bob knows Python",
            "2026-01-01",
            "9999-12-31",
        ))
        .expect("insert bob");
    let alice_facts = store
        .query_facts_temporal("alice", "2026-03-01", None)
        .expect("alice query");
    assert_eq!(alice_facts.len(), 1);
    assert_eq!(alice_facts[0].content, "Alice knows Rust");
    let bob_facts = store
        .query_facts_temporal("bob", "2026-03-01", None)
        .expect("bob query");
    assert_eq!(bob_facts.len(), 1);
    assert_eq!(bob_facts[0].content, "Bob knows Python");
}

#[cfg(feature = "mneme-engine")]
#[test]
fn temporal_query_excludes_forgotten_facts() {
    let store = make_store();
    store
        .insert_fact(&make_temporal_fact(
            "t1",
            "agent",
            "forgotten fact",
            "2026-01-01",
            "9999-12-31",
        ))
        .expect("insert");
    store
        .forget_fact(
            &crate::id::FactId::new_unchecked("t1"),
            crate::knowledge::ForgetReason::UserRequested,
        )
        .expect("forget");
    let results = store
        .query_facts_temporal("agent", "2026-03-01", None)
        .expect("query");
    assert!(results.is_empty(), "forgotten facts should be excluded");
}

#[cfg(feature = "mneme-engine")]
#[tokio::test]
async fn temporal_query_async_works() {
    let store = make_store();
    store
        .insert_fact(&make_temporal_fact(
            "t1",
            "agent",
            "async temporal",
            "2026-01-01",
            "9999-12-31",
        ))
        .expect("insert");
    let results = store
        .query_facts_temporal_async("agent".to_owned(), "2026-03-01".to_owned(), None)
        .await
        .expect("async query");
    assert_eq!(results.len(), 1);
}

#[cfg(feature = "mneme-engine")]
#[tokio::test]
async fn temporal_diff_async_works() {
    let store = make_store();
    store
        .insert_fact(&make_temporal_fact(
            "t1",
            "agent",
            "diff async",
            "2026-02-01",
            "9999-12-31",
        ))
        .expect("insert");
    let diff = store
        .query_facts_diff_async(
            "agent".to_owned(),
            "2026-01-01".to_owned(),
            "2026-03-01".to_owned(),
        )
        .await
        .expect("async diff");
    assert_eq!(diff.added.len(), 1);
}

#[tokio::test]
async fn search_temporal_async_works() {
    let store = make_store();
    let fact = make_fact("fst1", "agent-a", "temporal search target");
    store.insert_fact(&fact).expect("insert fact");
    let emb = make_embedding("est1", "temporal search target", "fst1", "agent-a");
    store.insert_embedding(&emb).expect("insert embedding");
    let q = HybridQuery {
        text: "temporal".to_owned(),
        embedding: vec![0.5, 0.5, 0.5, 0.5],
        seed_entities: vec![],
        limit: 10,
        ef: 16,
    };
    let results = store
        .search_temporal_async(q, "2026-06-01".to_owned())
        .await
        .expect("async temporal search");
    assert!(!results.is_empty());
}
