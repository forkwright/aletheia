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
use crate::knowledge_store::{WriteStep, failpoint};
use crate::test_fixtures::{make_entity, make_fact, make_store};

/// Commit a hand-built `ConsolidationResult` through the same atomic write
/// path `execute_consolidation` uses: derive the run key from the candidate's
/// source set, build the write plan, commit it as one transaction.
fn commit_result(
    store: &KnowledgeStore,
    result: &ConsolidationResult,
    nous_id: &str,
) -> Result<Vec<FactId>, ConsolidationError> {
    let source_ids: Vec<FactId> = result.superseded_fact_ids.clone();
    let entity_id = EntityId::new("e-commit").expect("valid test id");
    let candidate = ConsolidationCandidate {
        trigger: ConsolidationTrigger::EntityOverflow {
            entity_id: entity_id.clone(),
            fact_count: source_ids.len(),
        },
        fact_ids: source_ids.clone(),
        fact_count: source_ids.len(),
        entity_id: Some(entity_id),
        cluster_id: None,
    };
    // Group outputs into supersession batches the way run_llm_consolidation
    // does: one batch per distinct source set, pointing at its first output.
    let mut batches: Vec<BatchSupersession> = Vec::new();
    for (index, consolidated) in result.consolidated_facts.iter().enumerate() {
        let seen = batches
            .iter()
            .any(|b| b.source_fact_ids == consolidated.source_fact_ids);
        if !seen {
            batches.push(BatchSupersession {
                source_fact_ids: Arc::clone(&consolidated.source_fact_ids),
                consolidated_fact_index: index,
            });
        }
    }
    let run_key = consolidation_run_key(
        &candidate,
        &source_ids,
        nous_id,
        &ConsolidationConfig::default(),
    );
    let plan = build_consolidation_write_plan(&candidate, result, &batches, nous_id, &run_key)?;
    store.commit_consolidation(&plan)
}

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

    let new_ids = commit_result(&store, &result, "nous-test").expect("commit succeeds");
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

    let new_ids = commit_result(&store, &result, "nous-test").expect("commit succeeds");
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

    let new_ids = commit_result(&store, &result, "nous-test").expect("commit succeeds");
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

    let err =
        commit_result(&store, &result, "nous-test").expect_err("mixed project IDs must be refused");
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
    assert_eq!(
        audit_ids(&store).len(),
        1,
        "an empty consolidation still records its audit row (the rate limiter reads it)"
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

/// Insert one row into the current-schema `consolidation_audit` table.
fn insert_audit_row(store: &KnowledgeStore, id: &str, nous_id: &str, consolidated_at: &str) {
    let script = r"
?[id, nous_id, trigger_type, trigger_id, original_count, consolidated_count,
   original_fact_ids, consolidated_fact_ids, consolidated_at] <-
    [[$id, $nous_id, 'entity_overflow', 'entity-1', 3, 1, '[]', '[]', $consolidated_at]]

:put consolidation_audit {id => nous_id, trigger_type, trigger_id, original_count,
                          consolidated_count, original_fact_ids,
                          consolidated_fact_ids, consolidated_at}
";
    let mut params = BTreeMap::new();
    params.insert("id".to_owned(), DataValue::Str(id.into()));
    params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
    params.insert(
        "consolidated_at".to_owned(),
        DataValue::Str(consolidated_at.into()),
    );
    store
        .run_mut_query(script, params)
        .expect("insert audit row");
}

fn audit_ids(store: &KnowledgeStore) -> BTreeSet<String> {
    let rows = store
        .run_query("?[id] := *consolidation_audit{id}", BTreeMap::new())
        .expect("query consolidation_audit ids");
    (0..rows.row_count())
        .map(|i| rows.get_string(i, "id").expect("row has an id"))
        .collect()
}

/// Requirement #5674: `consolidation_audit` is append-only with no TTL and no
/// row cap, so it grows for the life of the instance. Pruning must remove the
/// rows outside the retention window and nothing else — in particular it must
/// not reach across `nous_id`, because the relation is shared by every nous on
/// the store.
#[test]
fn prune_consolidation_audit_removes_only_expired_rows_for_the_named_nous() {
    let store = make_store();

    insert_audit_row(&store, "audit-old-1", "nous-a", "2026-01-01T00:00:00Z");
    insert_audit_row(&store, "audit-old-2", "nous-a", "2026-02-01T00:00:00Z");
    insert_audit_row(&store, "audit-fresh", "nous-a", "2026-06-01T00:00:00Z");
    insert_audit_row(&store, "audit-other", "nous-b", "2026-01-01T00:00:00Z");

    let (examined, removed) = store
        .prune_consolidation_audit("nous-a", "2026-03-01T00:00:00Z")
        .expect("prune must succeed");

    assert_eq!(examined, 3, "examined counts only nous-a's rows");
    assert_eq!(removed, 2, "both pre-cutoff nous-a rows are removed");

    let remaining = audit_ids(&store);
    assert_eq!(
        remaining,
        ["audit-fresh".to_owned(), "audit-other".to_owned()]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        "the in-window row and the other nous's row must both survive"
    );
}

/// A cutoff older than every stored row must delete nothing and report it,
/// rather than reporting a successful prune it did not perform.
#[test]
fn prune_consolidation_audit_is_a_no_op_when_nothing_is_expired() {
    let store = make_store();

    insert_audit_row(&store, "audit-fresh", "nous-a", "2026-06-01T00:00:00Z");

    let (examined, removed) = store
        .prune_consolidation_audit("nous-a", "2026-01-01T00:00:00Z")
        .expect("prune must succeed");

    assert_eq!(examined, 1);
    assert_eq!(removed, 0);
    assert_eq!(
        audit_ids(&store),
        ["audit-fresh".to_owned()]
            .into_iter()
            .collect::<BTreeSet<_>>()
    );
}

/// Seed a `graph_scores` row exactly as `graph_intelligence::RECOMPUTE_GRAPH_SCORES`
/// writes a Louvain community member: `score_type = 'cluster'`.
fn insert_cluster_score(store: &KnowledgeStore, entity_id: &str, cluster_id: i64) {
    let script = r"
?[entity_id, score_type, score, cluster_id, updated_at] <-
    [[$entity_id, 'cluster', 0.0, $cluster_id, '2026-03-01T00:00:00Z']]

:put graph_scores { entity_id, score_type => score, cluster_id, updated_at }
";
    let mut params = BTreeMap::new();
    params.insert("entity_id".to_owned(), DataValue::Str(entity_id.into()));
    params.insert("cluster_id".to_owned(), DataValue::from(cluster_id));
    store
        .run_mut_query(script, params)
        .expect("seed graph_scores cluster row");
}

/// Requirement (aletheia#4678): `find_community_overflow_candidates` must see
/// the clusters that `graph_intelligence`'s recompute pipeline actually
/// writes. The write path stamps `score_type = 'cluster'`
/// (`RECOMPUTE_GRAPH_SCORES`); `COMMUNITY_OVERFLOW_CANDIDATES` and
/// `CLUSTER_FACTS_FOR_CONSOLIDATION` must join on that same string, or every
/// computed community is invisible to consolidation.
#[test]
fn find_community_overflow_candidates_sees_cluster_scores_from_recompute() {
    let store = make_store();
    let entity = make_entity("e-cluster-member", "Cluster Member", "topic");
    store.insert_entity(&entity).expect("insert entity");

    let fact = make_fact("f-cluster-0", "alice", "clustered source fact");
    store.insert_fact(&fact).expect("insert fact");
    store
        .insert_fact_entity(&fact.id, &entity.id)
        .expect("link fact to entity");

    insert_cluster_score(&store, entity.id.as_str(), 7);

    let candidates = store
        .find_community_overflow_candidates(
            "alice",
            &ConsolidationConfig {
                community_fact_threshold: 1,
                ..ConsolidationConfig::default()
            },
        )
        .expect("find_community_overflow_candidates must succeed");

    assert_eq!(
        candidates.len(),
        1,
        "the fact's cluster must surface as a community-overflow candidate; got {candidates:?}"
    );
    let candidate = candidates.first().expect("exactly one candidate");
    assert_eq!(candidate.cluster_id, Some(7));
    assert_eq!(
        candidate.fact_ids,
        vec![fact.id.clone()],
        "gather_cluster_facts must join the same score_type as the overflow query"
    );
}

// ---------------------------------------------------------------------
// #5311: atomicity and idempotency of the consolidation write sequence
// ---------------------------------------------------------------------

/// Provider returning one consolidated fact per call and counting
/// invocations, so a test can prove a short-circuited retry never re-runs
/// the LLM.
struct CountingOneFactProvider {
    calls: std::sync::atomic::AtomicUsize,
}

impl ConsolidationProvider for CountingOneFactProvider {
    fn consolidate(
        &self,
        _system: &str,
        _user_message: &str,
    ) -> Result<String, ConsolidationError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(r#"[{"content":"consolidated: alice is an engineer"}]"#.to_owned())
    }
}

/// Seed an entity with `fact_count` linked facts and return the candidate
/// plus the source IDs. The facts use the fixtures' fixed 2026-03-01
/// `recorded_at`, so they pass the age gate under `min_age_days: 0`.
fn seed_consolidation_candidate(
    store: &KnowledgeStore,
    label: &str,
    fact_count: usize,
) -> (ConsolidationCandidate, Vec<FactId>) {
    let entity = make_entity(&format!("e-{label}"), &format!("{label} Entity"), "topic");
    store.insert_entity(&entity).expect("insert entity");

    let mut source_ids = Vec::with_capacity(fact_count);
    for i in 0..fact_count {
        let fact = make_fact(
            &format!("f-{label}-{i}"),
            "alice",
            &format!("{label} source fact {i}"),
        );
        store.insert_fact(&fact).expect("insert source fact");
        store
            .insert_fact_entity(&fact.id, &entity.id)
            .expect("link source fact to entity");
        source_ids.push(fact.id);
    }

    let candidate = ConsolidationCandidate {
        trigger: ConsolidationTrigger::EntityOverflow {
            entity_id: entity.id.clone(),
            fact_count,
        },
        fact_ids: source_ids.clone(),
        fact_count,
        entity_id: Some(entity.id),
        cluster_id: None,
    };
    (candidate, source_ids)
}

fn no_age_gate_config() -> ConsolidationConfig {
    ConsolidationConfig {
        min_age_days: 0,
        ..ConsolidationConfig::default()
    }
}

fn relation_ids(store: &KnowledgeStore, script: &str, column: &str) -> BTreeSet<String> {
    let rows = store
        .run_query(script, BTreeMap::new())
        .expect("list relation rows");
    (0..rows.row_count())
        .map(|i| rows.get_string(i, column).expect("row has the column"))
        .collect()
}

/// Every fact row in the store, any lifecycle state.
fn all_fact_ids(store: &KnowledgeStore) -> BTreeSet<String> {
    relation_ids(store, "?[id] := *facts{id}", "id")
}

fn multiplicity_ids(store: &KnowledgeStore) -> BTreeSet<String> {
    relation_ids(
        store,
        "?[fact_id] := *fact_multiplicity{fact_id}",
        "fact_id",
    )
}

fn provenance_ids(store: &KnowledgeStore) -> BTreeSet<String> {
    relation_ids(
        store,
        "?[consolidated_fact_id] := *consolidation_provenance{consolidated_fact_id}",
        "consolidated_fact_id",
    )
}

/// The `superseded_by` target of each listed fact (`None` while active).
fn superseded_targets(store: &KnowledgeStore, ids: &[FactId]) -> Vec<Option<FactId>> {
    ids.iter()
        .map(|id| {
            store
                .read_facts_by_id(id.as_str())
                .expect("read fact")
                .first()
                .and_then(|fact| fact.lifecycle.superseded_by.clone())
        })
        .collect()
}

/// Assert the aborted run left no trace: exactly the seeded sources, all
/// still active, and every side-index and audit relation empty.
fn assert_no_partial_state(store: &KnowledgeStore, source_ids: &[FactId]) {
    let source_id_set: BTreeSet<String> =
        source_ids.iter().map(|id| id.as_str().to_owned()).collect();
    assert_eq!(
        all_fact_ids(store),
        source_id_set,
        "no consolidated fact row may survive the aborted commit"
    );
    assert!(
        superseded_targets(store, source_ids)
            .iter()
            .all(Option::is_none),
        "no source may be marked superseded after an aborted commit"
    );
    assert!(
        multiplicity_ids(store).is_empty(),
        "no multiplicity row may survive the aborted commit"
    );
    assert!(
        provenance_ids(store).is_empty(),
        "no provenance row may survive the aborted commit"
    );
    assert!(
        audit_ids(store).is_empty(),
        "no audit row may survive the aborted commit"
    );
}

/// Assert the retried run converged: exactly the rows an uninterrupted run
/// committed in the never-failed store, with sources superseded once each.
fn assert_retry_converged(
    failed_store: &KnowledgeStore,
    clean_store: &KnowledgeStore,
    source_ids: &[FactId],
) {
    let retried_fact_ids = all_fact_ids(failed_store);
    assert_eq!(
        retried_fact_ids,
        all_fact_ids(clean_store),
        "the retried run must land the same fact IDs as an uninterrupted run"
    );
    assert_eq!(
        retried_fact_ids.len(),
        3,
        "two sources plus exactly one consolidated fact"
    );
    let cons_id = retried_fact_ids
        .iter()
        .find(|id| id.starts_with("cons-"))
        .expect("the deterministic cons- fact exists")
        .clone();

    for source in source_ids {
        let stored = failed_store
            .read_facts_by_id(source.as_str())
            .expect("read source fact");
        assert_eq!(
            stored.len(),
            1,
            "each source must still have exactly one temporal row"
        );
        assert_eq!(
            stored
                .first()
                .and_then(|f| f.lifecycle.superseded_by.as_ref().map(FactId::as_str)),
            Some(cons_id.as_str()),
            "each source must be superseded by the consolidated fact exactly once"
        );
    }
    assert_eq!(
        multiplicity_ids(failed_store),
        BTreeSet::from([cons_id.clone()]),
        "exactly one multiplicity row, keyed on the consolidated fact"
    );
    assert_eq!(
        provenance_ids(failed_store),
        BTreeSet::from([cons_id.clone()]),
        "exactly one provenance row, keyed on the consolidated fact"
    );
    let retried_audits = audit_ids(failed_store);
    assert_eq!(retried_audits.len(), 1, "exactly one audit row");
    assert!(
        retried_audits
            .iter()
            .next()
            .expect("one audit row")
            .starts_with("cons-audit-"),
        "the audit row is keyed on the deterministic run idempotency key"
    );
    assert_eq!(
        retried_audits,
        audit_ids(clean_store),
        "the retried run must land the same audit row as an uninterrupted run"
    );
}

/// Requirement #5311: every write in the consolidation sequence — fact,
/// multiplicity, provenance, supersession, audit — commits atomically. A
/// failure injected at `step` must leave no trace of the run, and the retry
/// (the failpoint fires exactly once) must converge to exactly the rows an
/// uninterrupted run commits in a second, never-failed store.
fn assert_step_failure_is_atomic_and_retry_converges(step: WriteStep) {
    let config = no_age_gate_config();

    let failed_store = make_store();
    let (candidate, source_ids) = seed_consolidation_candidate(&failed_store, "atomic", 2);

    failpoint::arm(step);
    let err = failed_store
        .execute_consolidation(
            &OneFactPerBatchProvider,
            &candidate,
            "alice",
            &config,
            false,
        )
        .expect_err("the armed write step must fail the consolidation");
    assert!(
        err.to_string()
            .contains("injected consolidation write failure"),
        "the failure must come from the failpoint, not the environment: {err}"
    );
    assert_no_partial_state(&failed_store, &source_ids);

    // Retry with the failpoint spent: the run must succeed and converge.
    let retried = failed_store
        .execute_consolidation(
            &OneFactPerBatchProvider,
            &candidate,
            "alice",
            &config,
            false,
        )
        .expect("retry after the aborted run must succeed");
    assert_eq!(retried.consolidated_count, 1);

    // An identical run in a never-failed store must land the same rows: the
    // minted IDs derive from the source set, not from fresh ULIDs.
    let clean_store = make_store();
    let (clean_candidate, _) = seed_consolidation_candidate(&clean_store, "atomic", 2);
    clean_store
        .execute_consolidation(
            &OneFactPerBatchProvider,
            &clean_candidate,
            "alice",
            &config,
            false,
        )
        .expect("clean run must succeed");

    assert_retry_converged(&failed_store, &clean_store, &source_ids);
}

#[test]
fn failure_at_fact_write_leaves_no_partial_state_and_retry_converges() {
    assert_step_failure_is_atomic_and_retry_converges(WriteStep::Fact);
}

#[test]
fn failure_at_multiplicity_write_leaves_no_partial_state_and_retry_converges() {
    assert_step_failure_is_atomic_and_retry_converges(WriteStep::Multiplicity);
}

#[test]
fn failure_at_provenance_write_leaves_no_partial_state_and_retry_converges() {
    assert_step_failure_is_atomic_and_retry_converges(WriteStep::Provenance);
}

#[test]
fn failure_at_supersede_write_leaves_no_partial_state_and_retry_converges() {
    assert_step_failure_is_atomic_and_retry_converges(WriteStep::Supersede);
}

#[test]
fn failure_at_audit_write_leaves_no_partial_state_and_retry_converges() {
    assert_step_failure_is_atomic_and_retry_converges(WriteStep::Audit);
}

/// Requirement #5311: a retry that finds the run's audit row already
/// committed must not call the LLM again — the recorded counts come back and
/// the store is untouched.
#[test]
fn committed_run_short_circuits_retry_before_the_llm() {
    let store = make_store();
    let (candidate, source_ids) = seed_consolidation_candidate(&store, "shortcircuit", 2);
    let config = no_age_gate_config();
    let run_key = consolidation_run_key(&candidate, &source_ids, "alice", &config);

    // Hand-write the run's idempotency record: the state a retry observes
    // when the first attempt committed but its outcome never reached the
    // caller.
    let source_ids_json =
        serde_json::to_string(&source_ids.iter().map(FactId::as_str).collect::<Vec<_>>())
            .expect("serialize source ids");
    let script = r"
?[id, nous_id, trigger_type, trigger_id, original_count, consolidated_count,
   original_fact_ids, consolidated_fact_ids, consolidated_at] <-
    [[$id, $nous_id, 'entity_overflow', 'e-shortcircuit', 2, 1, $original_fact_ids, '[]', '2026-06-01T00:00:00Z']]

:put consolidation_audit {id => nous_id, trigger_type, trigger_id, original_count,
                          consolidated_count, original_fact_ids,
                          consolidated_fact_ids, consolidated_at}
";
    let mut params = BTreeMap::new();
    params.insert(
        "id".to_owned(),
        DataValue::Str(audit_id_for_run_key(&run_key).into()),
    );
    params.insert("nous_id".to_owned(), DataValue::Str("alice".into()));
    params.insert(
        "original_fact_ids".to_owned(),
        DataValue::Str(source_ids_json.into()),
    );
    store
        .run_mut_query(script, params)
        .expect("insert idempotency record");

    let provider = CountingOneFactProvider {
        calls: std::sync::atomic::AtomicUsize::new(0),
    };
    let result = store
        .execute_consolidation(&provider, &candidate, "alice", &config, false)
        .expect("the recorded run resolves without the LLM");

    assert_eq!(
        provider.calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "a recorded run must never re-run the LLM"
    );
    assert_eq!(result.original_count, 2);
    assert_eq!(result.consolidated_count, 1);
    assert_eq!(
        result.superseded_fact_ids, source_ids,
        "the recorded result carries the run's superseded source IDs"
    );
    assert!(
        result.consolidated_facts.is_empty(),
        "a short-circuited retry returns recorded counts, not new payloads"
    );

    let expected: BTreeSet<String> = source_ids.iter().map(|id| id.as_str().to_owned()).collect();
    assert_eq!(
        all_fact_ids(&store),
        expected,
        "a short-circuited retry writes no facts"
    );
    assert!(
        superseded_targets(&store, &source_ids)
            .iter()
            .all(Option::is_none),
        "a short-circuited retry writes no supersessions"
    );
    assert!(multiplicity_ids(&store).is_empty());
    assert!(provenance_ids(&store).is_empty());
    assert_eq!(
        audit_ids(&store).len(),
        1,
        "still exactly the one idempotency record"
    );
}
