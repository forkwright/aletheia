//! Tests for basic fact CRUD: insert, retrieve, forget, access.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]

use super::super::super::*;
use crate::knowledge::{EpistemicTier, Fact, ForgetReason};
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
fn query_timeout_returns_typed_error() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem");

    // Recursive transitive closure on a linear chain of N nodes requires N-1 semi-naive
    // fixpoint epochs. Each epoch checks the Poison flag. With N=2000 and timeout=50ms
    // the engine will hit the Poison kill well before all epochs complete.
    let result = store.run_query_with_timeout(
        r"
edge[a, b] := a in int_range(2000), b = a + 1
reach[a, b] := edge[a, b]
reach[a, c] := reach[a, b], edge[b, c]
?[a, c] := reach[a, c]
",
        BTreeMap::new(),
        Some(std::time::Duration::from_millis(50)),
    );

    assert!(result.is_err(), "expected timeout error");
    let err = result.expect_err("timeout query must fail");
    let msg = err.to_string();
    assert!(
        msg.contains("timed out"),
        "error should mention timeout, got: {msg}"
    );
    assert!(
        matches!(err, crate::error::Error::QueryTimeout { .. }),
        "error type should be QueryTimeout"
    );
}

#[test]
fn query_without_timeout_succeeds() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem");

    let result = store.run_query_with_timeout("?[x] := x = 42", BTreeMap::new(), None);

    assert!(result.is_ok(), "query without timeout should succeed");
    let rows = result.expect("query without timeout must succeed");
    assert_eq!(rows.rows.len(), 1, "simple query should return one row");
}

#[test]
fn insert_fact_and_retrieve() {
    let store = make_store();
    let fact = make_fact("f1", "agent-a", "Rust is a systems programming language");
    store.insert_fact(&fact).expect("insert fact");

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query facts");
    assert_eq!(
        results.len(),
        1,
        "should retrieve exactly one inserted fact"
    );
    assert_eq!(
        results[0].id.as_str(),
        "f1",
        "retrieved fact should have expected id"
    );
    assert_eq!(
        results[0].content, "Rust is a systems programming language",
        "retrieved fact should have expected content"
    );
    assert!(
        (results[0].confidence - 0.9).abs() < f64::EPSILON,
        "retrieved fact confidence should match inserted value"
    );
}

#[test]
fn insert_multiple_facts_and_retrieve() {
    let store = make_store();
    for i in 0..5 {
        let fact = make_fact(&format!("f{i}"), "agent-a", &format!("Fact number {i}"));
        store.insert_fact(&fact).expect("insert fact");
    }

    let results = store
        .query_facts("agent-a", "2026-06-01", 100)
        .expect("query facts");
    assert_eq!(results.len(), 5, "should retrieve all five inserted facts");
}

#[test]
fn upsert_fact_overwrites() {
    let store = make_store();
    let mut fact = make_fact("f1", "agent-a", "Original content");
    store.insert_fact(&fact).expect("insert fact");

    fact.content = "Updated content".to_owned();
    fact.confidence = 0.95;
    store.insert_fact(&fact).expect("upsert fact");

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query facts");
    assert_eq!(
        results.len(),
        1,
        "upsert should not create a duplicate fact"
    );
    assert_eq!(
        results[0].content, "Updated content",
        "upsert should overwrite content"
    );
    assert!(
        (results[0].confidence - 0.95).abs() < f64::EPSILON,
        "upsert should overwrite confidence"
    );
}

#[test]
fn forget_fact_excludes_from_query() {
    let store = make_store();
    let fact = make_fact("f1", "agent-a", "Secret fact");
    store.insert_fact(&fact).expect("insert fact");

    let forgotten = store
        .forget_fact(
            &crate::id::FactId::new_unchecked("f1"),
            ForgetReason::UserRequested,
        )
        .expect("forget fact");
    assert!(
        forgotten.is_forgotten,
        "returned fact should be marked as forgotten"
    );
    assert_eq!(
        forgotten.forget_reason,
        Some(ForgetReason::UserRequested),
        "forget reason should be preserved"
    );
    assert!(
        forgotten.forgotten_at.is_some(),
        "forgotten_at timestamp should be set"
    );

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query facts");
    assert!(
        results.is_empty(),
        "forgotten fact must not appear in recall"
    );
}

