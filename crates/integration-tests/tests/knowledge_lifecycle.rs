//! Integration tests for knowledge lifecycle: correct, retract, and audit operations.
#![cfg(feature = "engine-tests")]

use aletheia_mneme::engine::DataValue;
use aletheia_mneme::knowledge::{EpistemicTier, Fact};
use aletheia_mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::sync::Arc;

fn make_fact(id: &str, nous_id: &str, content: &str, confidence: f64, tier: EpistemicTier) -> Fact {
    Fact {
        id: id.to_owned(),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        confidence,
        tier,
        valid_from: "2026-01-01T00:00:00Z".to_owned(),
        valid_to: "9999-12-31".to_owned(),
        superseded_by: None,
        source_session_id: Some("ses-test".to_owned()),
        recorded_at: "2026-03-01T00:00:00Z".to_owned(),
        access_count: 0,
        last_accessed_at: String::new(),
        stability_hours: 720.0,
        fact_type: String::new(),
        is_forgotten: false,
        forgotten_at: None,
        forget_reason: None,
    }
}

/// Replicates the adapter's `correct_fact` logic at the store level.
/// Marks old fact as superseded (`valid_to` = `correction_time`, `superseded_by` = `new_id`),
/// then inserts a new corrected fact.
fn correct_fact(
    store: &Arc<KnowledgeStore>,
    old_id: &str,
    new_id: &str,
    new_content: &str,
    nous_id: &str,
    correction_time: &str,
) {
    let script = r"
        ?[id, valid_from, content, nous_id, confidence, tier, valid_to, superseded_by, source_session_id, recorded_at,
          access_count, last_accessed_at, stability_hours, fact_type,
          is_forgotten, forgotten_at, forget_reason] :=
            *facts{id, valid_from, content, nous_id, confidence, tier, source_session_id, recorded_at,
                   access_count, last_accessed_at, stability_hours, fact_type,
                   is_forgotten, forgotten_at, forget_reason},
            id = $old_id,
            valid_to = $now,
            superseded_by = $new_id
        :put facts {id, valid_from => content, nous_id, confidence, tier, valid_to, superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type,
                    is_forgotten, forgotten_at, forget_reason}
    ";
    let mut params = BTreeMap::new();
    params.insert(String::from("old_id"), DataValue::Str(old_id.into()));
    params.insert(String::from("now"), DataValue::Str(correction_time.into()));
    params.insert(String::from("new_id"), DataValue::Str(new_id.into()));
    store
        .run_mut_query(script, params)
        .expect("correct: supersede old fact");

    let new_fact = Fact {
        id: new_id.to_owned(),
        nous_id: nous_id.to_owned(),
        content: new_content.to_owned(),
        confidence: 1.0,
        tier: EpistemicTier::Verified,
        valid_from: correction_time.to_owned(),
        valid_to: "9999-12-31".to_owned(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: correction_time.to_owned(),
        access_count: 0,
        last_accessed_at: String::new(),
        stability_hours: 720.0,
        fact_type: String::new(),
        is_forgotten: false,
        forgotten_at: None,
        forget_reason: None,
    };
    store
        .insert_fact(&new_fact)
        .expect("correct: insert new fact");
}

/// Replicates the adapter's `retract_fact` logic at the store level.
/// Sets `valid_to` = `retraction_time` on the fact (soft delete).
fn retract_fact(store: &Arc<KnowledgeStore>, fact_id: &str, retraction_time: &str) {
    let script = r"
        ?[id, valid_from, content, nous_id, confidence, tier, valid_to, superseded_by, source_session_id, recorded_at,
          access_count, last_accessed_at, stability_hours, fact_type,
          is_forgotten, forgotten_at, forget_reason] :=
            *facts{id, valid_from, content, nous_id, confidence, tier, superseded_by, source_session_id, recorded_at,
                   access_count, last_accessed_at, stability_hours, fact_type,
                   is_forgotten, forgotten_at, forget_reason},
            id = $fact_id,
            valid_to = $now
        :put facts {id, valid_from => content, nous_id, confidence, tier, valid_to, superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type,
                    is_forgotten, forgotten_at, forget_reason}
    ";
    let mut params = BTreeMap::new();
    params.insert(String::from("fact_id"), DataValue::Str(fact_id.into()));
    params.insert(String::from("now"), DataValue::Str(retraction_time.into()));
    store.run_mut_query(script, params).expect("retract fact");
}

/// Raw Datalog audit query: returns ALL facts for a `nous_id` without temporal filtering.
/// This is what `audit_facts` SHOULD do (the adapter currently filters out historical facts).
fn audit_all_facts(store: &Arc<KnowledgeStore>, nous_id: &str) -> Vec<AuditRow> {
    let script = r"
        ?[id, content, confidence, tier, valid_from, valid_to, superseded_by, recorded_at,
          is_forgotten, forgotten_at, forget_reason] :=
            *facts{id, valid_from, content, nous_id, confidence, tier, valid_to, superseded_by, recorded_at,
                   is_forgotten, forgotten_at, forget_reason},
            nous_id = $nous_id
        :order recorded_at
    ";
    let mut params = BTreeMap::new();
    params.insert(String::from("nous_id"), DataValue::Str(nous_id.into()));
    let rows = store.run_query(script, params).expect("audit query");

    rows.rows
        .into_iter()
        .map(|row| {
            let id = match &row[0] {
                DataValue::Str(s) => s.to_string(),
                other => panic!("expected Str for id, got {other:?}"),
            };
            let content = match &row[1] {
                DataValue::Str(s) => s.to_string(),
                other => panic!("expected Str for content, got {other:?}"),
            };
            let confidence = {
                let json: JsonValue = serde_json::to_value(&row[2]).expect("serialize confidence");
                json.as_f64()
                    .or_else(|| json.get("Float").and_then(JsonValue::as_f64))
                    .or_else(|| json.get("Int").and_then(JsonValue::as_f64))
                    .or_else(|| {
                        json.get("Num")
                            .and_then(|v| v.get("Float"))
                            .and_then(JsonValue::as_f64)
                    })
                    .unwrap_or_else(|| panic!("confidence as f64, got: {json}"))
            };
            let tier = match &row[3] {
                DataValue::Str(s) => s.to_string(),
                other => panic!("expected Str for tier, got {other:?}"),
            };
            let valid_from = match &row[4] {
                DataValue::Str(s) => s.to_string(),
                other => panic!("expected Str for valid_from, got {other:?}"),
            };
            let valid_to = match &row[5] {
                DataValue::Str(s) => s.to_string(),
                other => panic!("expected Str for valid_to, got {other:?}"),
            };
            let superseded_by = match &row[6] {
                DataValue::Null => None,
                DataValue::Str(s) => Some(s.to_string()),
                other => panic!("expected Str or Null for superseded_by, got {other:?}"),
            };
            let recorded_at = match &row[7] {
                DataValue::Str(s) => s.to_string(),
                other => panic!("expected Str for recorded_at, got {other:?}"),
            };
            let is_forgotten = match &row[8] {
                DataValue::Bool(b) => *b,
                other => panic!("expected Bool for is_forgotten, got {other:?}"),
            };
            let forgotten_at = match &row[9] {
                DataValue::Null => None,
                DataValue::Str(s) => Some(s.to_string()),
                other => panic!("expected Str or Null for forgotten_at, got {other:?}"),
            };
            let forget_reason = match &row[10] {
                DataValue::Null => None,
                DataValue::Str(s) => Some(s.to_string()),
                other => panic!("expected Str or Null for forget_reason, got {other:?}"),
            };
            AuditRow {
                id,
                content,
                confidence,
                tier,
                valid_from,
                valid_to,
                superseded_by,
                recorded_at,
                is_forgotten,
                forgotten_at,
                forget_reason,
            }
        })
        .collect()
}

#[derive(Debug)]
#[expect(
    dead_code,
    reason = "fields populated for debug output and future assertions"
)]
struct AuditRow {
    id: String,
    content: String,
    confidence: f64,
    tier: String,
    valid_from: String,
    valid_to: String,
    superseded_by: Option<String>,
    recorded_at: String,
    is_forgotten: bool,
    forgotten_at: Option<String>,
    forget_reason: Option<String>,
}

