//! Integration tests for the consolidation engine against a real
//! in-memory `KnowledgeStore`.
//!
//! These tests exercise the multiplicity side-index introduced for #3634:
//! when facts are consolidated, the source-observation count, time spread,
//! and first/last observation timestamps must be preserved so downstream
//! recall and conflict resolution can weight consolidated facts by
//! convergence strength.
#![expect(clippy::expect_used, reason = "test assertions")]

use super::*;
use crate::consolidation::ConsolidationResult;
use crate::test_fixtures::make_store;

/// Requirement #3634: consolidating N source facts into one Fact must
/// preserve the source count so downstream recall and conflict resolution
/// can weight by convergence strength.
///
/// Builds a `ConsolidationResult` describing 5 source facts merged into a
/// single consolidated fact, persists it via `persist_consolidated_facts`,
/// then reads back the multiplicity record and asserts:
/// - `source_count` equals the input count (5)
/// - `first_observed` / `last_observed` bound the source timestamps
/// - `time_spread_seconds` is non-negative and matches the span
#[test]
fn consolidation_preserves_multiplicity_metadata() {
    let store = make_store();

    let source_ids: Vec<FactId> = (0..5)
        .map(|i| FactId::new(format!("src-fact-{i}")).expect("valid test id"))
        .collect();
    let source_recorded_ats: Vec<String> = vec![
        "2026-01-01T00:00:00Z".to_owned(),
        "2026-01-02T00:00:00Z".to_owned(),
        "2026-01-03T00:00:00Z".to_owned(),
        "2026-01-04T00:00:00Z".to_owned(),
        "2026-01-05T00:00:00Z".to_owned(),
    ];

    let consolidated = ConsolidatedFact {
        content: "Alice is a senior engineer at Acme Corp".to_owned(),
        confidence: 0.95,
        tier: "inferred".to_owned(),
        source_fact_ids: source_ids.clone(),
        source_recorded_ats: source_recorded_ats.clone(),
    };
    let result = ConsolidationResult {
        original_count: source_ids.len(),
        consolidated_count: 1,
        consolidated_facts: vec![consolidated],
        superseded_fact_ids: source_ids.clone(),
    };

    let new_ids = store
        .persist_consolidated_facts(&result, "nous-test")
        .expect("persist succeeds");
    assert_eq!(
        new_ids.len(),
        1,
        "exactly one consolidated fact must be persisted"
    );

    let new_id = new_ids.first().expect("one new fact id").clone();
    let multiplicity = store
        .get_fact_multiplicity(&new_id)
        .expect("query succeeds")
        .expect("multiplicity record must exist for a consolidated fact");

    // Acceptance: source_count ≥ input count (equal here, ≥ honors the
    // brief's contract for cases where batches merge multiple times).
    let input_count = u32::try_from(source_ids.len()).expect("fits u32");
    assert!(
        multiplicity.source_count >= input_count,
        "source_count ({}) must be ≥ input count ({})",
        multiplicity.source_count,
        input_count
    );
    assert_eq!(
        multiplicity.source_count, input_count,
        "exact source_count must equal the number of source fact IDs"
    );

    // Time-spread: first/last observed must bound the inputs and the
    // spread must equal the full 4-day window in seconds (4 * 86_400).
    assert_eq!(
        multiplicity.first_observed, "2026-01-01T00:00:00Z",
        "first_observed must be the earliest source recorded_at"
    );
    assert_eq!(
        multiplicity.last_observed, "2026-01-05T00:00:00Z",
        "last_observed must be the latest source recorded_at"
    );
    assert_eq!(
        multiplicity.time_spread_seconds,
        4 * 86_400,
        "time_spread_seconds must match the full 4-day window"
    );
    assert_eq!(
        multiplicity.fact_id, new_id,
        "multiplicity record must be keyed on the new consolidated fact id"
    );
}

/// Negative control: facts not produced by consolidation have no
/// multiplicity record. `get_fact_multiplicity` returns `Ok(None)`.
#[test]
fn non_consolidated_fact_has_no_multiplicity() {
    let store = make_store();
    let missing_id = FactId::new("does-not-exist").expect("valid test id");
    let result = store
        .get_fact_multiplicity(&missing_id)
        .expect("query succeeds");
    assert!(
        result.is_none(),
        "facts with no consolidation history must return None"
    );
}
