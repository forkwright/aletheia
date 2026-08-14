//! Shared Datalog multi-path consistency queries against the knowledge store.
//!
//! Compares direct fact lookups against entity-traversal paths to detect
//! orphaned facts and dangling `fact_entities` references. Used by both
//! [`crate::prosoche::ProsocheCheck`]'s periodic `AttentionItem` pipeline
//! and [`crate::prosoche_audit::ConsistencyCheck`]'s `Finding` pipeline —
//! same queries, two different result shapes.

use std::collections::BTreeMap;
use std::collections::HashSet;

/// Query which of the given fact IDs have at least one `fact_entities` link.
///
/// Returns the subset of `fact_ids` that are present in the `fact_entities` relation.
pub(crate) fn query_entity_linked_fact_ids(
    store: &episteme::knowledge_store::KnowledgeStore,
    fact_ids: &[&str],
) -> Result<HashSet<String>, episteme::error::Error> {
    if fact_ids.is_empty() {
        return Ok(HashSet::new());
    }

    // WHY: Build an inline list of IDs for the Datalog `in [...]` operator.
    // Each ID is single-quoted and interior quotes are escaped.
    let id_list: Vec<String> = fact_ids
        .iter()
        .map(|id| format!("'{}'", id.replace('\'', "''")))
        .collect();

    let script = format!(
        "?[fact_id] := *fact_entities{{fact_id, entity_id}}, fact_id in [{}]",
        id_list.join(", ")
    );

    let result = store.run_query(&script, BTreeMap::new())?;

    let mut linked = HashSet::new();
    for i in 0..result.row_count() {
        if let Some(s) = result.get_string(i, "fact_id") {
            linked.insert(s);
        }
    }

    Ok(linked)
}

/// Query `fact_entities` for entries whose `fact_id` does not exist in `facts`.
///
/// Samples up to `limit` entries from `fact_entities` and checks each against
/// the `facts` relation. Returns the fact IDs that are dangling references.
pub(crate) fn query_dangling_fact_entity_refs(
    store: &episteme::knowledge_store::KnowledgeStore,
    limit: usize,
) -> Result<Vec<String>, episteme::error::Error> {
    // WHY: Two-pass in Rust because Datalog negation (`not`) requires all key
    // columns to be bound, but `facts` has composite key `(id, valid_from)`.
    // Query 1: collect distinct fact IDs referenced by `fact_entities`.
    // Query 2: collect distinct fact IDs that exist in `facts`.
    // Subtract in Rust to find dangling references.

    let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);

    // Step 1: Get a sample of fact IDs from fact_entities.
    let mut params1 = BTreeMap::new();
    params1.insert(
        "limit".to_owned(),
        episteme::engine::DataValue::from(limit_i64),
    );
    let fe_result = store.run_query(
        "?[fact_id] := *fact_entities{fact_id, entity_id} :limit $limit",
        params1,
    )?;

    let fe_ids: HashSet<String> = (0..fe_result.row_count())
        .filter_map(|i| fe_result.get_string(i, "fact_id"))
        .collect();

    if fe_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Step 2: Check which of those fact IDs actually exist in facts.
    let id_list: Vec<String> = fe_ids
        .iter()
        .map(|id| format!("'{}'", id.replace('\'', "''")))
        .collect();
    let script = format!(
        "?[id] := *facts{{id, valid_from}}, id in [{}]",
        id_list.join(", ")
    );
    let existing_result = store.run_query(&script, BTreeMap::new())?;

    let existing_ids: HashSet<String> = (0..existing_result.row_count())
        .filter_map(|i| existing_result.get_string(i, "id"))
        .collect();

    // Step 3: Difference = dangling references.
    let dangling: Vec<String> = fe_ids
        .into_iter()
        .filter(|id| !existing_ids.contains(id))
        .collect();

    Ok(dangling)
}