fn open_store() -> Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem")
}

#[test]
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

#[test]
fn supersession_chain() {
    let store = open_store();
    let nous = "test-agent";
    let query_time = "2026-09-01T00:00:00Z";

    // v1: original fact
    let v1 = make_fact(
        "v1",
        nous,
        "Project uses Python",
        0.8,
        EpistemicTier::Inferred,
    );
    store.insert_fact(&v1).expect("insert v1");

    // v1 -> v2: first correction
    correct_fact(
        &store,
        "v1",
        "v2",
        "Project uses Python and Rust",
        nous,
        "2026-04-01T00:00:00Z",
    );

    // v2 -> v3: second correction
    correct_fact(
        &store,
        "v2",
        "v3",
        "Project migrated fully to Rust",
        nous,
        "2026-07-01T00:00:00Z",
    );

    // Only v3 visible in query
    let results = store.query_facts(nous, query_time, 10).expect("query");
    assert_eq!(results.len(), 1, "only latest fact should be visible");
    assert_eq!(results[0].id, "v3");
    assert_eq!(results[0].content, "Project migrated fully to Rust");

    // Audit shows full chain
    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 3, "audit should show all 3 versions");

    let a_v1 = audit.iter().find(|r| r.id == "v1").expect("v1 in audit");
    let a_v2 = audit.iter().find(|r| r.id == "v2").expect("v2 in audit");
    let a_v3 = audit.iter().find(|r| r.id == "v3").expect("v3 in audit");

    // Supersession chain: v1 -> v2 -> v3
    assert_eq!(
        a_v1.superseded_by.as_deref(),
        Some("v2"),
        "v1 superseded by v2"
    );
    assert_eq!(
        a_v2.superseded_by.as_deref(),
        Some("v3"),
        "v2 superseded by v3"
    );
    assert!(
        a_v3.superseded_by.is_none(),
        "v3 is current — not superseded"
    );

    // Temporal validity: each version expired when the next was created
    assert_eq!(
        a_v1.valid_to, "2026-04-01T00:00:00Z",
        "v1 expired at v2 creation"
    );
    assert_eq!(
        a_v2.valid_to, "2026-07-01T00:00:00Z",
        "v2 expired at v3 creation"
    );
    assert_eq!(a_v3.valid_to, "9999-12-31", "v3 still current");
}

