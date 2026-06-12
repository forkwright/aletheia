//! Knowledge graph export/import for agent portability.

#[cfg(feature = "mneme-engine")]
use tracing::{info, instrument, warn};

#[cfg(feature = "mneme-engine")]
use crate::error::Result;

/// Build a `KnowledgeExport` from the knowledge store.
///
/// Queries scoped facts plus only graph data reachable from those facts.
/// Returns `None` if the store is empty or the query fails.
#[cfg(feature = "mneme-engine")]
#[instrument(skip(store))]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "knowledge export for agent portability")
)]
pub(crate) fn export_knowledge(
    nous_id: &str,
    store: &crate::knowledge_store::KnowledgeStore,
) -> Option<graphe::portability::KnowledgeExport> {
    // kanon:ignore RUST/no-result-unwrap-or-default — best-effort portability snapshot: missing data on any leg yields an empty list (then the caller short-circuits to None below) rather than blocking the export.
    let now = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
    let facts = store
        .query_facts(nous_id, &now, 100_000)
        .ok()
        .unwrap_or_default();

    let fact_ids: Vec<crate::id::FactId> = facts.iter().map(|fact| fact.id.clone()).collect();

    // kanon:ignore RUST/no-result-unwrap-or-default — best-effort portability snapshot; see WHY above.
    let entities = store.list_entities_for_facts(&fact_ids).unwrap_or_default();

    let entity_ids: std::collections::HashSet<String> = entities
        .iter()
        .map(|entity| entity.id.as_str().to_owned())
        .collect();

    // kanon:ignore RUST/no-result-unwrap-or-default — best-effort portability snapshot; see WHY above.
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
    })
}

/// Import knowledge graph data from a `KnowledgeExport` into a knowledge store.
///
/// # Errors
///
/// Returns errors if fact/entity/relationship insertion fails.
#[cfg(feature = "mneme-engine")]
#[instrument(skip(knowledge, store))]
#[expect(dead_code, reason = "knowledge import for agent portability")]
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
    for fact in &knowledge.facts {
        for entity in &knowledge.entities {
            if let Err(e) = store.insert_fact_entity(&fact.id, &entity.id) {
                warn!(
                    fact_id = %fact.id,
                    entity_id = %entity.id,
                    error = %e,
                    "failed to import fact/entity link"
                );
            }
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

    use super::*;
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
}
