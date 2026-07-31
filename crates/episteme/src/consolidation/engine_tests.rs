//! Integration tests for the consolidation engine against a real
//! in-memory `KnowledgeStore`.
//!
//! These tests exercise the multiplicity side-index introduced for #3634:
//! when facts are consolidated, the source-observation count, time spread,
//! and first/last observation timestamps must be preserved so downstream
//! recall and conflict resolution can weight consolidated facts by
//! convergence strength.
#![expect(clippy::expect_used, reason = "test assertions")]

use std::collections::BTreeSet;

use super::*;
use crate::consolidation::ConsolidationResult;
use crate::engine::DataValue;
use crate::test_fixtures::{make_entity, make_fact, make_store};

// kanon:ignore RUST/doc-promised-observability — doc comment describes data-flow invariants, not tracing
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
        source_fact_ids: source_ids.clone().into(),
        source_recorded_ats: source_recorded_ats.clone().into(),
        source_scopes: vec![None; source_ids.len()].into(),
        source_project_ids: vec![None; source_ids.len()].into(),
        source_sensitivities: vec![crate::knowledge::FactSensitivity::Public; source_ids.len()]
            .into(),
        source_visibilities: vec![crate::knowledge::Visibility::Private; source_ids.len()].into(),
        source_session_ids: vec![Some("test-session".to_owned()); source_ids.len()].into(),
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