#[test]
fn forget_fact_then_unforget_restores_recall() {
    let store = make_store();
    let fact = make_fact("f1", "agent-a", "Recoverable fact");
    store.insert_fact(&fact).expect("insert fact");

    store
        .forget_fact(
            &crate::id::FactId::new_unchecked("f1"),
            ForgetReason::Outdated,
        )
        .expect("forget");

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query");
    assert!(results.is_empty(), "forgotten fact excluded from recall");

    let restored = store
        .unforget_fact(&crate::id::FactId::new_unchecked("f1"))
        .expect("unforget");
    assert!(
        !restored.is_forgotten,
        "unforgotten fact should not be marked as forgotten"
    );
    assert!(
        restored.forgotten_at.is_none(),
        "unforgotten fact should have no forgotten_at timestamp"
    );
    assert!(
        restored.forget_reason.is_none(),
        "unforgotten fact should have no forget reason"
    );

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query after unforget");
    assert_eq!(results.len(), 1, "unforget must restore recall visibility");
    assert_eq!(
        results[0].id.as_str(),
        "f1",
        "the restored fact should have the original id"
    );
}

#[test]
fn forget_preserves_in_audit() {
    let store = make_store();
    let fact = make_fact("f1", "agent-a", "Auditable fact");
    store.insert_fact(&fact).expect("insert fact");

    store
        .forget_fact(
            &crate::id::FactId::new_unchecked("f1"),
            ForgetReason::Privacy,
        )
        .expect("forget");

    let all = store.audit_all_facts("agent-a", 100).expect("audit all");
    let found = all.iter().find(|f| f.id.as_str() == "f1");
    assert!(found.is_some(), "audit must return forgotten facts");
    let found = found.expect("f1 must appear in audit after forget");
    assert!(
        found.is_forgotten,
        "audited fact should be marked as forgotten"
    );
    assert_eq!(
        found.forget_reason,
        Some(ForgetReason::Privacy),
        "audit should preserve forget reason"
    );
}

#[test]
fn forget_reason_roundtrips() {
    let store = make_store();

    let reasons = [
        ("f-ur", ForgetReason::UserRequested),
        ("f-od", ForgetReason::Outdated),
        ("f-ic", ForgetReason::Incorrect),
        ("f-pr", ForgetReason::Privacy),
    ];

    for (id, reason) in reasons {
        let fact = make_fact(id, "agent-a", &format!("fact for {reason}"));
        store.insert_fact(&fact).expect("insert");

        let forgotten = store
            .forget_fact(&crate::id::FactId::new_unchecked(id), reason)
            .expect("forget");
        assert_eq!(
            forgotten.forget_reason,
            Some(reason),
            "reason must round-trip for {reason}"
        );
    }

    let forgotten_list = store
        .list_forgotten("agent-a", 100)
        .expect("list_forgotten");
    assert_eq!(
        forgotten_list.len(),
        reasons.len(),
        "list_forgotten should return all forgotten facts"
    );
    for (id, reason) in reasons {
        let found = forgotten_list
            .iter()
            .find(|f| f.id.as_str() == id)
            .unwrap_or_else(|| panic!("missing {id} in list_forgotten"));
        assert_eq!(
            found.forget_reason,
            Some(reason),
            "forget reason should round-trip for {reason}"
        );
    }
}

#[test]
fn forget_nonexistent_fact_errors() {
    let store = make_store();
    let result = store.forget_fact(
        &crate::id::FactId::new_unchecked("nonexistent"),
        ForgetReason::UserRequested,
    );
    assert!(result.is_err(), "forgetting non-existent fact must error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not found"),
        "error should mention not found: {err}"
    );
}

#[test]
fn forget_excluded_from_temporal_diff() {
    let store = make_store();
    let fact = make_fact("f-diff", "agent-a", "Temporal diff fact");
    store.insert_fact(&fact).expect("insert");

    store
        .forget_fact(
            &crate::id::FactId::new_unchecked("f-diff"),
            ForgetReason::Incorrect,
        )
        .expect("forget");

    let diff = store
        .query_facts_diff("agent-a", "2025-01-01", "2027-01-01")
        .expect("diff");
    assert!(
        !diff.added.iter().any(|f| f.id.as_str() == "f-diff"),
        "forgotten fact must not appear in temporal diff added"
    );
}

