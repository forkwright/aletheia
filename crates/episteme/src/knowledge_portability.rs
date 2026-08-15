//! Knowledge graph export/import for agent portability.

#[cfg(feature = "mneme-engine")]
use tracing::{info, instrument, warn};

#[cfg(feature = "mneme-engine")]
use crate::error::Result;

/// Build a `KnowledgeExport` from the knowledge store.
///
/// Queries scoped facts plus only graph data reachable from those facts. See
/// [`KnowledgeExport`](graphe::portability::KnowledgeExport) for the field
/// coverage this export provides. Returns `None` if the store is empty or
/// the query fails.
///
/// WHY: reads via [`audit_all_facts`](crate::knowledge_store::KnowledgeStore::audit_all_facts),
/// not [`query_facts`](crate::knowledge_store::KnowledgeStore::query_facts) --
/// the latter is a current-facts query (excludes forgotten and superseded
/// rows), so a portability export built on it would silently lose history a
/// caller might need to replay or audit.
#[cfg(feature = "mneme-engine")]
#[instrument(skip(store))]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "audit-correct knowledge export for agent portability; \
                  crates/aletheia/src/commands/agent_io.rs carries an \
                  independent export path pending unification"
    )
)]
pub(crate) fn export_knowledge(
    nous_id: &str,
    store: &crate::knowledge_store::KnowledgeStore,
) -> Option<graphe::portability::KnowledgeExport> {
    // kanon:ignore RUST/no-result-unwrap-or-default - best-effort portability snapshot: missing data on any leg yields an empty list (then the caller short-circuits to None below) rather than blocking the export.
    let facts = store
        .audit_all_facts(nous_id, 100_000)
        .ok()
        .unwrap_or_default();

    let fact_ids: Vec<crate::id::FactId> = facts.iter().map(|fact| fact.id.clone()).collect();

    // kanon:ignore RUST/no-result-unwrap-or-default - best-effort portability snapshot; see WHY above.
    let entities = store.list_entities_for_facts(&fact_ids).unwrap_or_default();
    // kanon:ignore RUST/no-result-unwrap-or-default - best-effort portability snapshot; see WHY above.
    let fact_entity_edges = store
        .list_fact_entity_edges_for_facts(&fact_ids)
        .unwrap_or_default()
        .into_iter()
        .map(|(fact_id, entity_id)| graphe::portability::FactEntityEdge { fact_id, entity_id })
        .collect();

    let entity_ids: std::collections::HashSet<String> = entities
        .iter()
        .map(|entity| entity.id.as_str().to_owned())
        .collect();

    // kanon:ignore RUST/no-result-unwrap-or-default - best-effort portability snapshot; see WHY above.
    let relationships = store
        .list_relationships_between_entities(&entity_ids)
        .unwrap_or_default();

    if facts.is_empty() && entities.is_empty() && relationships.is_empty() {
        return None;
    }

    info!(
        nous_id,
        facts = facts.len(),
        entities = entities.len(),
        relationships = relationships.len(),
        "knowledge exported"
    );

    Some(graphe::portability::KnowledgeExport {
        facts,
        entities,
        relationships,
        fact_entity_edges,
    })
}

/// Import knowledge graph data from a `KnowledgeExport` into a knowledge store.
///
/// # Errors
///
/// Returns errors if fact/entity/relationship insertion fails.
#[cfg(feature = "mneme-engine")]
#[instrument(skip(knowledge, store))]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "knowledge import for agent portability; \
                  crates/aletheia/src/commands/agent_io.rs carries an \
                  independent import path pending unification"
    )
)]
pub(crate) fn import_knowledge(
    knowledge: &graphe::portability::KnowledgeExport,
    store: &crate::knowledge_store::KnowledgeStore,
) -> Result<KnowledgeImportResult> {
    let mut result = KnowledgeImportResult::default();

    for entity in &knowledge.entities {
        if let Err(e) = store.insert_entity(entity) {
            warn!(entity_id = %entity.id, error = %e, "failed to import entity");
            continue;
        }
        result.entities_imported += 1;
    }

    for rel in &knowledge.relationships {
        if let Err(e) = store.insert_relationship(rel) {
            warn!(src = %rel.src, dst = %rel.dst, error = %e, "failed to import relationship");
            continue;
        }
        result.relationships_imported += 1;
    }

    for fact in &knowledge.facts {
        if let Err(e) = store.insert_fact(fact) {
            warn!(fact_id = %fact.id, error = %e, "failed to import fact");
            continue;
        }
        result.facts_imported += 1;
    }
    for edge in &knowledge.fact_entity_edges {
        if let Err(e) = store.insert_fact_entity(&edge.fact_id, &edge.entity_id) {
            warn!(
                fact_id = %edge.fact_id,
                entity_id = %edge.entity_id,
                error = %e,
                "failed to import fact/entity link"
            );
        }
    }

    info!(
        facts = result.facts_imported,
        entities = result.entities_imported,
        relationships = result.relationships_imported,
        "knowledge imported"
    );

    Ok(result)
}