/// Requirement #4660: a consolidated fact built from confidential,
/// project-scoped sources must stay confidential and project-scoped.
///
/// Builds a `ConsolidationResult` whose sources all share
/// `scope = Project`, a single project ID, `sensitivity = Confidential`,
/// and a common source session. After `persist_consolidated_facts`, the
/// stored fact and its provenance side-index must retain those boundaries.
#[test]
fn consolidation_preserves_confidential_project_metadata() {
    use crate::knowledge::{FactSensitivity, MemoryScope, Visibility};
    use eidos::workspace::ProjectId;

    let store = make_store();
    let project_id = ProjectId::from_git_remote("https://github.com/forkwright/secret-project.git")
        .expect("valid project remote");

    let source_ids: Vec<FactId> = (0..3)
        .map(|i| FactId::new(format!("src-conf-{i}")).expect("valid test id"))
        .collect();

    let consolidated = ConsolidatedFact {
        content: "Alice has access to the secret project".to_owned(),
        confidence: 0.95,
        tier: "inferred".to_owned(),
        source_fact_ids: source_ids.clone().into(),
        source_recorded_ats: vec!["2026-01-01T00:00:00Z".to_owned(); source_ids.len()].into(),
        source_scopes: vec![Some(MemoryScope::Project); source_ids.len()].into(),
        source_project_ids: vec![Some(project_id.as_str().to_owned()); source_ids.len()].into(),
        source_sensitivities: vec![FactSensitivity::Confidential; source_ids.len()].into(),
        source_visibilities: vec![Visibility::Private; source_ids.len()].into(),
        source_session_ids: vec![Some("secret-session".to_owned()); source_ids.len()].into(),
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
    let new_id = new_ids.first().expect("one new fact").clone();

    let stored = store
        .read_facts_by_id(new_id.as_str())
        .expect("read back consolidated fact");
    let fact = stored
        .first()
        .expect("consolidated fact has one temporal row");

    assert_eq!(
        fact.sensitivity,
        FactSensitivity::Confidential,
        "confidential sources must produce a confidential consolidated fact"
    );
    assert_eq!(
        fact.visibility,
        Visibility::Private,
        "private visibility must be preserved"
    );
    assert_eq!(
        fact.scope,
        Some(MemoryScope::Project),
        "project scope must be preserved"
    );
    assert_eq!(
        fact.project_id.as_ref().map(ProjectId::as_str),
        Some(project_id.as_str()),
        "project ID must be preserved"
    );

    let provenance = store
        .get_consolidation_provenance(&new_id)
        .expect("provenance query succeeds")
        .expect("provenance side-index must exist");
    assert!(
        provenance.0.len() >= source_ids.len(),
        "provenance must record at least the source fact IDs"
    );
    assert!(
        provenance.1.contains(&"secret-session".to_owned()),
        "provenance must retain the source session ID"
    );
}

/// Requirement #4660: mixed sensitivities take the strictest (most
/// restrictive) value, so a single confidential source prevents the output
/// from becoming public.
#[test]
fn consolidation_mixed_sensitivity_takes_strictest() {
    use crate::knowledge::{FactSensitivity, Visibility};

    let store = make_store();
    let source_ids: Vec<FactId> = (0..3)
        .map(|i| FactId::new(format!("src-mixed-{i}")).expect("valid test id"))
        .collect();

    let sensitivities = vec![
        FactSensitivity::Public,
        FactSensitivity::Internal,
        FactSensitivity::Confidential,
    ];
    let consolidated = ConsolidatedFact {
        content: "Alice can access internal systems".to_owned(),
        confidence: 0.95,
        tier: "inferred".to_owned(),
        source_fact_ids: source_ids.clone().into(),
        source_recorded_ats: vec!["2026-01-01T00:00:00Z".to_owned(); source_ids.len()].into(),
        source_scopes: vec![None; source_ids.len()].into(),
        source_project_ids: vec![None; source_ids.len()].into(),
        source_sensitivities: sensitivities.into(),
        source_visibilities: vec![Visibility::Private; source_ids.len()].into(),
        source_session_ids: vec![None; source_ids.len()].into(),
    };
    let result = ConsolidationResult {
        original_count: source_ids.len(),
        consolidated_count: 1,
        consolidated_facts: vec![consolidated],
        superseded_fact_ids: source_ids,
    };

    let new_ids = store
        .persist_consolidated_facts(&result, "nous-test")
        .expect("persist succeeds");
    let new_id = new_ids.first().expect("one new fact").clone();

    let stored = store
        .read_facts_by_id(new_id.as_str())
        .expect("read back consolidated fact");
    let fact = stored.first().expect("one row");
    assert_eq!(
        fact.sensitivity,
        FactSensitivity::Confidential,
        "mixed sensitivities must collapse to the most restrictive"
    );
}

/// Requirement #4660: mixed project IDs are refused rather than emitted as a
/// single global fact, avoiding cross-project leakage.
#[test]
fn consolidation_mixed_project_ids_refused() {
    use crate::knowledge::{FactSensitivity, Visibility};
    use eidos::workspace::ProjectId;

    let store = make_store();
    let project_a = ProjectId::from_git_remote("https://github.com/forkwright/project-a.git")
        .expect("valid project remote");
    let project_b = ProjectId::from_git_remote("https://github.com/forkwright/project-b.git")
        .expect("valid project remote");

    let source_ids: Vec<FactId> = (0..2)
        .map(|i| FactId::new(format!("src-proj-{i}")).expect("valid test id"))
        .collect();
    let project_ids: Vec<Option<String>> = vec![
        Some(project_a.as_str().to_owned()),
        Some(project_b.as_str().to_owned()),
    ];

    let consolidated = ConsolidatedFact {
        content: "Alice works on both projects".to_owned(),
        confidence: 0.95,
        tier: "inferred".to_owned(),
        source_fact_ids: source_ids.into(),
        source_recorded_ats: vec!["2026-01-01T00:00:00Z".to_owned(); 2].into(),
        source_scopes: vec![None; 2].into(),
        source_project_ids: project_ids.into(),
        source_sensitivities: vec![FactSensitivity::Public; 2].into(),
        source_visibilities: vec![Visibility::Private; 2].into(),
        source_session_ids: vec![None; 2].into(),
    };
    let result = ConsolidationResult {
        original_count: 2,
        consolidated_count: 1,
        consolidated_facts: vec![consolidated],
        superseded_fact_ids: vec![],
    };

    let err = store
        .persist_consolidated_facts(&result, "nous-test")
        .expect_err("mixed project IDs must be refused");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("mixed project IDs"),
        "error should identify project conflict: {msg}"
    );
}

