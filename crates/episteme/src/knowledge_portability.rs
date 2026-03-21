//! Knowledge graph export/import for agent portability.
#![cfg_attr(
    feature = "mneme-engine",
    expect(
        clippy::indexing_slicing,
        reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
    )
)]

use tracing::{info, instrument, warn};

use crate::error::Result;

/// Build a `KnowledgeExport` from the knowledge store.
///
/// Queries all facts, entities, and relationships for the given nous.
/// Returns `None` if the store is empty or the query fails.
#[cfg(feature = "mneme-engine")]
#[instrument(skip(store))]
pub fn export_knowledge(
    nous_id: &str,
    store: &crate::knowledge_store::KnowledgeStore,
) -> Option<aletheia_graphe::portability::KnowledgeExport> {
    let facts = store
        .query_facts(nous_id, "9999-01-01T00:00:00Z", 100_000)
        .ok()
        .unwrap_or_default();

    let entities = query_all_entities(store).unwrap_or_default();

    let relationships = query_all_relationships(store).unwrap_or_default();

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

    Some(aletheia_graphe::portability::KnowledgeExport {
        facts,
        entities,
        relationships,
    })
}

#[cfg(feature = "mneme-engine")]
fn query_all_entities(
    store: &crate::knowledge_store::KnowledgeStore,
) -> Result<Vec<crate::knowledge::Entity>> {
    use std::collections::BTreeMap;

    let script = r"?[id, name, entity_type, aliases, created_at, updated_at] := *entities{id, name, entity_type, aliases, created_at, updated_at}";
    let rows = store.run_query(script, BTreeMap::new())?;

    let mut entities = Vec::new();
    for row in &rows.rows {
        if row.len() < 6 {
            continue;
        }
        let id = crate::id::EntityId::new_unchecked(row[0].get_str().unwrap_or_default());
        let name = row[1].get_str().unwrap_or_default().to_owned();
        let entity_type = row[2].get_str().unwrap_or_default().to_owned();
        let aliases_str = row[3].get_str().unwrap_or_default();
        let aliases = if aliases_str.is_empty() {
            vec![]
        } else {
            aliases_str
                .split(',')
                .map(|s: &str| s.trim().to_owned())
                .collect()
        };
        let created_at = crate::knowledge::parse_timestamp(row[4].get_str().unwrap_or_default())
            .unwrap_or_else(jiff::Timestamp::now);
        let updated_at = crate::knowledge::parse_timestamp(row[5].get_str().unwrap_or_default())
            .unwrap_or_else(jiff::Timestamp::now);

        entities.push(crate::knowledge::Entity {
            id,
            name,
            entity_type,
            aliases,
            created_at,
            updated_at,
        });
    }

    Ok(entities)
}

#[cfg(feature = "mneme-engine")]
fn query_all_relationships(
    store: &crate::knowledge_store::KnowledgeStore,
) -> Result<Vec<crate::knowledge::Relationship>> {
    use std::collections::BTreeMap;

    let script = r"?[src, dst, relation, weight, created_at] := *relationships{src, dst, relation, weight, created_at}";
    let rows = store.run_query(script, BTreeMap::new())?;

    let mut relationships = Vec::new();
    for row in &rows.rows {
        if row.len() < 5 {
            continue;
        }
        let src = crate::id::EntityId::new_unchecked(row[0].get_str().unwrap_or_default());
        let dst = crate::id::EntityId::new_unchecked(row[1].get_str().unwrap_or_default());
        let relation = row[2].get_str().unwrap_or_default().to_owned();
        let weight = row[3].get_float().unwrap_or(0.0);
        let created_at = crate::knowledge::parse_timestamp(row[4].get_str().unwrap_or_default())
            .unwrap_or_else(jiff::Timestamp::now);

        relationships.push(crate::knowledge::Relationship {
            src,
            dst,
            relation,
            weight,
            created_at,
        });
    }

    Ok(relationships)
}

/// Import knowledge graph data from a `KnowledgeExport` into a knowledge store.
///
/// # Errors
///
/// Returns errors if fact/entity/relationship insertion fails.
#[cfg(feature = "mneme-engine")]
#[instrument(skip(knowledge, store))]
pub fn import_knowledge(
    knowledge: &aletheia_graphe::portability::KnowledgeExport,
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