/// Summary of knowledge graph import results.
#[cfg(feature = "mneme-engine")]
#[derive(Debug, Clone, Default)]
pub struct KnowledgeImportResult {
    /// Number of facts successfully imported.
    pub facts_imported: usize,
    /// Number of entities successfully imported.
    pub entities_imported: usize,
    /// Number of relationships successfully imported.
    pub relationships_imported: usize,
}

#[cfg(all(test, feature = "mneme-engine"))]
mod tests {
    #![expect(
        clippy::expect_used,
        clippy::indexing_slicing,
        reason = "test assertions"
    )]

    use std::collections::BTreeSet;

    use super::*;
    use crate::knowledge::{
        EpistemicTier, FactSensitivity, ForgetReason, MemoryScope, Visibility, format_timestamp,
    };
    use crate::test_fixtures::{make_entity, make_fact, make_relationship, make_store};

    #[test]
    fn knowledge_import_result_default_starts_at_zero() {
        let result = KnowledgeImportResult::default();
        assert_eq!(result.facts_imported, 0, "default facts_imported must be 0");
        assert_eq!(
            result.entities_imported, 0,
            "default entities_imported must be 0"
        );
        assert_eq!(
            result.relationships_imported, 0,
            "default relationships_imported must be 0"
        );
    }

    #[test]
    fn knowledge_import_result_fields_are_independent() {
        let result = KnowledgeImportResult {
            facts_imported: 5,
            entities_imported: 3,
            relationships_imported: 2,
        };
        assert_eq!(result.facts_imported, 5, "facts_imported should preserve 5");
        assert_eq!(
            result.entities_imported, 3,
            "entities_imported should preserve 3"
        );
        assert_eq!(
            result.relationships_imported, 2,
            "relationships_imported should preserve 2"
        );
    }

    #[test]
    fn export_knowledge_includes_only_entities_reachable_from_exported_facts() {
        let store = make_store();
        let alice_fact = make_fact("export-alice-fact", "alice", "alice scoped fact");
        let bob_fact = make_fact("export-bob-fact", "bob", "bob scoped fact");
        let alice_entity = make_entity("export-alice-entity", "Alice Entity", "topic");
        let bob_entity = make_entity("export-bob-entity", "Bob Entity", "topic");

        store.insert_fact(&alice_fact).expect("insert alice fact");
        store.insert_fact(&bob_fact).expect("insert bob fact");
        store
            .insert_entity(&alice_entity)
            .expect("insert alice entity");
        store.insert_entity(&bob_entity).expect("insert bob entity");
        store
            .insert_fact_entity(&alice_fact.id, &alice_entity.id)
            .expect("link alice entity");
        store
            .insert_fact_entity(&bob_fact.id, &bob_entity.id)
            .expect("link bob entity");
        store
            .insert_relationship(&make_relationship(
                "export-alice-entity",
                "export-bob-entity",
                "knows",
                0.8,
            ))
            .expect("insert cross-nous relationship");

        let exported = export_knowledge("alice", &store).expect("knowledge export");
        let entity_ids: Vec<&str> = exported
            .entities
            .iter()
            .map(|entity| entity.id.as_str())
            .collect();

        assert_eq!(exported.facts.len(), 1);
        assert_eq!(exported.facts[0].id.as_str(), "export-alice-fact");
        assert_eq!(exported.fact_entity_edges.len(), 1);
        assert_eq!(
            exported.fact_entity_edges[0].fact_id.as_str(),
            "export-alice-fact"
        );
        assert_eq!(
            exported.fact_entity_edges[0].entity_id.as_str(),
            "export-alice-entity"
        );
        assert!(entity_ids.contains(&"export-alice-entity"));
        assert!(
            !entity_ids.contains(&"export-bob-entity"),
            "foreign entity must not appear in scoped export"
        );
        assert!(
            exported.relationships.is_empty(),
            "relationship to a foreign entity must not appear in scoped export"
        );
    }

    #[test]
    fn export_uses_audit_path_and_includes_open_ended_forgotten_and_superseded_facts() {
        let store = make_store();

        // Open-ended fact: make_fact defaults valid_to to the far-future
        // sentinel. Regression coverage for #4548 -- a current-facts query
        // that compares valid_to > $now with the sentinel itself as $now
        // would exclude this row; an audit-path query must not.
        let open_ended = make_fact("kp-open-ended", "alice", "open-ended fact");

        let mut forgotten = make_fact("kp-forgotten", "alice", "forgotten fact");
        forgotten.lifecycle.is_forgotten = true;
        forgotten.lifecycle.forgotten_at = Some(forgotten.temporal.recorded_at);
        forgotten.lifecycle.forget_reason = Some(ForgetReason::UserRequested);

        let superseding = make_fact("kp-superseding", "alice", "superseding fact");
        let mut superseded = make_fact("kp-superseded", "alice", "superseded fact");
        superseded.lifecycle.superseded_by =
            Some(crate::id::FactId::new("kp-superseding").expect("valid id"));
        superseded.temporal.valid_to = superseded.temporal.recorded_at;

        store.insert_fact(&open_ended).expect("insert open-ended");
        store.insert_fact(&forgotten).expect("insert forgotten");
        store.insert_fact(&superseded).expect("insert superseded");
        store.insert_fact(&superseding).expect("insert superseding");

        let exported = export_knowledge("alice", &store).expect("knowledge export");
        let exported_ids: BTreeSet<&str> = exported.facts.iter().map(|f| f.id.as_str()).collect();
        for id in [
            "kp-open-ended",
            "kp-forgotten",
            "kp-superseded",
            "kp-superseding",
        ] {
            assert!(
                exported_ids.contains(id),
                "{id} must survive an audit-path export, which a \
                 current-facts query would silently drop"
            );
        }
    }

    #[test]
    fn export_import_round_trip_preserves_lifecycle_and_provenance_fields() {
        let source_store = make_store();

        let open_ended = make_fact("rt-open-ended", "alice", "open-ended fact");

        let mut forgotten = make_fact("rt-forgotten", "alice", "forgotten fact");
        forgotten.lifecycle.is_forgotten = true;
        forgotten.lifecycle.forgotten_at = Some(forgotten.temporal.recorded_at);
        forgotten.lifecycle.forget_reason = Some(ForgetReason::Outdated);

        let superseding = make_fact("rt-superseding", "alice", "superseding fact");
        let mut superseded = make_fact("rt-superseded", "alice", "superseded fact");
        superseded.lifecycle.superseded_by =
            Some(crate::id::FactId::new("rt-superseding").expect("valid id"));
        superseded.temporal.valid_to = superseded.temporal.recorded_at;

        let mut rich = make_fact("rt-rich", "alice", "rich provenance fact");
        rich.provenance.confidence = 0.42;
        rich.provenance.tier = EpistemicTier::Verified;
        rich.provenance.source_session_id = Some("session-99".to_owned());
        rich.visibility = Visibility::Shared;
        rich.sensitivity = FactSensitivity::Internal;
        rich.scope = Some(MemoryScope::Feedback);

        let entity = make_entity("rt-entity", "Entity", "topic");

        for fact in [&open_ended, &forgotten, &superseded, &superseding, &rich] {
            source_store.insert_fact(fact).expect("insert fact");
        }
        source_store.insert_entity(&entity).expect("insert entity");
        source_store
            .insert_fact_entity(&rich.id, &entity.id)
            .expect("link entity");

        let exported = export_knowledge("alice", &source_store).expect("knowledge export");
        assert_eq!(exported.facts.len(), 5, "all five facts must export");

        let dest_store = make_store();
        let counts = import_knowledge(&exported, &dest_store).expect("knowledge import");
        assert_eq!(counts.facts_imported, 5);
        assert_eq!(counts.entities_imported, 1);

        let reimported = export_knowledge("alice", &dest_store).expect("re-export after import");

        // Genuine round-trip equality: every field the acceptance criteria
        // name (provenance, lifecycle, scope, visibility) must match the
        // original after export -> import -> export, not merely be non-empty.
        let mut original: Vec<_> = exported.facts.iter().collect();
        let mut round_tripped: Vec<_> = reimported.facts.iter().collect();
        original.sort_by(|a, b| a.id.cmp(&b.id));
        round_tripped.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(original.len(), round_tripped.len());

        for (orig, back) in original.iter().zip(round_tripped.iter()) {
            assert_eq!(orig.id, back.id, "id must round-trip");
            assert_eq!(orig.content, back.content, "content must round-trip");
            assert!(
                (orig.provenance.confidence - back.provenance.confidence).abs() < f64::EPSILON,
                "confidence must round-trip: {} vs {}",
                orig.provenance.confidence,
                back.provenance.confidence
            );
            assert_eq!(
                orig.provenance.tier, back.provenance.tier,
                "tier must round-trip"
            );
            assert_eq!(
                orig.provenance.source_session_id, back.provenance.source_session_id,
                "source_session_id must round-trip"
            );
            assert_eq!(
                orig.lifecycle.is_forgotten, back.lifecycle.is_forgotten,
                "is_forgotten must round-trip"
            );
            assert_eq!(
                orig.lifecycle.superseded_by, back.lifecycle.superseded_by,
                "superseded_by must round-trip"
            );
            assert_eq!(
                format_timestamp(&orig.temporal.valid_to),
                format_timestamp(&back.temporal.valid_to),
                "valid_to must round-trip, including the far-future sentinel"
            );
            assert_eq!(
                orig.visibility, back.visibility,
                "visibility must round-trip"
            );
            assert_eq!(
                orig.sensitivity, back.sensitivity,
                "sensitivity must round-trip"
            );
            assert_eq!(orig.scope, back.scope, "scope must round-trip");
        }

        assert_eq!(
            reimported.fact_entity_edges.len(),
            1,
            "fact-entity edge must round-trip"
        );
        assert_eq!(reimported.fact_entity_edges[0].fact_id.as_str(), "rt-rich");
        assert_eq!(
            reimported.fact_entity_edges[0].entity_id.as_str(),
            "rt-entity"
        );
    }
}