// WHY (#5849): Mock provider that returns an empty JSON array, exercising the
// zero-output consolidation path that previously destroyed source facts.
struct EmptyResponseProvider;

impl ConsolidationProvider for EmptyResponseProvider {
    fn consolidate(
        &self,
        _system: &str,
        _user_message: &str,
    ) -> Result<String, ConsolidationError> {
        Ok("[]".to_owned())
    }
}

/// Requirement #5849: a batch whose LLM response is `[]` must produce zero
/// consolidated facts and zero superseded fact IDs.
#[test]
fn run_llm_consolidation_empty_response_skips_supersession() {
    let provider = EmptyResponseProvider;
    let facts: Vec<SourceFact> = (0..3)
        .map(|i| SourceFact {
            id: FactId::new(format!("f-empty-{i}")).expect("valid test id"),
            content: format!("source fact {i}"),
            confidence: 0.8,
            recorded_at: "2026-01-01T00:00:00Z".to_owned(),
            scope: None,
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
            source_session_id: None,
        })
        .collect();

    let LlmConsolidationResult {
        result,
        supersession_batches,
    } = run_llm_consolidation(&provider, &facts, &ConsolidationConfig::default())
        .expect("run_llm_consolidation must succeed");

    assert!(
        result.consolidated_facts.is_empty(),
        "empty LLM response must produce zero consolidated facts"
    );
    assert!(
        result.superseded_fact_ids.is_empty(),
        "empty LLM response must not supersede any source facts"
    );
    assert!(
        supersession_batches.is_empty(),
        "empty LLM response must not create a batch supersession plan"
    );
}

/// Requirement #5849: after `execute_consolidation` with an empty LLM response,
/// the source facts must remain retrievable (not marked superseded).
#[test]
fn execute_consolidation_empty_response_preserves_source_facts() {
    let store = make_store();
    let entity = make_entity("e-empty", "Empty Entity", "topic");
    store.insert_entity(&entity).expect("insert entity");

    let fact = crate::test_fixtures::make_fact("f-empty-0", "alice", "source fact zero");
    store.insert_fact(&fact).expect("insert fact");
    store
        .insert_fact_entity(&fact.id, &entity.id)
        .expect("link fact to entity");

    let candidate = ConsolidationCandidate {
        trigger: ConsolidationTrigger::EntityOverflow {
            entity_id: entity.id.clone(),
            fact_count: 1,
        },
        fact_ids: vec![fact.id.clone()],
        fact_count: 1,
        entity_id: Some(entity.id.clone()),
        cluster_id: None,
    };

    let result = store
        .execute_consolidation(
            &EmptyResponseProvider,
            &candidate,
            "alice",
            &ConsolidationConfig::default(),
            false,
        )
        .expect("execute_consolidation must succeed");

    assert!(
        result.consolidated_facts.is_empty(),
        "empty consolidation must produce zero new facts"
    );
    assert!(
        result.superseded_fact_ids.is_empty(),
        "empty consolidation must not supersede source facts"
    );

    let remaining = store
        .query_facts("alice", "2026-06-17T00:00:00Z", 10)
        .expect("query active facts");
    let ids: Vec<&str> = remaining.iter().map(|f| f.id.as_str()).collect();
    assert!(
        ids.contains(&"f-empty-0"),
        "source fact must remain retrievable after empty consolidation; got {ids:?}"
    );
}

// WHY (#5847): one output per provider call makes each persisted consolidated
// fact correspond to exactly one source batch.
struct OneFactPerBatchProvider;

