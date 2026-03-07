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
          access_count, last_accessed_at, stability_hours, fact_type] :=
            *facts{id, valid_from, content, nous_id, confidence, tier, source_session_id, recorded_at,
                   access_count, last_accessed_at, stability_hours, fact_type},
            id = $old_id,
            valid_to = $now,
            superseded_by = $new_id
        :put facts {id, valid_from => content, nous_id, confidence, tier, valid_to, superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type}
    ";
    let mut params = BTreeMap::new();
    params.insert("old_id".to_owned(), DataValue::Str(old_id.into()));
    params.insert("now".to_owned(), DataValue::Str(correction_time.into()));
    params.insert("new_id".to_owned(), DataValue::Str(new_id.into()));
    store.run_mut_query(script, params).expect("correct: supersede old fact");

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
    };
    store.insert_fact(&new_fact).expect("correct: insert new fact");
}

/// Replicates the adapter's `retract_fact` logic at the store level.
/// Sets `valid_to` = `retraction_time` on the fact (soft delete).
fn retract_fact(store: &Arc<KnowledgeStore>, fact_id: &str, retraction_time: &str) {
    let script = r"
        ?[id, valid_from, content, nous_id, confidence, tier, valid_to, superseded_by, source_session_id, recorded_at,
          access_count, last_accessed_at, stability_hours, fact_type] :=
            *facts{id, valid_from, content, nous_id, confidence, tier, superseded_by, source_session_id, recorded_at,
                   access_count, last_accessed_at, stability_hours, fact_type},
            id = $fact_id,
            valid_to = $now
        :put facts {id, valid_from => content, nous_id, confidence, tier, valid_to, superseded_by, source_session_id, recorded_at,
                    access_count, last_accessed_at, stability_hours, fact_type}
    ";
    let mut params = BTreeMap::new();
    params.insert("fact_id".to_owned(), DataValue::Str(fact_id.into()));
    params.insert("now".to_owned(), DataValue::Str(retraction_time.into()));
    store.run_mut_query(script, params).expect("retract fact");
}

/// Raw Datalog audit query — returns ALL facts for a `nous_id` without temporal filtering.
/// This is what `audit_facts` SHOULD do (the adapter currently filters out historical facts).
fn audit_all_facts(store: &Arc<KnowledgeStore>, nous_id: &str) -> Vec<AuditRow> {
    let script = r"
        ?[id, content, confidence, tier, valid_from, valid_to, superseded_by, recorded_at] :=
            *facts{id, valid_from, content, nous_id, confidence, tier, valid_to, superseded_by, recorded_at},
            nous_id = $nous_id
        :order recorded_at
    ";
    let mut params = BTreeMap::new();
    params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
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
                    .or_else(|| json.get("Num").and_then(|v| v.get("Float")).and_then(JsonValue::as_f64))
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
            AuditRow {
                id,
                content,
                confidence,
                tier,
                valid_from,
                valid_to,
                superseded_by,
                recorded_at,
            }
        })
        .collect()
}

#[derive(Debug)]
#[expect(dead_code, reason = "fields populated for debug output and future assertions")]
struct AuditRow {
    id: String,
    content: String,
    confidence: f64,
    tier: String,
    valid_from: String,
    valid_to: String,
    superseded_by: Option<String>,
    recorded_at: String,
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
    let original = make_fact("f-1", nous, "Cody's favorite language is Rust", 0.9, EpistemicTier::Inferred);
    store.insert_fact(&original).expect("insert original");

    // Verify searchable
    let results = store.query_facts(nous, query_time, 10).expect("query after insert");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "Cody's favorite language is Rust");

    // 2. Correct the fact
    correct_fact(&store, "f-1", "f-2", "Cody's favorite languages are Rust and TypeScript", nous, "2026-06-01T00:00:00Z");

    // 3. Verify correction — only new fact visible
    let results = store.query_facts(nous, query_time, 10).expect("query after correct");
    assert_eq!(results.len(), 1, "only corrected fact should be visible");
    assert_eq!(results[0].id, "f-2");
    assert_eq!(results[0].content, "Cody's favorite languages are Rust and TypeScript");

    // Audit: both facts visible with supersession metadata
    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 2, "audit should show both original and corrected");

    let old = audit.iter().find(|r| r.id == "f-1").expect("original in audit");
    assert_eq!(old.valid_to, "2026-06-01T00:00:00Z", "old fact should be expired");
    assert_eq!(old.superseded_by.as_deref(), Some("f-2"), "old fact should point to new");

    let new = audit.iter().find(|r| r.id == "f-2").expect("corrected in audit");
    assert_eq!(new.valid_to, "9999-12-31", "new fact should be current");
    assert!(new.superseded_by.is_none(), "new fact should not be superseded");

    // 4. Retract the corrected fact
    retract_fact(&store, "f-2", "2026-06-15T00:00:00Z");

    // 5. Verify retraction — nothing visible
    let results = store.query_facts(nous, query_time, 10).expect("query after retract");
    assert!(results.is_empty(), "no facts should be visible after retraction");

    // 6. Audit: both facts still present with full temporal metadata
    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 2, "audit should still show both facts");

    let retracted = audit.iter().find(|r| r.id == "f-2").expect("retracted in audit");
    assert_eq!(retracted.valid_to, "2026-06-15T00:00:00Z", "retracted fact should have valid_to set");
}

