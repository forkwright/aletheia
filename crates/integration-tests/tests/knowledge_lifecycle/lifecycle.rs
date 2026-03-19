//! Full lifecycle, correction, retraction, and audit integration tests.
use super::*;

fn full_knowledge_lifecycle() {
    let store = open_store();
    let nous = "test-agent";
    let query_time = "2026-07-01T00:00:00Z";

    // 1. Insert original fact
    let original = make_fact(
        "f-1",
        nous,
        "Cody's favorite language is Rust",
        0.9,
        EpistemicTier::Inferred,
    );
    store.insert_fact(&original).expect("insert original");

    // Verify searchable
    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after insert");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "Cody's favorite language is Rust");

    // 2. Correct the fact
    correct_fact(
        &store,
        "f-1",
        "f-2",
        "Cody's favorite languages are Rust and TypeScript",
        nous,
        "2026-06-01T00:00:00Z",
    );

    // 3. Verify correction: only new fact visible
    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after correct");
    assert_eq!(results.len(), 1, "only corrected fact should be visible");
    assert_eq!(results[0].id, "f-2");
    assert_eq!(
        results[0].content,
        "Cody's favorite languages are Rust and TypeScript"
    );

    // Audit: both facts visible with supersession metadata
    let audit = audit_all_facts(&store, nous);
    assert_eq!(
        audit.len(),
        2,
        "audit should show both original and corrected"
    );

    let old = audit
        .iter()
        .find(|r| r.id == "f-1")
        .expect("original in audit");
    assert_eq!(
        old.valid_to, "2026-06-01T00:00:00Z",
        "old fact should be expired"
    );
    assert_eq!(
        old.superseded_by.as_deref(),
        Some("f-2"),
        "old fact should point to new"
    );

    let new = audit
        .iter()
        .find(|r| r.id == "f-2")
        .expect("corrected in audit");
    assert_eq!(new.valid_to, "9999-12-31", "new fact should be current");
    assert!(
        new.superseded_by.is_none(),
        "new fact should not be superseded"
    );

    // 4. Retract the corrected fact
    retract_fact(&store, "f-2", "2026-06-15T00:00:00Z");

    // 5. Verify retraction: nothing visible
    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after retract");
    assert!(
        results.is_empty(),
        "no facts should be visible after retraction"
    );

    // 6. Audit: both facts still present with full temporal metadata
    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 2, "audit should still show both facts");

    let retracted = audit
        .iter()
        .find(|r| r.id == "f-2")
        .expect("retracted in audit");
    assert_eq!(
        retracted.valid_to, "2026-06-15T00:00:00Z",
        "retracted fact should have valid_to set"
    );
}

#[test]
fn correct_preserves_metadata() {
    let store = open_store();
    let nous = "test-agent";

    // Insert fact with specific metadata
    let original = make_fact(
        "f-orig",
        nous,
        "The user prefers tea",
        0.75,
        EpistemicTier::Assumed,
    );
    store.insert_fact(&original).expect("insert original");

    // Correct it
    correct_fact(
        &store,
        "f-orig",
        "f-corrected",
        "The user prefers green tea",
        nous,
        "2026-06-01T00:00:00Z",
    );

    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 2);

    // Old fact's original metadata should be unchanged (except valid_to and superseded_by)
    let old = audit
        .iter()
        .find(|r| r.id == "f-orig")
        .expect("original in audit");
    assert!(
        (old.confidence - 0.75).abs() < f64::EPSILON,
        "original confidence unchanged"
    );
    assert_eq!(old.tier, "assumed", "original tier unchanged");
    assert_eq!(
        old.content, "The user prefers tea",
        "original content unchanged"
    );

    // New fact has corrected content and Verified tier
    let new = audit
        .iter()
        .find(|r| r.id == "f-corrected")
        .expect("corrected in audit");
    assert!(
        (new.confidence - 1.0).abs() < f64::EPSILON,
        "corrected fact gets confidence 1.0"
    );
    assert_eq!(new.tier, "verified", "corrected fact gets Verified tier");
    assert_eq!(new.content, "The user prefers green tea");
}

#[test]
fn retract_excludes_from_recall() {
    let store = open_store();
    let nous = "test-agent";
    let query_time = "2026-07-01T00:00:00Z";

    // Insert 3 facts on different topics
    let facts = [
        make_fact(
            "f-1",
            nous,
            "The engineer works at a startup",
            0.9,
            EpistemicTier::Verified,
        ),
        make_fact(
            "f-2",
            nous,
            "The engineer studies distributed systems",
            0.8,
            EpistemicTier::Inferred,
        ),
        make_fact(
            "f-3",
            nous,
            "The engineer speaks three languages",
            0.7,
            EpistemicTier::Assumed,
        ),
    ];
    for f in &facts {
        store.insert_fact(f).expect("insert fact");
    }

    // Verify all 3 visible
    let results = store.query_facts(nous, query_time, 10).expect("query all");
    assert_eq!(results.len(), 3);

    // Retract fact #2
    retract_fact(&store, "f-2", "2026-06-01T00:00:00Z");

    // Only facts #1 and #3 visible
    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after retract");
    assert_eq!(results.len(), 2, "retracted fact should be excluded");
    let ids: Vec<&str> = results.iter().map(|f| f.id.as_str()).collect();
    assert!(ids.contains(&"f-1"), "fact 1 should still be visible");
    assert!(ids.contains(&"f-3"), "fact 3 should still be visible");
    assert!(!ids.contains(&"f-2"), "fact 2 should be retracted");

    // Audit returns all 3 including retracted
    let audit = audit_all_facts(&store, nous);
    assert_eq!(
        audit.len(),
        3,
        "audit should show all facts including retracted"
    );

    let retracted = audit
        .iter()
        .find(|r| r.id == "f-2")
        .expect("retracted in audit");
    assert_eq!(retracted.valid_to, "2026-06-01T00:00:00Z");
}

#[test]
fn audit_filters_by_nous_id() {
    let store = open_store();

    // Insert facts under different nous_ids
    let facts_a = [
        make_fact(
            "fa-1",
            "agent-a",
            "Agent A fact one",
            0.9,
            EpistemicTier::Verified,
        ),
        make_fact(
            "fa-2",
            "agent-a",
            "Agent A fact two",
            0.8,
            EpistemicTier::Inferred,
        ),
    ];
    let facts_b = [make_fact(
        "fb-1",
        "agent-b",
        "Agent B fact one",
        0.85,
        EpistemicTier::Verified,
    )];

    for f in facts_a.iter().chain(facts_b.iter()) {
        store.insert_fact(f).expect("insert fact");
    }

    // Audit for agent-a
    let audit_a = audit_all_facts(&store, "agent-a");
    assert_eq!(audit_a.len(), 2, "agent-a should have 2 facts");
    assert!(
        audit_a.iter().all(|r| r.id.starts_with("fa-")),
        "all should be agent-a facts"
    );

    // Audit for agent-b
    let audit_b = audit_all_facts(&store, "agent-b");
    assert_eq!(audit_b.len(), 1, "agent-b should have 1 fact");
    assert_eq!(audit_b[0].id, "fb-1");

    // query_facts also scoped by nous_id
    let results_a = store
        .query_facts("agent-a", "2026-07-01T00:00:00Z", 10)
        .expect("query a");
    assert_eq!(results_a.len(), 2);

    let results_b = store
        .query_facts("agent-b", "2026-07-01T00:00:00Z", 10)
        .expect("query b");
    assert_eq!(results_b.len(), 1);
}
