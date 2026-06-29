use snafu::ResultExt;

use super::marshal::{
    entity_to_params, extract_float, extract_int, extract_str, relationship_to_params,
};
use tracing::instrument;

use super::{KnowledgeStore, QueryResult, queries};

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    /// Insert or update an entity.
    #[instrument(skip(self, entity), fields(entity_id = %entity.id))]
    pub fn insert_entity(&self, entity: &crate::knowledge::Entity) -> crate::error::Result<()> {
        use snafu::ensure;
        ensure!(!entity.name.is_empty(), crate::error::EmptyEntityNameSnafu);
        let params = entity_to_params(entity);
        self.run_mut(&queries::upsert_entity(), params)?;
        // WHY (#4662): ontological rules key off `entity_type`, so an entity
        // upsert can change derived IS-A closure. Mark derived facts stale.
        self.invalidate_derived_facts()
    }

    /// Insert a relationship.
    #[instrument(skip(self, rel))]
    pub fn insert_relationship(
        &self,
        rel: &crate::knowledge::Relationship,
    ) -> crate::error::Result<()> {
        use snafu::ensure;
        ensure!(
            (0.0..=1.0).contains(&rel.weight),
            crate::error::InvalidWeightSnafu { value: rel.weight }
        );
        let params = relationship_to_params(rel);
        self.run_mut(&queries::upsert_relationship(), params)
    }

    /// Remove entities that have neither relationships nor fact references.
    ///
    /// The orphan predicate matches the operator-facing validation check:
    /// entities are safe to delete only when they have no incoming or
    /// outgoing relationships and no `fact_entities` rows pointing at them.
    #[instrument(skip(self))]
    pub fn remove_orphaned_entities(&self) -> crate::error::Result<usize> {
        let orphaned_ids = self.orphaned_entity_ids()?;
        let mut removed = 0usize;

        for orphan_id in orphaned_ids {
            let entity_id =
                crate::id::EntityId::new(&orphan_id).context(crate::error::InvalidIdSnafu)?;
            self.delete_entity(&entity_id)?;
            removed = removed.saturating_add(1);
        }

        Ok(removed)
    }

    /// Query 2-hop entity neighborhood.
    ///
    /// Returns a [`QueryResult`] whose rows correspond to the Datalog output of
    /// `ENTITY_NEIGHBORHOOD`. Columns: `id`, `score`, `hops`.
    #[instrument(skip(self))]
    pub(crate) fn entity_neighborhood(
        &self,
        entity_id: &crate::id::EntityId,
    ) -> crate::error::Result<QueryResult> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(
            "entity_id".to_owned(),
            DataValue::Str(entity_id.as_str().into()),
        );
        self.run_read(queries::ENTITY_NEIGHBORHOOD, params)
            .map(QueryResult::from)
    }

    /// Insert a fact-entity mapping.
    #[instrument(skip(self))]
    pub fn insert_fact_entity(
        &self,
        fact_id: &crate::id::FactId,
        entity_id: &crate::id::EntityId,
    ) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let now = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
        let mut params = BTreeMap::new();
        params.insert(
            "fact_id".to_owned(),
            DataValue::Str(fact_id.as_str().into()),
        );
        params.insert(
            "entity_id".to_owned(),
            DataValue::Str(entity_id.as_str().into()),
        );
        params.insert("created_at".to_owned(), DataValue::Str(now.into()));
        self.run_mut(&queries::upsert_fact_entity(), params)
    }

    /// List all entities in the knowledge store.
    #[instrument(skip(self))]
    pub fn list_entities(&self) -> crate::error::Result<Vec<crate::knowledge::Entity>> {
        use std::collections::BTreeMap;

        let script = r"?[id, name, entity_type, aliases, created_at, updated_at] :=
            *entities{id, name, entity_type, aliases, created_at, updated_at}
            :order name";
        let rows = self.run_read(script, BTreeMap::new())?;

        parse_entities(&rows)
    }

    /// List entities reachable from the supplied fact IDs.
    ///
    /// Used by portability/export paths so a scoped fact export cannot carry
    /// graph nodes that belong only to other nouses in the same cohort store.
    pub fn list_entities_for_facts(
        &self,
        fact_ids: &[crate::id::FactId],
    ) -> crate::error::Result<Vec<crate::knowledge::Entity>> {
        use std::collections::BTreeMap;

        if fact_ids.is_empty() {
            return Ok(Vec::new());
        }

        let fact_id_list = quoted_id_list(fact_ids.iter().map(crate::id::FactId::as_str));
        let script = format!(
            r"?[id, name, entity_type, aliases, created_at, updated_at] :=
                *fact_entities{{fact_id, entity_id: id}},
                fact_id in [{fact_id_list}],
                *entities{{id, name, entity_type, aliases, created_at, updated_at}}
            :order name"
        );
        let rows = self.run_read(&script, BTreeMap::new())?;
        parse_entities(&rows)
    }

    /// List exact fact-to-entity links for the supplied fact IDs.
    ///
    /// Used by portability/export paths so import can restore the original
    /// bipartite edges instead of linking every exported fact to every entity.
    pub fn list_fact_entity_edges_for_facts(
        &self,
        fact_ids: &[crate::id::FactId],
    ) -> crate::error::Result<Vec<(crate::id::FactId, crate::id::EntityId)>> {
        use std::collections::BTreeMap;

        if fact_ids.is_empty() {
            return Ok(Vec::new());
        }

        let fact_id_list = quoted_id_list(fact_ids.iter().map(crate::id::FactId::as_str));
        let script = format!(
            r"?[fact_id, entity_id] :=
                *fact_entities{{fact_id, entity_id}},
                fact_id in [{fact_id_list}]
            :order fact_id, entity_id"
        );
        let rows = self.run_read(&script, BTreeMap::new())?;

        let mut edges = Vec::new();
        for row in &rows.rows {
            let fact_id = crate::id::FactId::new(row_str(row, 0, "fact_id", "fact_entity edge")?)
                .context(crate::error::InvalidIdSnafu)?;
            let entity_id =
                crate::id::EntityId::new(row_str(row, 1, "entity_id", "fact_entity edge")?)
                    .context(crate::error::InvalidIdSnafu)?;
            edges.push((fact_id, entity_id));
        }
        Ok(edges)
    }

    /// List all relationships in the knowledge graph.
    ///
    /// Used by agent portability export (issue #4163) to round-trip the full
    /// relationship set. Not part of the recall/serve hot path — recall uses
    /// targeted hop queries, not full enumeration.
    #[instrument(skip(self))]
    pub fn list_all_relationships(
        &self,
    ) -> crate::error::Result<Vec<crate::knowledge::Relationship>> {
        use std::collections::BTreeMap;

        let script = r"?[src, dst, relation, weight, created_at] :=
            *relationships{src, dst, relation, weight, created_at}
            :order src";
        let rows = self.run_read(script, BTreeMap::new())?;

        parse_relationships(&rows)
    }

    /// List relationships whose endpoints are both in `entity_ids`.
    pub fn list_relationships_between_entities(
        &self,
        entity_ids: &std::collections::HashSet<String>,
    ) -> crate::error::Result<Vec<crate::knowledge::Relationship>> {
        use std::collections::BTreeMap;

        if entity_ids.is_empty() {
            return Ok(Vec::new());
        }

        let entity_id_list = quoted_id_list(entity_ids.iter().map(String::as_str));
        let script = format!(
            r"?[src, dst, relation, weight, created_at] :=
                *relationships{{src, dst, relation, weight, created_at}},
                src in [{entity_id_list}],
                dst in [{entity_id_list}]
            :order src"
        );
        let rows = self.run_read(&script, BTreeMap::new())?;
        parse_relationships(&rows)
    }

    /// Build a serendipity-ready graph snapshot from the current store.
    pub(crate) fn build_serendipity_snapshot(
        &self,
        seed_entity_ids: &[String],
    ) -> crate::error::Result<crate::serendipity::GraphSnapshot> {
        let ctx = self.build_graph_context(seed_entity_ids)?;
        let nodes = self
            .list_entities()?
            .into_iter()
            .map(|entity| (entity.id.as_str().to_owned(), entity.name));
        let edges = self.list_all_relationships()?.into_iter().map(|rel| {
            (
                rel.src.as_str().to_owned(),
                rel.dst.as_str().to_owned(),
                rel.relation,
            )
        });

        Ok(crate::serendipity::GraphSnapshot::from_graph_context(
            &ctx, nodes, edges,
        ))
    }

    /// Load entities scoped to `nous_id` as lightweight `EntityInfo` structs.
    ///
    /// Restricts the result to entities reachable from a fact owned by
    /// `nous_id` via the `fact_entities` join. The `entities` relation
    /// itself carries no tenant column (entities are physically shared
    /// across nouses inside a cohort store), so the only honest way to
    /// scope the dedup input is to walk through the per-tenant `facts`
    /// relation. Without this join, two nouses' entity sets would bleed
    /// into each other during dedup once Path A's embedding wiring made
    /// cross-tenant `AutoMerge` reachable (#4165 E / latent F).
    ///
    /// Entities not referenced by any fact (e.g. raw `insert_entity`
    /// calls in unit-test fixtures) are excluded — they have no tenant
    /// affiliation and could be merged into any nous's graph, which is
    /// exactly the leak this scoping is meant to close. Test fixtures
    /// that want their entities to participate in dedup must link them
    /// via [`insert_fact_entity`](Self::insert_fact_entity) to a fact
    /// owned by the target `nous_id`.
    pub(super) fn load_entity_infos(
        &self,
        nous_id: &str,
    ) -> crate::error::Result<Vec<crate::dedup::EntityInfo>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;

        // WHY (#4165 Path A): pull `name_embedding` alongside the other
        // entity fields so the dedup pipeline can compute real cosine
        // similarity for the `embed_sim` term in the merge score.
        //
        // WHY (#4165 E): tenant-scope via fact_entities → facts.nous_id.
        // The set-of-tuples semantics of Datalog deduplicate entities
        // referenced by multiple facts within the same nous, so the
        // returned vector never contains a given entity twice.
        let script = r"?[id, name, entity_type, aliases, created_at, name_embedding] :=
            *facts{id: fact_id, nous_id},
            nous_id == $nous_id,
            *fact_entities{fact_id, entity_id: id},
            *entities{id, name, entity_type, aliases, created_at, name_embedding}";
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        let rows = self.run_read(script, params)?;

        let mut entities = Vec::new();
        for row in &rows.rows {
            let id_str = row_str(row, 0, "id", "dedup entity info")?;
            let name = row_str(row, 1, "name", "dedup entity info")?;
            let entity_type = row_str(row, 2, "entity_type", "dedup entity info")?;
            let aliases = parse_aliases(&row_str(row, 3, "aliases", "dedup entity info")?);
            let created_at = row_timestamp(row, 4, "created_at", "dedup entity info")?;
            let name_embedding = super::marshal::extract_optional_f32_vec(row_value(
                row,
                5,
                "name_embedding",
                "dedup entity info",
            )?)?;

            let rel_count = self.count_relationships(&id_str)?;
            let fact_count = self.count_facts(&id_str)?;

            let id = crate::id::EntityId::new(&id_str).context(crate::error::InvalidIdSnafu)?;
            entities.push(crate::dedup::EntityInfo {
                id,
                name,
                entity_type,
                aliases,
                relationship_count: checked_u32(rel_count, "dedup entity relationship_count")?,
                fact_count: checked_u32(fact_count, "dedup entity fact_count")?,
                created_at,
                name_embedding,
            });
        }
        Ok(entities)
    }

    /// Return the entity IDs that can be garbage-collected safely.
    ///
    /// This keeps the garbage-collection command aligned with the same orphan
    /// predicate used by the validation report: no relationships in either
    /// direction and no facts referencing the entity.
    pub fn orphaned_entity_ids(&self) -> crate::error::Result<Vec<String>> {
        use std::collections::BTreeMap;

        let script = r"?[id] :=
            *entities{id},
            not *relationships{src: id},
            not *relationships{dst: id},
            not *fact_entities{entity_id: id}";
        let rows = self.run_read(script, BTreeMap::new())?;
        let mut ids = Vec::with_capacity(rows.rows.len());
        for row in &rows.rows {
            if let Some(id) = row.first().and_then(|v| v.get_str()) {
                ids.push(id.to_owned());
            }
        }
        Ok(ids)
    }

    /// Load a single entity by ID.
    pub(super) fn load_entity(
        &self,
        entity_id: &crate::id::EntityId,
    ) -> crate::error::Result<crate::knowledge::Entity> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(entity_id.as_str().into()));
        let script = r"?[id, name, entity_type, aliases, created_at, updated_at] :=
            *entities{id, name, entity_type, aliases, created_at, updated_at},
            id = $id";
        let rows = self.run_read(script, params)?;
        let row = rows.rows.into_iter().next().ok_or_else(|| {
            crate::error::EngineQuerySnafu {
                message: format!("entity not found: {entity_id}"),
            }
            .build()
        })?;

        let aliases = parse_aliases(&row_str(&row, 3, "aliases", "entity row")?);
        let created_at = row_timestamp(&row, 4, "created_at", "entity row")?;
        let updated_at = row_timestamp(&row, 5, "updated_at", "entity row")?;

        Ok(crate::knowledge::Entity {
            id: entity_id.clone(),
            name: row_str(&row, 1, "name", "entity row")?,
            entity_type: row_str(&row, 2, "entity_type", "entity row")?,
            aliases,
            created_at,
            updated_at,
        })
    }

    /// Count relationships involving an entity (as src or dst).
    pub(super) fn count_relationships(&self, entity_id: &str) -> crate::error::Result<i64> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert("eid".to_owned(), DataValue::Str(entity_id.into()));
        let script = r"?[count(src)] :=
            *relationships{src, dst},
            (src = $eid or dst = $eid)";
        let rows = self.run_read(script, params)?;
        if let Some(row) = rows.rows.first()
            && let Some(val) = row.first()
        {
            return extract_int(val);
        }
        Ok(0)
    }

    /// Count facts linked to an entity via `fact_entities`.
    pub(super) fn count_facts(&self, entity_id: &str) -> crate::error::Result<i64> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert("eid".to_owned(), DataValue::Str(entity_id.into()));
        let script = r"?[count(fact_id)] :=
            *fact_entities{fact_id, entity_id},
            entity_id = $eid";
        let rows = self.run_read(script, params)?;
        if let Some(row) = rows.rows.first()
            && let Some(val) = row.first()
        {
            return extract_int(val);
        }
        Ok(0)
    }

    /// Redirect relationships where merged entity is the source.
    pub(super) fn redirect_relationships_src(
        &self,
        from_id: &crate::id::EntityId,
        to_id: &crate::id::EntityId,
    ) -> crate::error::Result<u32> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(
            "from_id".to_owned(),
            DataValue::Str(from_id.as_str().into()),
        );
        let script = r"?[src, dst, relation, weight, created_at] :=
            *relationships{src, dst, relation, weight, created_at},
            src = $from_id";
        let rows = self.run_read(script, params)?;

        let count = rows.rows.len();
        for row in &rows.rows {
            let dst = row_str(row, 1, "dst", "relationship redirect src")?;
            let relation = row_str(row, 2, "relation", "relationship redirect src")?;
            let weight = row_weight(row, 3, "relationship redirect src")?;
            let created_at = row_str(row, 4, "created_at", "relationship redirect src")?;

            if dst == to_id.as_str() {
                let mut rm_params = BTreeMap::new();
                rm_params.insert("src".to_owned(), DataValue::Str(from_id.as_str().into()));
                rm_params.insert("dst".to_owned(), DataValue::Str(dst.into()));
                // kanon:ignore RUST/no-silent-result-swallow — stale row cleanup after merge; non-fatal if missing
                let _ = self.run_mut(&queries::rm_relationship(), rm_params);
                continue;
            }

            let mut put_params = BTreeMap::new();
            put_params.insert("src".to_owned(), DataValue::Str(to_id.as_str().into()));
            put_params.insert("dst".to_owned(), DataValue::Str(dst.into()));
            put_params.insert("relation".to_owned(), DataValue::Str(relation.into()));
            put_params.insert("weight".to_owned(), DataValue::from(weight));
            put_params.insert("created_at".to_owned(), DataValue::Str(created_at.into()));
            self.run_mut(&queries::upsert_relationship(), put_params)?;

            let mut rm_params = BTreeMap::new();
            rm_params.insert("src".to_owned(), DataValue::Str(from_id.as_str().into()));
            rm_params.insert(
                "dst".to_owned(),
                DataValue::Str(row_str(row, 1, "dst", "relationship redirect src")?.into()),
            );
            // kanon:ignore RUST/no-silent-result-swallow — stale row cleanup after merge; non-fatal if missing
            let _ = self.run_mut(&queries::rm_relationship(), rm_params);
        }

        usize_to_u32(count, "redirected source relationship count")
    }

    /// Redirect relationships where merged entity is the destination.
    pub(super) fn redirect_relationships_dst(
        &self,
        from_id: &crate::id::EntityId,
        to_id: &crate::id::EntityId,
    ) -> crate::error::Result<u32> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(
            "from_id".to_owned(),
            DataValue::Str(from_id.as_str().into()),
        );
        let script = r"?[src, dst, relation, weight, created_at] :=
            *relationships{src, dst, relation, weight, created_at},
            dst = $from_id";
        let rows = self.run_read(script, params)?;

        let count = rows.rows.len();
        for row in &rows.rows {
            let src = row_str(row, 0, "src", "relationship redirect dst")?;
            let relation = row_str(row, 2, "relation", "relationship redirect dst")?;
            let weight = row_weight(row, 3, "relationship redirect dst")?;
            let created_at = row_str(row, 4, "created_at", "relationship redirect dst")?;

            if src == to_id.as_str() {
                let mut rm_params = BTreeMap::new();
                rm_params.insert("src".to_owned(), DataValue::Str(src.into()));
                rm_params.insert("dst".to_owned(), DataValue::Str(from_id.as_str().into()));
                // kanon:ignore RUST/no-silent-result-swallow — stale row cleanup after merge; non-fatal if missing
                let _ = self.run_mut(&queries::rm_relationship(), rm_params);
                continue;
            }

            let mut put_params = BTreeMap::new();
            put_params.insert("src".to_owned(), DataValue::Str(src.into()));
            put_params.insert("dst".to_owned(), DataValue::Str(to_id.as_str().into()));
            put_params.insert("relation".to_owned(), DataValue::Str(relation.into()));
            put_params.insert("weight".to_owned(), DataValue::from(weight));
            put_params.insert("created_at".to_owned(), DataValue::Str(created_at.into()));
            self.run_mut(&queries::upsert_relationship(), put_params)?;

            let mut rm_params = BTreeMap::new();
            rm_params.insert(
                "src".to_owned(),
                DataValue::Str(row_str(row, 0, "src", "relationship redirect dst")?.into()),
            );
            rm_params.insert("dst".to_owned(), DataValue::Str(from_id.as_str().into()));
            // kanon:ignore RUST/no-silent-result-swallow — stale row cleanup after merge; non-fatal if missing
            let _ = self.run_mut(&queries::rm_relationship(), rm_params);
        }

        usize_to_u32(count, "redirected destination relationship count")
    }

    /// Delete an entity from the entities relation.
    pub(super) fn delete_entity(
        &self,
        entity_id: &crate::id::EntityId,
    ) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(entity_id.as_str().into()));
        self.run_mut(&queries::rm_entity(), params)?;
        // WHY: keep the flag relation consistent with the entity lifecycle;
        // flags are review state tied to a specific entity row.
        if let Err(e) = self.clear_entity_flags(entity_id) {
            tracing::warn!(
                entity_id = %entity_id,
                error = %e,
                "failed to clear entity flags during delete"
            );
        }
        // WHY (#4662): deleting an entity removes `entity_type` input for
        // ontological rules. Mark derived materializations stale after the
        // durable delete succeeds.
        self.invalidate_derived_facts()
    }

    /// Flag an entity for operator review.
    #[instrument(skip(self))]
    pub fn flag_entity(
        &self,
        entity_id: &crate::id::EntityId,
        reason: &str,
        severity: &str,
        flagged_by: &str,
    ) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let now = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
        let mut params = BTreeMap::new();
        params.insert(
            "entity_id".to_owned(),
            DataValue::Str(entity_id.as_str().into()),
        );
        params.insert("reason".to_owned(), DataValue::Str(reason.into()));
        params.insert("severity".to_owned(), DataValue::Str(severity.into()));
        params.insert("flagged_by".to_owned(), DataValue::Str(flagged_by.into()));
        params.insert("flagged_at".to_owned(), DataValue::Str(now.into()));
        self.run_mut(&queries::upsert_entity_flag(), params)
    }

    /// Remove any review flags for an entity.
    #[instrument(skip(self))]
    pub fn clear_entity_flags(&self, entity_id: &crate::id::EntityId) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert(
            "entity_id".to_owned(),
            DataValue::Str(entity_id.as_str().into()),
        );
        self.run_mut(&queries::rm_entity_flag(), params)
    }
}

