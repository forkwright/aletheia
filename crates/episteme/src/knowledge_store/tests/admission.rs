//! Tests for wired `StructuredAdmissionPolicy` and atomicity guarantees.
//!
//! Validates:
//! - With `Structured` configured, a fact failing the policy is rejected.
//! - With `Default` configured, admit-all holds for the same fact.
//! - Concurrent insertion of the same fact is admitted exactly once.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::sync::Arc;

use crate::admission::{
    DefaultAdmissionPolicy, StructuredAdmissionConfig, StructuredAdmissionPolicy,
};
use crate::knowledge_store::{KnowledgeConfig, KnowledgeStore};
use crate::test_fixtures::{DIM, make_fact};

fn make_structured_store(threshold: f64) -> Arc<KnowledgeStore> {
    let config = StructuredAdmissionConfig {
        threshold,
        ..Default::default()
    };
    KnowledgeStore::open_mem_with_config(KnowledgeConfig {
        dim: DIM,
        admission_policy: Box::new(StructuredAdmissionPolicy::new(config)),
        ..Default::default()
    })
    .expect("open structured-policy store")
}

fn make_default_store() -> Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig {
        dim: DIM,
        admission_policy: Box::new(DefaultAdmissionPolicy),
        ..Default::default()
    })
    .expect("open default-policy store")
}

// ── Capability 1a: Structured policy rejects low-quality facts ─────────────────

#[test]
fn structured_policy_rejects_fact_below_threshold() {
    // Very strict threshold: combined score needs to be 0.9+ to pass.
    let store = make_structured_store(0.9);

    // Short content + low confidence + ephemeral type → well below 0.9 threshold.
    let mut fact = make_fact("adm-reject", "alice", "x");
    fact.provenance.confidence = 0.15;
    fact.fact_type = "ephemeral".to_owned();

    let result = store.insert_fact(&fact);
    assert!(
        result.is_err(),
        "structured policy with strict threshold should reject low-quality fact"
    );
    assert!(
        matches!(
            result.expect_err("must fail"),
            crate::error::Error::AdmissionRejected { .. }
        ),
        "error variant should be AdmissionRejected"
    );

    let stored = store
        .query_facts("alice", "2026-06-01", 100)
        .expect("query after rejection");
    assert!(
        stored.is_empty(),
        "rejected fact must not appear in the store"
    );
}

// ── Capability 1b: Default policy admits the same fact ─────────────────────────

#[test]
fn default_policy_admits_same_low_quality_fact() {
    let store = make_default_store();

    // Same fact that the structured policy rejects.
    let mut fact = make_fact("adm-admit", "alice", "x");
    fact.provenance.confidence = 0.15;
    fact.fact_type = "ephemeral".to_owned();

    // Default policy bypasses all scoring — non-empty content passes.
    let result = store.insert_fact(&fact);
    assert!(
        result.is_ok(),
        "default (admit-all) policy should accept the fact: {result:?}"
    );

    let stored = store
        .query_facts("alice", "2026-06-01", 100)
        .expect("query after admission");
    assert_eq!(
        stored.len(),
        1,
        "admitted fact must be present in the store"
    );
}

// ── Capability 1c: Default policy allows high-quality fact through structured ──

#[test]
fn structured_policy_admits_high_quality_fact() {
    let store = make_structured_store(0.3);

    let mut fact = make_fact(
        "adm-hq",
        "alice",
        "Alice consistently prefers Rust for performance-critical systems work",
    );
    fact.provenance.confidence = 0.9;
    fact.fact_type = "preference".to_owned();

    store
        .insert_fact(&fact)
        .expect("structured policy should admit high-quality fact");

    let stored = store
        .query_facts("alice", "2026-06-01", 100)
        .expect("query after admission");
    assert_eq!(stored.len(), 1, "high-quality fact must be present");
}

// ── Capability 1d: Atomicity — concurrent insert of same fact admits once ──────

#[test]
fn concurrent_insert_same_fact_admits_exactly_once() {
    // WHY (#5673): the sharded insert lock prevents admission-then-write races
    // within the fact's nous shard. Both threads race to insert the same fact;
    // after both complete, the store must still contain exactly one row.
    let store = make_default_store();
    let store2 = Arc::clone(&store);

    let mut fact = make_fact("adm-concurrent", "alice", "Alice prefers dark mode");
    fact.provenance.confidence = 0.8;

    let fact2 = fact.clone();

    let h1 = std::thread::spawn(move || store.insert_fact(&fact));
    let h2 = std::thread::spawn(move || store2.insert_fact(&fact2));

    let r1 = h1.join().expect("thread 1 panicked");
    let r2 = h2.join().expect("thread 2 panicked");

    // At least one must succeed (the other may also succeed via upsert).
    assert!(
        r1.is_ok() || r2.is_ok(),
        "at least one concurrent insert must succeed"
    );

    // WHY: the Datalog `:put` upsert is idempotent on the (id, valid_from)
    // primary key, so concurrent inserts store exactly one row; no panic plus
    // at least one Ok is the verifiable invariant here.
    let _ = (r1, r2); // both joins completed without panic
}

// ── Atomicity variant: verify with a store we can still query ──────────────────

#[test]
fn concurrent_insert_same_fact_store_contains_one_row() {
    let store = Arc::new(
        KnowledgeStore::open_mem_with_config(KnowledgeConfig {
            dim: DIM,
            admission_policy: Box::new(DefaultAdmissionPolicy),
            ..Default::default()
        })
        .expect("open store"),
    );

    let mut fact = make_fact(
        "adm-conc-row",
        "alice",
        "Alice prefers dark mode in all editors",
    );
    fact.provenance.confidence = 0.8;

    let fact2 = fact.clone();
    let s1 = Arc::clone(&store);
    let s2 = Arc::clone(&store);

    let h1 = std::thread::spawn(move || s1.insert_fact(&fact));
    let h2 = std::thread::spawn(move || s2.insert_fact(&fact2));

    let _ = h1.join().expect("thread 1");
    let _ = h2.join().expect("thread 2");

    let stored = store
        .query_facts("alice", "2026-06-01", 100)
        .expect("query concurrent result");

    let count = stored.len();
    assert_eq!(
        count, 1,
        "concurrent upsert of same fact must result in exactly one row; got {count}"
    );
}

#[test]
fn insert_lock_sharding_distinguishes_different_nous_ids() {
    let alice_shard = KnowledgeStore::insert_lock_shard_for_test("alice");
    let peer_shard = (0..128)
        .map(|idx| format!("peer-{idx}"))
        .map(|nous_id| KnowledgeStore::insert_lock_shard_for_test(&nous_id))
        .find(|shard| *shard != alice_shard)
        .expect("fixture must find a different insert-lock shard");

    assert_ne!(
        alice_shard, peer_shard,
        "different nous IDs must be able to map to different insert-lock shards"
    );
}