impl ConsolidationProvider for OneFactPerBatchProvider {
    fn consolidate(
        &self,
        _system: &str,
        _user_message: &str,
    ) -> Result<String, ConsolidationError> {
        Ok(r#"[{"content":"consolidated batch"}]"#.to_owned())
    }
}

/// Requirement #5847: each batch's sources must point to that batch's own
/// consolidated fact, not the first fact persisted by the entire run.
#[test]
fn multi_batch_consolidation_supersedes_sources_with_their_own_batch_fact() {
    let store = make_store();
    let entity = make_entity("e-multi-batch", "Multi Batch Entity", "topic");
    store.insert_entity(&entity).expect("insert entity");

    let source_facts: Vec<_> = (0..4)
        .map(|i| {
            make_fact(
                &format!("f-multi-batch-{i}"),
                "alice",
                &format!("source fact {i}"),
            )
        })
        .collect();
    for fact in &source_facts {
        store.insert_fact(fact).expect("insert source fact");
        store
            .insert_fact_entity(&fact.id, &entity.id)
            .expect("link source fact to entity");
    }

    let candidate = ConsolidationCandidate {
        trigger: ConsolidationTrigger::EntityOverflow {
            entity_id: entity.id.clone(),
            fact_count: source_facts.len(),
        },
        fact_ids: source_facts.iter().map(|fact| fact.id.clone()).collect(),
        fact_count: source_facts.len(),
        entity_id: Some(entity.id.clone()),
        cluster_id: None,
    };
    let config = ConsolidationConfig {
        min_age_days: 0,
        batch_limit: 2,
        ..ConsolidationConfig::default()
    };

    let result = store
        .execute_consolidation(
            &OneFactPerBatchProvider,
            &candidate,
            "alice",
            &config,
            false,
        )
        .expect("multi-batch consolidation succeeds");

    assert_eq!(
        result.consolidated_count, 2,
        "four sources with batch_limit two must produce two consolidated facts"
    );

    let mut superseding_ids = BTreeSet::new();
    for source in &source_facts {
        let stored = store
            .read_facts_by_id(source.id.as_str())
            .expect("read superseded source fact");
        let superseding_id = stored
            .first()
            .expect("source fact row exists")
            .lifecycle
            .superseded_by
            .clone()
            .expect("source fact is superseded");
        let provenance = store
            .get_consolidation_provenance(&superseding_id)
            .expect("read consolidated provenance")
            .expect("superseding fact has provenance");

        assert!(
            provenance.0.contains(&source.id),
            "source {} must point to a consolidated fact built from it; target {} has sources {:?}",
            source.id,
            superseding_id,
            provenance.0
        );
        superseding_ids.insert(superseding_id);
    }

    assert_eq!(
        superseding_ids.len(),
        2,
        "two batches must produce two distinct supersession targets"
    );
}

/// The `consolidation_audit` shape in force before #6380 added `nous_id`.
/// `KnowledgeStore::open_mem` always builds the current schema (`nous_id`
/// baked in via `CONSOLIDATION_AUDIT_DDL`), so a real legacy table has to be
/// recreated by hand to exercise the backfill branch under test.
const PRE_6380_CONSOLIDATION_AUDIT_DDL: &str = r":create consolidation_audit {
    id: String =>
    trigger_type: String,
    trigger_id: String,
    original_count: Int,
    consolidated_count: Int,
    original_fact_ids: String,
    consolidated_fact_ids: String,
    consolidated_at: String
}";