#[cfg(feature = "mneme-engine")]
fn quoted_id_list<'a>(ids: impl Iterator<Item = &'a str>) -> String {
    ids.map(|id| format!("'{}'", id.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(feature = "mneme-engine")]
fn row_value<'a>(
    row: &'a [crate::engine::DataValue],
    index: usize,
    column: &str,
    context: &str,
) -> crate::error::Result<&'a crate::engine::DataValue> {
    row.get(index).ok_or_else(|| {
        crate::error::ConversionSnafu {
            message: format!(
                "{context}: missing {column} column at index {index}; row has {} columns",
                row.len()
            ),
        }
        .build()
    })
}

#[cfg(feature = "mneme-engine")]
fn row_str(
    row: &[crate::engine::DataValue],
    index: usize,
    column: &str,
    context: &str,
) -> crate::error::Result<String> {
    extract_str(row_value(row, index, column, context)?)
}

#[cfg(feature = "mneme-engine")]
fn row_timestamp(
    row: &[crate::engine::DataValue],
    index: usize,
    column: &str,
    context: &str,
) -> crate::error::Result<jiff::Timestamp> {
    let raw = row_str(row, index, column, context)?;
    crate::knowledge::parse_timestamp(&raw).ok_or_else(|| {
        crate::error::EngineQuerySnafu {
            message: format!("{context}: invalid {column} timestamp '{raw}'"),
        }
        .build()
    })
}