// --- Forget lifecycle tests ---

#[test]
fn forget_excludes_from_recall() {
    use aletheia_mneme::knowledge::ForgetReason;

    let store = open_store();
    let nous = "test-agent";
    let query_time = "2026-07-01T00:00:00Z";

    let fact = make_fact(
        "f-forget",
        nous,
        "Sensitive credential: token-abc-12345",
        0.9,
        EpistemicTier::Verified,
    );
    store.insert_fact(&fact).expect("insert");

    // Visible before forget
    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query before forget");
    assert_eq!(results.len(), 1);

    // Forget it
    store
        .forget_fact("f-forget", ForgetReason::Privacy)
        .expect("forget");

    // Not visible after forget
    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after forget");
    assert!(
        results.is_empty(),
        "forgotten fact should be excluded from recall"
    );
}

#[test]
fn forget_preserves_for_audit() {
    use aletheia_mneme::knowledge::ForgetReason;

    let store = open_store();
    let nous = "test-agent";

    let fact = make_fact(
        "f-audit",
        nous,
        "sensitive data",
        0.9,
        EpistemicTier::Verified,
    );
    store.insert_fact(&fact).expect("insert");

    store
        .forget_fact("f-audit", ForgetReason::Privacy)
        .expect("forget");

    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 1, "forgotten fact should appear in audit");

    let row = &audit[0];
    assert!(row.is_forgotten, "should be marked forgotten");
    assert!(
        row.forgotten_at.is_some(),
        "should have forgotten_at timestamp"
    );
    assert_eq!(
        row.forget_reason.as_deref(),
        Some("privacy"),
        "should have privacy reason"
    );
}

