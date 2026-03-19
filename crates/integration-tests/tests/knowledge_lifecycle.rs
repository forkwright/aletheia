//! Integration tests for knowledge lifecycle: correct, retract, and audit operations.
#![cfg(feature = "engine-tests")]

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value as JsonValue;

use aletheia_mneme::engine::DataValue;
use aletheia_mneme::knowledge::{EpistemicTier, Fact};
use aletheia_mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};

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

mod forget;
#[test]
mod lifecycle;