#[cfg(feature = "mneme-engine")]
fn row_weight(
    row: &[crate::engine::DataValue],
    index: usize,
    context: &str,
) -> crate::error::Result<f64> {
    let weight = extract_float(row_value(row, index, "weight", context)?)?;
    if !(0.0..=1.0).contains(&weight) {
        return Err(crate::error::EngineQuerySnafu {
            message: format!("{context}: relationship weight out of range: {weight}"),
        }
        .build());
    }
    Ok(weight)
}

#[cfg(feature = "mneme-engine")]
fn parse_aliases(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        Vec::new()
    } else {
        raw.split(',').map(|s| s.trim().to_owned()).collect()
    }
}

#[cfg(feature = "mneme-engine")]
fn checked_u32(value: i64, context: &str) -> crate::error::Result<u32> {
    u32::try_from(value).map_err(|err| {
        crate::error::ConversionSnafu {
            message: format!("{context}: cannot convert {value} to u32: {err}"),
        }
        .build()
    })
}

#[cfg(feature = "mneme-engine")]
fn usize_to_u32(value: usize, context: &str) -> crate::error::Result<u32> {
    u32::try_from(value).map_err(|err| {
        crate::error::ConversionSnafu {
            message: format!("{context}: cannot convert {value} to u32: {err}"),
        }
        .build()
    })
}