#[test]
fn correct_preserves_metadata() {
    let store = open_store();
    let nous = "test-agent";

    // Insert fact with specific metadata
    let original = make_fact("f-orig", nous, "Alice prefers tea", 0.75, EpistemicTier::Assumed);
    store.insert_fact(&original).expect("insert original");

    // Correct it
    correct_fact(&store, "f-orig", "f-corrected", "Alice prefers green tea", nous, "2026-06-01T00:00:00Z");

    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 2);

    // Old fact's original metadata should be unchanged (except valid_to and superseded_by)
    let old = audit.iter().find(|r| r.id == "f-orig").expect("original in audit");
    assert!((old.confidence - 0.75).abs() < f64::EPSILON, "original confidence unchanged");
    assert_eq!(old.tier, "assumed", "original tier unchanged");
    assert_eq!(old.content, "Alice prefers tea", "original content unchanged");

    // New fact has corrected content and Verified tier
    let new = audit.iter().find(|r| r.id == "f-corrected").expect("corrected in audit");
    assert!((new.confidence - 1.0).abs() < f64::EPSILON, "corrected fact gets confidence 1.0");
    assert_eq!(new.tier, "verified", "corrected fact gets Verified tier");
    assert_eq!(new.content, "Alice prefers green tea");
}

#[test]
fn retract_excludes_from_recall() {
    let store = open_store();
    let nous = "test-agent";
    let query_time = "2026-07-01T00:00:00Z";

    // Insert 3 facts on different topics
    let facts = [
        make_fact("f-1", nous, "Bob works at Acme Corp", 0.9, EpistemicTier::Verified),
        make_fact("f-2", nous, "Bob lives in Springfield", 0.8, EpistemicTier::Inferred),
        make_fact("f-3", nous, "Bob speaks three languages", 0.7, EpistemicTier::Assumed),
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
    let results = store.query_facts(nous, query_time, 10).expect("query after retract");
    assert_eq!(results.len(), 2, "retracted fact should be excluded");
    let ids: Vec<&str> = results.iter().map(|f| f.id.as_str()).collect();
    assert!(ids.contains(&"f-1"), "fact 1 should still be visible");
    assert!(ids.contains(&"f-3"), "fact 3 should still be visible");
    assert!(!ids.contains(&"f-2"), "fact 2 should be retracted");

    // Audit returns all 3 including retracted
    let audit = audit_all_facts(&store, nous);
    assert_eq!(audit.len(), 3, "audit should show all facts including retracted");

    let retracted = audit.iter().find(|r| r.id == "f-2").expect("retracted in audit");
    assert_eq!(retracted.valid_to, "2026-06-01T00:00:00Z");
}

#[test]
fn audit_filters_by_nous_id() {
    let store = open_store();

    // Insert facts under different nous_ids
    let facts_a = [
        make_fact("fa-1", "agent-a", "Agent A fact one", 0.9, EpistemicTier::Verified),
        make_fact("fa-2", "agent-a", "Agent A fact two", 0.8, EpistemicTier::Inferred),
    ];
    let facts_b = [
        make_fact("fb-1", "agent-b", "Agent B fact one", 0.85, EpistemicTier::Verified),
    ];

    for f in facts_a.iter().chain(facts_b.iter()) {
        store.insert_fact(f).expect("insert fact");
    }

    // Audit for agent-a
    let audit_a = audit_all_facts(&store, "agent-a");
    assert_eq!(audit_a.len(), 2, "agent-a should have 2 facts");
    assert!(audit_a.iter().all(|r| r.id.starts_with("fa-")), "all should be agent-a facts");

    // Audit for agent-b
    let audit_b = audit_all_facts(&store, "agent-b");
    assert_eq!(audit_b.len(), 1, "agent-b should have 1 fact");
    assert_eq!(audit_b[0].id, "fb-1");

    // query_facts also scoped by nous_id
    let results_a = store.query_facts("agent-a", "2026-07-01T00:00:00Z", 10).expect("query a");
    assert_eq!(results_a.len(), 2);

    let results_b = store.query_facts("agent-b", "2026-07-01T00:00:00Z", 10).expect("query b");
    assert_eq!(results_b.len(), 1);
}

#[test]
fn supersession_chain() {
    let store = open_store();
    let nous = "test-agent";
    let query_time = "2026-09-01T00:00:00Z";

    // v1: original fact
    let v1 = make_fact("v1", nous, "Project uses Python", 0.8, EpistemicTier::Inferred);
    store.insert_fact(&v1).expect("insert v1");

    // v1 -> v2: first correction
    correct_fact(&store, "v1", "v2", "Project uses Python and Rust", nous, "2026-04-01T00:00:00Z");

    // v2 -> v3: second correction
    correct_fact(&store, "v2", "v3", "Project migrated fully to Rust", nous, "2026-07-01T00:00:00Z");

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
    assert_eq!(a_v1.superseded_by.as_deref(), Some("v2"), "v1 superseded by v2");
    assert_eq!(a_v2.superseded_by.as_deref(), Some("v3"), "v2 superseded by v3");
    assert!(a_v3.superseded_by.is_none(), "v3 is current — not superseded");

    // Temporal validity: each version expired when the next was created
    assert_eq!(a_v1.valid_to, "2026-04-01T00:00:00Z", "v1 expired at v2 creation");
    assert_eq!(a_v2.valid_to, "2026-07-01T00:00:00Z", "v2 expired at v3 creation");
    assert_eq!(a_v3.valid_to, "9999-12-31", "v3 still current");
}
