//! Tests for script execution and cross-agent fact listing.
//! Tests for fact query filtering, confidence, concurrent ops, and cross-agent listing.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

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

fn make_fact(id: &str, nous_id: &str, content: &str) -> crate::knowledge::Fact {
    crate::knowledge::Fact {
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
fn run_script_read_only_basic() {
    let store = make_store();
    let result = store
        .run_script_read_only("?[x] := x = 42", BTreeMap::new())
        .expect("read-only query should succeed");
    assert_eq!(
        result.rows.len(),
        1,
        "basic read-only query should return one row"
    );
}

#[test]
fn run_script_read_only_rejects_mutations() {
    let store = make_store();
    let result = store.run_script_read_only(r"?[x] <- [[1]] :put facts { id: 'x', valid_from: 'now' => content: 'test', nous_id: 'a', confidence: 1.0, tier: 'verified', valid_to: 'end', recorded_at: 'now', access_count: 0, last_accessed_at: '', stability_hours: 720.0, fact_type: '' }", BTreeMap::new());
    assert!(
        result.is_err(),
        "read-only mode should reject :put operations"
    );
}

#[test]
fn audit_all_facts_returns_forgotten() {
    let store = make_store();
    let f1 = make_fact("f1", "agent-a", "visible fact");
    let f2 = make_fact("f2", "agent-a", "forgotten fact");
    store.insert_fact(&f1).expect("insert f1");
    store.insert_fact(&f2).expect("insert f2");
    store
        .forget_fact(
            &crate::id::FactId::new_unchecked("f2"),
            ForgetReason::UserRequested,
        )
        .expect("forget f2");
    let all = store.audit_all_facts("agent-a", 100).expect("audit");
    assert_eq!(
        all.len(),
        2,
        "audit should return both visible and forgotten facts"
    );
    let forgotten_count = all.iter().filter(|f| f.is_forgotten).count();
    assert_eq!(
        forgotten_count, 1,
        "audit should show exactly one forgotten fact"
    );
}

#[test]
fn audit_all_facts_empty_store() {
    let store = make_store();
    let all = store.audit_all_facts("agent-a", 100).expect("audit empty");
    assert!(
        all.is_empty(),
        "audit of empty store should return no facts"
    );
}

#[test]
fn forget_already_forgotten_is_idempotent() {
    let store = make_store();
    let f1 = make_fact("f1", "agent-a", "will be forgotten twice");
    store.insert_fact(&f1).expect("insert f1");
    store
        .forget_fact(
            &crate::id::FactId::new_unchecked("f1"),
            ForgetReason::Outdated,
        )
        .expect("first forget");
    store
        .forget_fact(
            &crate::id::FactId::new_unchecked("f1"),
            ForgetReason::Outdated,
        )
        .expect("second forget should not panic");
    let all = store.audit_all_facts("agent-a", 100).expect("audit");
    assert_eq!(
        all.len(),
        1,
        "audit should return the one fact that was forgotten twice"
    );
    assert!(
        all[0].is_forgotten,
        "fact forgotten twice should still be marked as forgotten"
    );
}

#[test]
fn unforget_never_forgotten_is_noop() {
    let store = make_store();
    let f1 = make_fact("f1", "agent-a", "never forgotten");
    store.insert_fact(&f1).expect("insert f1");
    store
        .unforget_fact(&crate::id::FactId::new_unchecked("f1"))
        .expect("unforget should succeed");
    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query");
    assert_eq!(
        results.len(),
        1,
        "unforgetting a never-forgotten fact should keep it visible"
    );
    assert_eq!(
        results[0].content, "never forgotten",
        "content should be unchanged after noop unforget"
    );
}

#[test]
fn forget_nonexistent_fact_is_err() {
    let store = make_store();
    let result = store.forget_fact(
        &crate::id::FactId::new_unchecked("nonexistent"),
        ForgetReason::UserRequested,
    );
    assert!(
        result.is_err(),
        "forgetting a nonexistent fact should return an error"
    );
}

#[test]
fn forget_with_all_reasons() {
    let store = make_store();
    let reasons = [
        ("f1", ForgetReason::UserRequested),
        ("f2", ForgetReason::Outdated),
        ("f3", ForgetReason::Incorrect),
        ("f4", ForgetReason::Privacy),
    ];
    for (id, _) in &reasons {
        let fact = make_fact(id, "agent-a", &format!("fact {id}"));
        store.insert_fact(&fact).expect("insert");
    }
    for (id, reason) in &reasons {
        store
            .forget_fact(&crate::id::FactId::new_unchecked(*id), *reason)
            .expect("forget");
    }
    let all = store.audit_all_facts("agent-a", 100).expect("audit");
    assert_eq!(all.len(), 4, "audit should return all four forgotten facts");
    for fact in &all {
        assert!(fact.is_forgotten, "each fact should be marked as forgotten");
        assert!(
            fact.forget_reason.is_some(),
            "each forgotten fact should have a reason"
        );
    }
    let reasons: Vec<ForgetReason> = all.iter().filter_map(|f| f.forget_reason).collect();
    assert!(
        reasons.contains(&ForgetReason::UserRequested),
        "UserRequested reason should be present"
    );
    assert!(
        reasons.contains(&ForgetReason::Outdated),
        "Outdated reason should be present"
    );
    assert!(
        reasons.contains(&ForgetReason::Incorrect),
        "Incorrect reason should be present"
    );
    assert!(
        reasons.contains(&ForgetReason::Privacy),
        "Privacy reason should be present"
    );
}

#[test]
fn insert_fact_unicode_content() {
    let store = make_store();
    let mut fact = make_fact("fu", "agent-a", "placeholder");
    fact.content = "日本語のファクト 🦀".to_owned();
    store.insert_fact(&fact).expect("insert unicode fact");
    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query");
    assert_eq!(results.len(), 1, "unicode fact should be retrievable");
    assert_eq!(
        results[0].content, "日本語のファクト 🦀",
        "unicode content should round-trip correctly"
    );
}

#[test]
fn insert_fact_very_long_content() {
    let store = make_store();
    let long_content = "x".repeat(10240);
    let mut fact = make_fact("fl", "agent-a", "placeholder");
    fact.content = long_content.clone();
    store.insert_fact(&fact).expect("insert long fact");
    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query");
    assert_eq!(results.len(), 1, "very long fact should be retrievable");
    assert_eq!(
        results[0].content.len(),
        10240,
        "very long content should be stored without truncation"
    );
}

#[test]
fn run_query_malformed_datalog_errors() {
    let store = make_store();
    let result = store.run_query("this is not valid datalog!!!", BTreeMap::new());
    assert!(result.is_err(), "malformed datalog should error");
}

#[test]
fn insert_fact_confidence_zero() {
    let store = make_store();
    let mut fact = make_fact("fc0", "agent-a", "zero confidence");
    fact.confidence = 0.0;
    store.insert_fact(&fact).expect("insert zero confidence");
    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query");
    let found = results
        .iter()
        .find(|f| f.id.as_str() == "fc0")
        .expect("find fact");
    assert!(
        (found.confidence - 0.0).abs() < f64::EPSILON,
        "zero confidence should be stored and retrieved correctly"
    );
}

#[test]
fn insert_fact_confidence_one() {
    let store = make_store();
    let mut fact = make_fact("fc1", "agent-a", "full confidence");
    fact.confidence = 1.0;
    store.insert_fact(&fact).expect("insert full confidence");
    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query");
    let found = results
        .iter()
        .find(|f| f.id.as_str() == "fc1")
        .expect("find fact");
    assert!(
        (found.confidence - 1.0).abs() < f64::EPSILON,
        "full confidence should be stored and retrieved correctly"
    );
}

#[test]
fn query_facts_limit_zero() {
    let store = make_store();
    store
        .insert_fact(&make_fact("f1", "agent-a", "fact one"))
        .expect("insert");
    let results = store
        .query_facts("agent-a", "2026-06-01", 0)
        .expect("query with limit 0");
    assert!(results.is_empty(), "limit 0 should return no facts");
}

#[test]
fn query_facts_large_limit() {
    let store = make_store();
    store
        .insert_fact(&make_fact("f1", "agent-a", "one"))
        .expect("insert f1");
    store
        .insert_fact(&make_fact("f2", "agent-a", "two"))
        .expect("insert f2");
    store
        .insert_fact(&make_fact("f3", "agent-a", "three"))
        .expect("insert f3");
    let results = store
        .query_facts("agent-a", "2026-06-01", 1000)
        .expect("query large limit");
    assert_eq!(
        results.len(),
        3,
        "large limit should return all three facts"
    );
}

#[test]
fn retrieve_nonexistent_fact() {
    let store = make_store();
    let results = store
        .query_facts("nonexistent-agent", "2026-06-01", 10)
        .expect("query should succeed, returning empty");
    assert!(
        results.is_empty(),
        "querying nonexistent agent should return empty results"
    );
}

#[test]
fn forget_nonexistent_fact_returns_error() {
    let store = make_store();
    let result = store.forget_fact(
        &crate::id::FactId::new_unchecked("nonexistent"),
        ForgetReason::UserRequested,
    );
    assert!(result.is_err(), "forgetting a non-existent fact must error");
}

#[test]
fn concurrent_inserts() {
    let store = make_store();
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let s = Arc::clone(&store);
            std::thread::spawn(move || {
                let fact = Fact {
                    id: crate::id::FactId::new_unchecked(format!("f-concurrent-{i}")),
                    nous_id: "agent-a".to_owned(),
                    content: format!("Concurrent fact {i}"),
                    confidence: 0.9,
                    tier: EpistemicTier::Inferred,
                    valid_from: crate::knowledge::parse_timestamp("2026-01-01")
                        .expect("valid test timestamp"),
                    valid_to: crate::knowledge::far_future(),
                    superseded_by: None,
                    source_session_id: None,
                    recorded_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                        .expect("valid test timestamp"),
                    access_count: 0,
                    last_accessed_at: None,
                    stability_hours: 720.0,
                    fact_type: String::new(),
                    is_forgotten: false,
                    forgotten_at: None,
                    forget_reason: None,
                };
                s.insert_fact(&fact).expect("concurrent insert");
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread join");
    }

    let results = store
        .query_facts("agent-a", "2026-06-01", 100)
        .expect("query after concurrent inserts");
    assert_eq!(
        results.len(),
        10,
        "all ten concurrently inserted facts should be retrievable"
    );
}

#[test]
fn run_query_returns_results() {
    let store = make_store();
    let rows = store
        .run_query("?[x] := x = 42", std::collections::BTreeMap::new())
        .expect("run_query");
    assert_eq!(
        rows.rows.len(),
        1,
        "run_query should return one row for x = 42"
    );
}

#[test]
fn run_mut_query_creates_and_reads() {
    let store = make_store();
    store
        .insert_fact(&make_fact("f1", "agent-a", "Mutable test"))
        .expect("insert");

    let mut params = std::collections::BTreeMap::new();
    params.insert("id".to_owned(), crate::engine::DataValue::Str("f1".into()));
    store
        .run_mut_query(
            r"?[id, valid_from] := *facts{id, valid_from}, id = $id :rm facts {id, valid_from}",
            params,
        )
        .expect("delete via run_mut_query");

    let results = store
        .query_facts("agent-a", "2026-06-01", 10)
        .expect("query after delete");
    assert!(
        results.is_empty(),
        "fact deleted via run_mut_query should not be retrievable"
    );
}

#[test]
fn list_all_facts_returns_facts_across_agents() {
    let store = make_store();
    store
        .insert_fact(&make_fact("f1", "agent-a", "Fact from agent A"))
        .expect("insert a");
    store
        .insert_fact(&make_fact("f2", "agent-b", "Fact from agent B"))
        .expect("insert b");

    let all = store.list_all_facts(100).expect("list_all_facts");
    assert_eq!(all.len(), 2, "both agents' facts must be returned");
    let ids: Vec<&str> = all.iter().map(|f| f.id.as_str()).collect();
    assert!(
        ids.contains(&"f1"),
        "list_all_facts should include fact f1 from agent-a"
    );
    assert!(
        ids.contains(&"f2"),
        "list_all_facts should include fact f2 from agent-b"
    );
    assert_eq!(
        all.iter().find(|f| f.id.as_str() == "f1").unwrap().nous_id,
        "agent-a",
        "f1 should belong to agent-a"
    );
    assert_eq!(
        all.iter().find(|f| f.id.as_str() == "f2").unwrap().nous_id,
        "agent-b",
        "f2 should belong to agent-b"
    );
}

#[test]
fn list_all_facts_empty_store_returns_empty() {
    let store = make_store();
    let all = store.list_all_facts(100).expect("list_all_facts empty");
    assert!(
        all.is_empty(),
        "list_all_facts on empty store should return empty"
    );
}

#[tokio::test]
async fn insert_fact_async_works() {
    let store = make_store();
    let fact = make_fact("f-async", "agent-a", "Async inserted fact");
    store.insert_fact_async(fact).await.expect("async insert");

    let results = store
        .query_facts_async("agent-a".to_owned(), "2026-06-01".to_owned(), 10)
        .await
        .expect("async query");
    assert_eq!(results.len(), 1, "async insert should make fact queryable");
    assert_eq!(
        results[0].id.as_str(),
        "f-async",
        "async inserted fact should have expected id"
    );
}

#[tokio::test]
async fn query_facts_async_works() {
    let store = make_store();
    store
        .insert_fact(&make_fact("fa1", "agent-a", "async fact one"))
        .expect("insert");
    store
        .insert_fact(&make_fact("fa2", "agent-a", "async fact two"))
        .expect("insert");
    let results = store
        .query_facts_async("agent-a".to_owned(), "2026-06-01".to_owned(), 10)
        .await
        .expect("async query");
    assert_eq!(
        results.len(),
        2,
        "async query should return both async-inserted facts"
    );
}

#[tokio::test]
async fn audit_all_facts_async_works() {
    let store = make_store();
    store
        .insert_fact(&make_fact("faa1", "agent-a", "audit async one"))
        .expect("insert");
    store
        .insert_fact(&make_fact("faa2", "agent-a", "audit async two"))
        .expect("insert");
    store
        .forget_fact(
            &crate::id::FactId::new_unchecked("faa2"),
            ForgetReason::Incorrect,
        )
        .expect("forget");
    let all = store
        .audit_all_facts_async("agent-a".to_owned(), 100)
        .await
        .expect("async audit");
    assert_eq!(
        all.len(),
        2,
        "async audit should return both visible and forgotten facts"
    );
    let forgotten_count = all.iter().filter(|f| f.is_forgotten).count();
    assert_eq!(
        forgotten_count, 1,
        "async audit should show exactly one forgotten fact"
    );
}

#[tokio::test]
async fn forget_fact_async_works() {
    let store = make_store();
    let fact = make_fact("f-forget-async", "agent-a", "Async forget");
    store.insert_fact_async(fact).await.expect("insert");

    let forgotten = store
        .forget_fact_async(
            crate::id::FactId::new_unchecked("f-forget-async"),
            ForgetReason::Incorrect,
        )
        .await
        .expect("async forget");
    assert!(
        forgotten.is_forgotten,
        "async forget should mark fact as forgotten"
    );
    assert_eq!(
        forgotten.forget_reason,
        Some(ForgetReason::Incorrect),
        "async forget should set the correct reason"
    );

    let all = store
        .audit_all_facts_async("agent-a".to_owned(), 100)
        .await
        .expect("async audit");
    let found = all
        .iter()
        .find(|f| f.id.as_str() == "f-forget-async")
        .expect("found");
    assert!(
        found.is_forgotten,
        "async-forgotten fact should appear as forgotten in audit"
    );
}

#[tokio::test]
async fn unforget_fact_async_works() {
    let store = make_store();
    let fact = make_fact("f-unforget-async", "agent-a", "Async unforget");
    store.insert_fact_async(fact).await.expect("insert");

    store
        .forget_fact_async(
            crate::id::FactId::new_unchecked("f-unforget-async"),
            ForgetReason::Outdated,
        )
        .await
        .expect("forget");
    store
        .unforget_fact_async(crate::id::FactId::new_unchecked("f-unforget-async"))
        .await
        .expect("unforget");

    let all = store
        .audit_all_facts_async("agent-a".to_owned(), 100)
        .await
        .expect("audit");
    let found = all
        .iter()
        .find(|f| f.id.as_str() == "f-unforget-async")
        .expect("found");
    assert!(
        !found.is_forgotten,
        "async-unforgotten fact should not be marked as forgotten"
    );
}

#[tokio::test]
async fn increment_access_async_works() {
    let store = make_store();
    let fact = make_fact("f-access-async", "agent-a", "Async access");
    store.insert_fact_async(fact).await.expect("insert");

    store
        .increment_access_async(vec![crate::id::FactId::new_unchecked("f-access-async")])
        .await
        .expect("async increment");

    let results = store
        .query_facts_async("agent-a".to_owned(), "2026-06-01".to_owned(), 10)
        .await
        .expect("query");
    let found = results
        .iter()
        .find(|f| f.id.as_str() == "f-access-async")
        .expect("found");
    assert_eq!(
        found.access_count, 1,
        "async increment should update access count to 1"
    );
}