/// Insert one row into a pre-#6380 `consolidation_audit` table (no `nous_id`
/// column).
fn insert_pre_6380_audit_row(store: &KnowledgeStore, id: &str, consolidated_at: &str) {
    let script = r"
?[id, trigger_type, trigger_id, original_count, consolidated_count,
   original_fact_ids, consolidated_fact_ids, consolidated_at] <-
    [[$id, 'entity_overflow', 'entity-legacy', 3, 1, '[]', '[]', $consolidated_at]]

:put consolidation_audit {id => trigger_type, trigger_id, original_count,
                          consolidated_count, original_fact_ids,
                          consolidated_fact_ids, consolidated_at}
";
    let mut params = BTreeMap::new();
    params.insert("id".to_owned(), DataValue::Str(id.into()));
    params.insert(
        "consolidated_at".to_owned(),
        DataValue::Str(consolidated_at.into()),
    );
    store
        .run_mut_query(script, params)
        .expect("insert pre-#6380 audit row");
}

/// Requirement #6384: `ensure_consolidation_audit_owner_scope` must migrate a
/// genuine pre-#6380 `consolidation_audit` table (no `nous_id` column at
/// all), not the current schema that already bakes the column in. Every
/// legacy row must survive the migration, and each must land with
/// `nous_id == ""` — the conservative default, since a legacy row does not
/// reliably prove a single owner (#6380).
#[test]
fn ensure_consolidation_audit_owner_scope_backfills_pre_6380_table() {
    let store = make_store();

    // `make_store` builds the CURRENT schema (nous_id already baked in via
    // `init_schema` -> `CONSOLIDATION_AUDIT_DDL`). Drop it and recreate the
    // relation in the exact pre-#6380 shape so the backfill branch has a
    // real legacy table to migrate, rather than a no-op over the current one.
    store
        .run_mut_query("::remove consolidation_audit", BTreeMap::new())
        .expect("drop current-schema consolidation_audit relation");
    store
        .run_mut_query(PRE_6380_CONSOLIDATION_AUDIT_DDL, BTreeMap::new())
        .expect("recreate consolidation_audit in the pre-#6380 shape");

    let legacy_ids = ["audit-legacy-1", "audit-legacy-2", "audit-legacy-3"];
    for (i, id) in legacy_ids.iter().enumerate() {
        insert_pre_6380_audit_row(&store, id, &format!("2026-01-0{}T00:00:00Z", i + 1));
    }

    store
        .ensure_consolidation_audit_owner_scope()
        .expect("migration must succeed against a real pre-#6380 table");

    let rows = store
        .run_query(
            "?[id, nous_id] := *consolidation_audit{id, nous_id}",
            BTreeMap::new(),
        )
        .expect("query migrated consolidation_audit rows");

    assert_eq!(
        rows.row_count(),
        legacy_ids.len(),
        "backfill must preserve every legacy row, not drop or duplicate any"
    );

    let mut migrated_ids = BTreeSet::new();
    for i in 0..rows.row_count() {
        assert_eq!(
            rows.get_string(i, "nous_id").as_deref(),
            Some(""),
            "row {i} must backfill to the empty owner default, not an arbitrary nous"
        );
        migrated_ids.insert(rows.get_string(i, "id").expect("row has an id"));
    }
    for id in legacy_ids {
        assert!(
            migrated_ids.contains(id),
            "legacy row {id} must survive the migration"
        );
    }
}

// WHY(#5694): three outputs per provider call makes the sharing of batch-level
// metadata across sibling facts observable — one output per batch could not
// distinguish a shared allocation from a per-fact copy.
struct ThreeFactsPerBatchProvider;