#[test]
fn unforget_restores_to_search() {
    use aletheia_mneme::knowledge::ForgetReason;

    let store = open_store();
    let nous = "test-agent";
    let query_time = "2026-07-01T00:00:00Z";

    let fact = make_fact(
        "f-unforget",
        nous,
        "reinstated fact",
        0.9,
        EpistemicTier::Verified,
    );
    store.insert_fact(&fact).expect("insert");

    store
        .forget_fact("f-unforget", ForgetReason::Outdated)
        .expect("forget");

    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after forget");
    assert!(results.is_empty(), "should be excluded after forget");

    store.unforget_fact("f-unforget").expect("unforget");

    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after unforget");
    assert_eq!(results.len(), 1, "should be restored after unforget");
    assert_eq!(results[0].id, "f-unforget");

    // Audit should show cleared forget metadata
    let audit = audit_all_facts(&store, nous);
    let row = &audit[0];
    assert!(
        !row.is_forgotten,
        "should not be marked forgotten after unforget"
    );
    assert!(row.forgotten_at.is_none(), "forgotten_at should be cleared");
    assert!(
        row.forget_reason.is_none(),
        "forget_reason should be cleared"
    );
}

#[test]
fn forget_with_each_reason() {
    use aletheia_mneme::knowledge::ForgetReason;

    let store = open_store();
    let nous = "test-agent";

    for (i, (reason, reason_str)) in [
        (ForgetReason::UserRequested, "user_requested"),
        (ForgetReason::Outdated, "outdated"),
        (ForgetReason::Incorrect, "incorrect"),
        (ForgetReason::Privacy, "privacy"),
    ]
    .iter()
    .enumerate()
    {
        let id = format!("f-reason-{i}");
        let fact = make_fact(
            &id,
            nous,
            &format!("fact for {reason_str}"),
            0.9,
            EpistemicTier::Verified,
        );
        store.insert_fact(&fact).expect("insert");
        store.forget_fact(&id, *reason).expect("forget");
    }

    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 4);
    for (i, reason_str) in ["user_requested", "outdated", "incorrect", "privacy"]
        .iter()
        .enumerate()
    {
        let row = audit
            .iter()
            .find(|r| r.id == format!("f-reason-{i}"))
            .expect("find fact");
        assert!(row.is_forgotten);
        assert_eq!(row.forget_reason.as_deref(), Some(*reason_str));
    }
}

#[test]
fn full_forget_lifecycle() {
    use aletheia_mneme::knowledge::ForgetReason;

    let store = open_store();
    let nous = "test-agent";
    let query_time = "2026-07-01T00:00:00Z";

    // 1. Insert
    let fact = make_fact(
        "f-lifecycle",
        nous,
        "The user stores a private note here",
        0.95,
        EpistemicTier::Verified,
    );
    store.insert_fact(&fact).expect("insert");

    // 2. Search: found
    let results = store.query_facts(nous, query_time, 10).expect("query");
    assert_eq!(results.len(), 1);

    // 3. Forget: privacy
    store
        .forget_fact("f-lifecycle", ForgetReason::Privacy)
        .expect("forget");

    // 4. Search: not found
    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after forget");
    assert!(results.is_empty());

    // 5. Audit: found with metadata
    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 1);
    assert!(audit[0].is_forgotten);
    assert_eq!(audit[0].forget_reason.as_deref(), Some("privacy"));

    // 6. Unforget
    store.unforget_fact("f-lifecycle").expect("unforget");

    // 7. Search: found again
    let results = store
        .query_facts(nous, query_time, 10)
        .expect("query after unforget");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "The user stores a private note here");
}