#[cfg(feature = "mneme-engine")]
fn decode_entity_row(
    row: &[crate::engine::DataValue],
    context: &str,
) -> crate::error::Result<crate::knowledge::Entity> {
    let id = crate::id::EntityId::new(row_str(row, 0, "id", context)?)
        .context(crate::error::InvalidIdSnafu)?;
    Ok(crate::knowledge::Entity {
        id,
        name: row_str(row, 1, "name", context)?,
        entity_type: row_str(row, 2, "entity_type", context)?,
        aliases: parse_aliases(&row_str(row, 3, "aliases", context)?),
        created_at: row_timestamp(row, 4, "created_at", context)?,
        updated_at: row_timestamp(row, 5, "updated_at", context)?,
    })
}

#[cfg(feature = "mneme-engine")]
fn decode_relationship_row(
    row: &[crate::engine::DataValue],
    context: &str,
) -> crate::error::Result<crate::knowledge::Relationship> {
    Ok(crate::knowledge::Relationship {
        src: crate::id::EntityId::new(row_str(row, 0, "src", context)?)
            .context(crate::error::InvalidIdSnafu)?,
        dst: crate::id::EntityId::new(row_str(row, 1, "dst", context)?)
            .context(crate::error::InvalidIdSnafu)?,
        relation: row_str(row, 2, "relation", context)?,
        weight: row_weight(row, 3, context)?,
        created_at: row_timestamp(row, 4, "created_at", context)?,
    })
}

#[cfg(feature = "mneme-engine")]
fn parse_entities(
    rows: &crate::engine::NamedRows,
) -> crate::error::Result<Vec<crate::knowledge::Entity>> {
    let mut entities = Vec::new();
    for row in &rows.rows {
        entities.push(decode_entity_row(row, "entity row")?);
    }
    Ok(entities)
}

#[cfg(feature = "mneme-engine")]
fn parse_relationships(
    rows: &crate::engine::NamedRows,
) -> crate::error::Result<Vec<crate::knowledge::Relationship>> {
    let mut relationships = Vec::new();
    for row in &rows.rows {
        relationships.push(decode_relationship_row(row, "relationship row")?);
    }
    Ok(relationships)
}