impl ConsolidationProvider for ThreeFactsPerBatchProvider {
    fn consolidate(
        &self,
        _system: &str,
        _user_message: &str,
    ) -> Result<String, ConsolidationError> {
        Ok(r#"[{"content":"first"},{"content":"second"},{"content":"third"}]"#.to_owned())
    }
}

/// Requirement #5694: the seven `source_*` metadata fields are batch-level, so
/// every consolidated fact from one batch must share one allocation with its
/// siblings rather than owning a copy — while facts from different batches
/// stay independent and keep their own batch's values.
#[test]
fn consolidated_facts_share_batch_metadata_within_a_batch_only() {
    let provider = ThreeFactsPerBatchProvider;
    // Two source facts per batch across two batches: sharing must hold inside
    // each batch and must not leak across them.
    let facts: Vec<SourceFact> = (0..4)
        .map(|i| SourceFact {
            id: FactId::new(format!("f-share-{i}")).expect("valid test id"),
            content: format!("source fact {i}"),
            confidence: 0.8,
            recorded_at: format!("2026-01-0{}T00:00:00Z", i + 1),
            scope: None,
            project_id: None,
            sensitivity: FactSensitivity::Public,
            visibility: Visibility::Private,
            source_session_id: None,
        })
        .collect();

    let config = ConsolidationConfig {
        batch_limit: 2,
        ..ConsolidationConfig::default()
    };
    let LlmConsolidationResult { result, .. } = run_llm_consolidation(&provider, &facts, &config)
        .expect("run_llm_consolidation must succeed");

    let consolidated = &result.consolidated_facts;
    assert_eq!(
        consolidated.len(),
        6,
        "two batches of three outputs each must produce six consolidated facts"
    );

    let mut batches = consolidated.chunks(3);
    let first_batch = batches.next().expect("first batch of three outputs");
    let second_batch = batches.next().expect("second batch of three outputs");
    assert!(
        batches.next().is_none(),
        "four sources at batch_limit 2 must form exactly two batches"
    );

    // Siblings from the same batch share every batch-level allocation.
    for (label, batch) in [("first", first_batch), ("second", second_batch)] {
        let (head, siblings) = batch.split_first().expect("batch has at least one output");
        for sibling in siblings {
            assert!(
                Arc::ptr_eq(&head.source_fact_ids, &sibling.source_fact_ids),
                "source_fact_ids must be shared within the {label} batch"
            );
            assert!(
                Arc::ptr_eq(&head.source_recorded_ats, &sibling.source_recorded_ats),
                "source_recorded_ats must be shared within the {label} batch"
            );
            assert!(
                Arc::ptr_eq(&head.source_scopes, &sibling.source_scopes),
                "source_scopes must be shared within the {label} batch"
            );
            assert!(
                Arc::ptr_eq(&head.source_project_ids, &sibling.source_project_ids),
                "source_project_ids must be shared within the {label} batch"
            );
            assert!(
                Arc::ptr_eq(&head.source_sensitivities, &sibling.source_sensitivities),
                "source_sensitivities must be shared within the {label} batch"
            );
            assert!(
                Arc::ptr_eq(&head.source_visibilities, &sibling.source_visibilities),
                "source_visibilities must be shared within the {label} batch"
            );
            assert!(
                Arc::ptr_eq(&head.source_session_ids, &sibling.source_session_ids),
                "source_session_ids must be shared within the {label} batch"
            );
        }
    }

    let first_head = first_batch.first().expect("first batch has an output");
    let second_head = second_batch.first().expect("second batch has an output");

    // Different batches keep independent allocations and their own sources.
    assert!(
        !Arc::ptr_eq(&first_head.source_fact_ids, &second_head.source_fact_ids),
        "separate batches must not share a source_fact_ids allocation"
    );

    // The sharing must not have changed what each batch actually carries.
    let batch_ids = |fact: &ConsolidatedFact| -> Vec<String> {
        fact.source_fact_ids
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect()
    };
    assert_eq!(
        batch_ids(first_head),
        vec!["f-share-0".to_owned(), "f-share-1".to_owned()],
        "first batch must carry its own two source ids"
    );
    assert_eq!(
        batch_ids(second_head),
        vec!["f-share-2".to_owned(), "f-share-3".to_owned()],
        "second batch must carry its own two source ids"
    );
    assert_eq!(
        first_head.source_recorded_ats.as_ref(),
        [
            "2026-01-01T00:00:00Z".to_owned(),
            "2026-01-02T00:00:00Z".to_owned()
        ],
        "batch metadata must stay aligned to the batch's own source facts"
    );
}
