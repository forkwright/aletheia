//! Tests for fact query behavior: expiry, forgotten state, confidence, bulk ops.
//! Tests for fact query filtering, confidence, concurrent ops, and cross-agent listing.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use super::super::super::*;
use crate::knowledge::{EpistemicTier, Fact};
use std::collections::BTreeMap;
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

#[test]
fn query_facts_excludes_expired() {
    let store = make_store();

    store
        .insert_fact(&make_fact("f-active", "agent-a", "Active fact"))
        .expect("insert active");

    let mut expired = make_fact("f-expired", "agent-a", "Expired fact");
    expired.valid_to =
        crate::knowledge::parse_timestamp("2025-01-01").expect("valid expiry timestamp");
    store.insert_fact(&expired).expect("insert expired");

    let results = store
        .query_facts("agent-a", "2026-06-01", 100)
        .expect("query");

    assert_eq!(results.len(), 1, "only the active fact should be returned");
    assert_eq!(
        results[0].id.as_str(),
        "f-active",
        "the returned fact should be the active one"
    );
}

#[test]
fn query_facts_empty_store_returns_empty() {
    let store = make_store();
    let results = store
        .query_facts("agent-a", "2026-06-01", 100)
        .expect("query empty store");
    assert!(results.is_empty(), "empty store should return no facts");
}

#[test]
fn query_facts_nonexistent_nous_id_returns_empty() {
    let store = make_store();
    store
        .insert_fact(&make_fact("f1", "agent-a", "Some fact"))
        .expect("insert");

    let results = store
        .query_facts("nonexistent-agent", "2026-06-01", 100)
        .expect("query nonexistent nous");
    assert!(
        results.is_empty(),
        "querying for unknown nous id should return empty results"
    );
}

#[test]
fn query_facts_at_returns_snapshot() {
    let store = make_store();

    let mut fact = make_fact("f1", "agent-a", "Temporal fact");
    fact.valid_from = crate::knowledge::parse_timestamp("2026-01-01")
        .expect("valid_from timestamp for temporal test");
    fact.valid_to = crate::knowledge::parse_timestamp("2026-06-01")
        .expect("valid_to timestamp for temporal test");
    store.insert_fact(&fact).expect("insert temporal fact");

    let results = store
        .query_facts_at("2026-03-15")
        .expect("query at mid-range");
    assert_eq!(
        results.len(),
        1,
        "fact should be visible at timestamp within its validity window"
    );
    assert_eq!(
        results[0].id.as_str(),
        "f1",
        "the visible fact should have id f1"
    );

    let results = store
        .query_facts_at("2026-07-01")
        .expect("query at post-range");
    assert!(
        results.is_empty(),
        "fact should not be visible after its validity window ends"
    );
}

#[test]
fn backup_db_returns_error_for_mem_backend() {
    let store = make_store();
    let dir = tempfile::tempdir().expect("create temp dir");
    let backup_path = dir.path().join("backup.db");
    let result = store.backup_db(&backup_path);
    assert!(
        result.is_err(),
        "backup_db should error on in-memory backend"
    );
}

#[test]
fn restore_backup_returns_error_for_mem_backend() {
    let store = make_store();
    let dir = tempfile::tempdir().expect("create temp dir");
    let backup_path = dir.path().join("backup.db");
    std::fs::write(&backup_path, "fake").expect("write fake backup file");
    let result = store.restore_backup(&backup_path);
    assert!(
        result.is_err(),
        "restore_backup should error on in-memory backend"
    );
}

#[test]
fn import_from_backup_returns_error_for_mem_backend() {
    let store = make_store();
    let dir = tempfile::tempdir().expect("create temp dir");
    let backup_path = dir.path().join("backup.db");
    std::fs::write(&backup_path, "fake").expect("write fake backup file");
    let result = store.import_from_backup(&backup_path, &["facts".to_owned()]);
    assert!(
        result.is_err(),
        "import_from_backup should error on in-memory backend"
    );
}

#[test]
fn query_result_does_not_expose_named_rows_type() {
    let store = make_store();
    let result: QueryResult = store
        .run_query("?[x] := x = 99", BTreeMap::new())
        .expect("simple query");
    assert_eq!(result.rows.len(), 1, "one result row expected");
    assert!(
        !result.headers.is_empty(),
        "headers should be populated in query result"
    );
}

#[test]
fn query_result_from_run_script_read_only() {
    let store = make_store();
    let result: QueryResult = store
        .run_script_read_only("?[x] := x = 42", BTreeMap::new())
        .expect("read-only query should succeed");
    assert_eq!(
        result.rows.len(),
        1,
        "read-only query should return one row"
    );
}