#[test]
fn increment_access_updates_count() {
    let store = make_store();
    let fact = make_fact("f1", "agent-a", "Accessed fact");
    store.insert_fact(&fact).expect("insert fact");

    store
        .increment_access(&[crate::id::FactId::new_unchecked("f1")])
        .expect("increment");
    store
        .increment_access(&[crate::id::FactId::new_unchecked("f1")])
        .expect("increment again");

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query");
    let found = results
        .iter()
        .find(|f| f.id.as_str() == "f1")
        .expect("found");
    assert_eq!(
        found.access_count, 2,
        "access count should reflect two increments"
    );
}

#[test]
fn increment_access_empty_ids_is_noop() {
    let store = make_store();
    store
        .increment_access(&[])
        .expect("empty increment should succeed");
}

#[test]
fn increment_access_nonexistent_id_is_silent() {
    let store = make_store();
    store
        .increment_access(&[crate::id::FactId::new_unchecked("nonexistent")])
        .expect("increment nonexistent should not error");
}

#[test]
fn insert_fact_empty_content_rejected() {
    let store = make_store();
    let fact = make_fact("f-empty", "agent-a", "");
    let result = store.insert_fact(&fact);
    assert!(result.is_err(), "empty content must be rejected");
    assert!(
        matches!(
            result.expect_err("empty content must fail"),
            crate::error::Error::EmptyContent { .. }
        ),
        "error type should be EmptyContent"
    );
}

#[test]
fn insert_fact_confidence_out_of_range_rejected() {
    let store = make_store();

    let mut high = make_fact("f-high", "agent-a", "High confidence");
    high.confidence = 1.5;
    let result = store.insert_fact(&high);
    assert!(result.is_err(), "confidence > 1.0 must be rejected");
    assert!(
        matches!(
            result.expect_err("confidence > 1.0 must fail"),
            crate::error::Error::InvalidConfidence { .. }
        ),
        "error type should be InvalidConfidence for confidence > 1.0"
    );

    let mut negative = make_fact("f-neg", "agent-a", "Negative confidence");
    negative.confidence = -0.5;
    let result = store.insert_fact(&negative);
    assert!(result.is_err(), "confidence < 0.0 must be rejected");
    assert!(
        matches!(
            result.expect_err("confidence < 0.0 must fail"),
            crate::error::Error::InvalidConfidence { .. }
        ),
        "error type should be InvalidConfidence for confidence < 0.0"
    );
}

#[test]
fn schema_version_returns_current() {
    let store = make_store();
    let version = store.schema_version().expect("schema version");
    assert_eq!(
        version,
        KnowledgeStore::SCHEMA_VERSION,
        "schema version should match current constant"
    );
}

#[test]
fn query_facts_filters_by_nous_id() {
    let store = make_store();
    store
        .insert_fact(&make_fact("f1", "agent-a", "Fact for A"))
        .expect("insert f1");
    store
        .insert_fact(&make_fact("f2", "agent-b", "Fact for B"))
        .expect("insert f2");
    store
        .insert_fact(&make_fact("f3", "agent-a", "Another fact for A"))
        .expect("insert f3");

    let results_a = store
        .query_facts("agent-a", "2026-06-01", 100)
        .expect("query agent-a");
    assert_eq!(results_a.len(), 2, "agent-a should have exactly two facts");
    assert!(
        results_a.iter().all(|f| f.nous_id == "agent-a"),
        "all agent-a results should have correct nous_id"
    );

    let results_b = store
        .query_facts("agent-b", "2026-06-01", 100)
        .expect("query agent-b");
    assert_eq!(results_b.len(), 1, "agent-b should have exactly one fact");
    assert_eq!(
        results_b[0].id.as_str(),
        "f2",
        "agent-b's fact should have id f2"
    );
}

#[test]
fn query_facts_respects_limit() {
    let store = make_store();
    for i in 0..20 {
        store
            .insert_fact(&make_fact(
                &format!("f{i}"),
                "agent-a",
                &format!("Fact {i}"),
            ))
            .expect("insert");
    }

    let results = store
        .query_facts("agent-a", "2026-06-01", 5)
        .expect("query with limit");
    assert_eq!(
        results.len(),
        5,
        "query with limit 5 should return exactly 5 results"
    );
}
