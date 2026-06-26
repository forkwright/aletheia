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
use crate::test_fixtures::{make_entity, make_fact, make_relationship, make_store};

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
        source_fact_ids: source_ids.clone(),
        source_recorded_ats: source_recorded_ats.clone(),
        source_scopes: vec![None; source_ids.len()],
        source_project_ids: vec![None; source_ids.len()],
        source_sensitivities: vec![crate::knowledge::FactSensitivity::Public; source_ids.len()],
        source_visibilities: vec![crate::knowledge::Visibility::Private; source_ids.len()],
        source_session_ids: vec![Some("test-session".to_owned()); source_ids.len()],
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
        source_fact_ids: source_ids.clone(),
        source_recorded_ats: vec!["2026-01-01T00:00:00Z".to_owned(); source_ids.len()],
        source_scopes: vec![Some(MemoryScope::Project); source_ids.len()],
        source_project_ids: vec![Some(project_id.as_str().to_owned()); source_ids.len()],
        source_sensitivities: vec![FactSensitivity::Confidential; source_ids.len()],
        source_visibilities: vec![Visibility::Private; source_ids.len()],
        source_session_ids: vec![Some("secret-session".to_owned()); source_ids.len()],
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
        source_fact_ids: source_ids.clone(),
        source_recorded_ats: vec!["2026-01-01T00:00:00Z".to_owned(); source_ids.len()],
        source_scopes: vec![None; source_ids.len()],
        source_project_ids: vec![None; source_ids.len()],
        source_sensitivities: sensitivities,
        source_visibilities: vec![Visibility::Private; source_ids.len()],
        source_session_ids: vec![None; source_ids.len()],
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
        source_fact_ids: source_ids,
        source_recorded_ats: vec!["2026-01-01T00:00:00Z".to_owned(); 2],
        source_scopes: vec![None; 2],
        source_project_ids: project_ids,
        source_sensitivities: vec![FactSensitivity::Public; 2],
        source_visibilities: vec![Visibility::Private; 2],
        source_session_ids: vec![None; 2],
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

/// Requirement #4678: graph recomputation writes Louvain community assignments
/// using the same score type that consolidation reads for community overflow.
#[test]
fn graph_recompute_clusters_are_discovered_for_community_consolidation() {
    let store = make_store();
    for (id, name) in [("alice", "Alice"), ("bob", "Bob"), ("charlie", "Charlie")] {
        store
            .insert_entity(&make_entity(id, name, "person"))
            .expect("insert entity");
    }
    for (src, dst) in [
        ("alice", "bob"),
        ("bob", "alice"),
        ("alice", "charlie"),
        ("charlie", "alice"),
        ("bob", "charlie"),
        ("charlie", "bob"),
    ] {
        store
            .insert_relationship(&make_relationship(src, dst, "KNOWS", 0.9))
            .expect("insert relationship");
    }

    let facts = [
        (
            "cluster-fact-alice",
            "alice",
            "alice coordinates the launch",
        ),
        ("cluster-fact-bob", "bob", "bob owns the rollout plan"),
        (
            "cluster-fact-charlie",
            "charlie",
            "charlie tracks launch risk",
        ),
    ];
    for (fact_id, entity_id, content) in facts {
        let fact = make_fact(fact_id, "alice", content);
        let entity_id = EntityId::new(entity_id).expect("valid entity id");
        store.insert_fact(&fact).expect("insert fact");
        store
            .insert_fact_entity(&fact.id, &entity_id)
            .expect("link fact to entity");
    }

    store
        .recompute_graph_scores()
        .expect("graph recompute creates scores");
    let ctx = store.load_graph_context().expect("load graph context");
    assert!(
        !ctx.clusters.is_empty(),
        "graph recompute must write cluster assignments"
    );

    let config = ConsolidationConfig {
        community_fact_threshold: 3,
        ..ConsolidationConfig::default()
    };
    let candidates = store
        .find_community_overflow_candidates("alice", &config)
        .expect("find community candidates");
    let candidate = candidates
        .iter()
        .find(|candidate| candidate.fact_ids.len() == 3)
        .expect("consolidation must discover the recomputed graph cluster");

    let mut fact_ids: Vec<&str> = candidate.fact_ids.iter().map(FactId::as_str).collect();
    fact_ids.sort_unstable();
    assert_eq!(
        fact_ids,
        vec![
            "cluster-fact-alice",
            "cluster-fact-bob",
            "cluster-fact-charlie"
        ],
        "community candidate must gather the facts linked to the recomputed cluster"
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

    let result = run_llm_consolidation(&provider, &facts, &ConsolidationConfig::default())
        .expect("run_llm_consolidation must succeed");

    assert!(
        result.consolidated_facts.is_empty(),
        "empty LLM response must produce zero consolidated facts"
    );
    assert!(
        result.superseded_fact_ids.is_empty(),
        "empty LLM response must not supersede any source facts"
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
