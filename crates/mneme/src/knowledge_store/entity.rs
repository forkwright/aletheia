use super::marshal::{
    entity_to_params, extract_bool, extract_float, extract_int, extract_str, relationship_to_params,
};
use super::{KnowledgeStore, QueryResult, queries};
use tracing::instrument;

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    /// Insert or update an entity.
    #[instrument(skip(self, entity), fields(entity_id = %entity.id))]
    pub fn insert_entity(&self, entity: &crate::knowledge::Entity) -> crate::error::Result<()> {
        use snafu::ensure;
        ensure!(!entity.name.is_empty(), crate::error::EmptyEntityNameSnafu);
        let params = entity_to_params(entity);
        self.run_mut(&queries::upsert_entity(), params)
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

    /// Query 2-hop entity neighborhood.
    ///
    /// Returns a [`QueryResult`] whose rows correspond to the Datalog output of
    /// `ENTITY_NEIGHBORHOOD`. Columns: `id`, `score`, `hops`.
    #[instrument(skip(self))]
    pub fn entity_neighborhood(
        &self,
        entity_id: &crate::id::EntityId,
    ) -> crate::error::Result<QueryResult> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

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
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

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
        self.run_mut(
            r"?[fact_id, entity_id, created_at] <- [[$fact_id, $entity_id, $created_at]]
            :put fact_entities {fact_id, entity_id => created_at}",
            params,
        )
    }

    // --- Entity deduplication ---

    /// Find duplicate entity candidates for a given nous.
    ///
    /// Loads all entities, groups by type, and runs the 3-phase candidate
    /// generation + scoring pipeline. Returns all candidates (auto-merge + review).
    #[instrument(skip(self))]
    pub fn find_duplicate_entities(
        &self,
        nous_id: &str,
    ) -> crate::error::Result<Vec<crate::dedup::EntityMergeCandidate>> {
        let entities = self.load_entity_infos(nous_id)?;
        let candidates = crate::dedup::generate_candidates(&entities, &|_a, _b| 0.0);
        Ok(candidates)
    }

    /// Execute a merge: transfer edges, aliases, `fact_entities`, and record audit.
    ///
    /// The entity with `canonical_id` survives; `merged_id` is removed.
    #[instrument(skip(self))]
    pub fn execute_merge(
        &self,
        canonical_id: &crate::id::EntityId,
        merged_id: &crate::id::EntityId,
    ) -> crate::error::Result<crate::dedup::MergeRecord> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        // 1. Load both entities
        let canonical = self.load_entity(canonical_id)?;
        let merged = self.load_entity(merged_id)?;

        // 2. Redirect relationships: update edges where merged entity is src
        let redirected_src = self.redirect_relationships_src(merged_id, canonical_id)?;
        // Update edges where merged entity is dst
        let redirected_dst = self.redirect_relationships_dst(merged_id, canonical_id)?;
        let relationships_redirected = redirected_src + redirected_dst;

        // 3. Transfer fact_entities mappings
        let facts_transferred = self.transfer_fact_entities(merged_id, canonical_id)?;

        // 4. Add merged entity's name as alias on canonical
        self.add_alias_to_entity(canonical_id, &merged.name)?;

        // 5. Delete merged entity
        self.delete_entity(merged_id)?;

        // 6. Record in merge_audit
        let now = jiff::Timestamp::now();
        let now_str = crate::knowledge::format_timestamp(&now);
        let mut params = BTreeMap::new();
        params.insert(
            "canonical_id".to_owned(),
            DataValue::Str(canonical_id.as_str().into()),
        );
        params.insert(
            "merged_id".to_owned(),
            DataValue::Str(merged_id.as_str().into()),
        );
        params.insert(
            "merged_name".to_owned(),
            DataValue::Str(merged.name.as_str().into()),
        );
        params.insert("merge_score".to_owned(), DataValue::from(0.0_f64));
        params.insert(
            "facts_transferred".to_owned(),
            DataValue::from(i64::from(facts_transferred)),
        );
        params.insert(
            "relationships_redirected".to_owned(),
            DataValue::from(i64::from(relationships_redirected)),
        );
        params.insert("merged_at".to_owned(), DataValue::Str(now_str.into()));
        self.run_mut(
            r"?[canonical_id, merged_id, merged_name, merge_score, facts_transferred, relationships_redirected, merged_at] <- [[
                $canonical_id, $merged_id, $merged_name, $merge_score, $facts_transferred, $relationships_redirected, $merged_at
            ]]
            :put merge_audit {canonical_id, merged_id => merged_name, merge_score, facts_transferred, relationships_redirected, merged_at}",
            params,
        )?;

        // 7. Remove from pending_merges if present
        let mut rm_params = BTreeMap::new();
        rm_params.insert(
            "entity_a".to_owned(),
            DataValue::Str(canonical_id.as_str().into()),
        );
        rm_params.insert(
            "entity_b".to_owned(),
            DataValue::Str(merged_id.as_str().into()),
        );
        // Try both orderings — non-critical cleanup, log on failure
        if let Err(e) = self.run_mut(
            r"?[entity_a, entity_b] <- [[$entity_a, $entity_b]]
            :rm pending_merges {entity_a, entity_b}",
            rm_params,
        ) {
            tracing::warn!(
                %canonical_id, %merged_id, error = %e,
                "failed to remove pending_merges entry (a,b ordering)"
            );
        }
        let mut rm_params2 = BTreeMap::new();
        rm_params2.insert(
            "entity_a".to_owned(),
            DataValue::Str(merged_id.as_str().into()),
        );
        rm_params2.insert(
            "entity_b".to_owned(),
            DataValue::Str(canonical_id.as_str().into()),
        );
        if let Err(e) = self.run_mut(
            r"?[entity_a, entity_b] <- [[$entity_a, $entity_b]]
            :rm pending_merges {entity_a, entity_b}",
            rm_params2,
        ) {
            tracing::warn!(
                %canonical_id, %merged_id, error = %e,
                "failed to remove pending_merges entry (b,a ordering)"
            );
        }

        Ok(crate::dedup::MergeRecord {
            canonical_entity_id: canonical.id,
            merged_entity_id: merged_id.clone(),
            merged_entity_name: merged.name,
            merge_score: 0.0,
            facts_transferred,
            relationships_redirected,
            merged_at: now,
        })
    }

    /// Get pending merge candidates (review queue) for a nous.
    #[instrument(skip(self))]
    #[expect(
        clippy::used_underscore_binding,
        reason = "nous_id reserved for future filtering"
    )]
    pub fn get_pending_merges(
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<Vec<crate::dedup::EntityMergeCandidate>> {
        use std::collections::BTreeMap;

        let script = r"?[entity_a, entity_b, name_a, name_b, name_similarity, embed_similarity, type_match, alias_overlap, merge_score] :=
            *pending_merges{entity_a, entity_b, name_a, name_b, name_similarity, embed_similarity, type_match, alias_overlap, merge_score}";
        let rows = self.run_read(script, BTreeMap::new())?;

        let mut results = Vec::new();
        for row in &rows.rows {
            if row.len() < 9 {
                continue;
            }
            results.push(crate::dedup::EntityMergeCandidate {
                entity_a: crate::id::EntityId::new_unchecked(extract_str(&row[0])?),
                entity_b: crate::id::EntityId::new_unchecked(extract_str(&row[1])?),
                name_a: extract_str(&row[2])?,
                name_b: extract_str(&row[3])?,
                name_similarity: extract_float(&row[4])?,
                embed_similarity: extract_float(&row[5])?,
                type_match: extract_bool(&row[6])?,
                alias_overlap: extract_bool(&row[7])?,
                merge_score: extract_float(&row[8])?,
            });
        }
        Ok(results)
    }

    /// Approve a pending merge — execute it.
    #[instrument(skip(self))]
    pub fn approve_merge(
        &self,
        canonical_id: &crate::id::EntityId,
        merged_id: &crate::id::EntityId,
    ) -> crate::error::Result<crate::dedup::MergeRecord> {
        self.execute_merge(canonical_id, merged_id)
    }

    /// Get the full merge history.
    #[instrument(skip(self))]
    #[expect(
        clippy::used_underscore_binding,
        reason = "nous_id reserved for future filtering"
    )]
    pub fn get_merge_history(
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<Vec<crate::dedup::MergeRecord>> {
        use std::collections::BTreeMap;

        let script = r"?[canonical_id, merged_id, merged_name, merge_score, facts_transferred, relationships_redirected, merged_at] :=
            *merge_audit{canonical_id, merged_id, merged_name, merge_score, facts_transferred, relationships_redirected, merged_at}";
        let rows = self.run_read(script, BTreeMap::new())?;

        let mut results = Vec::new();
        for row in &rows.rows {
            if row.len() < 7 {
                continue;
            }
            let merged_at = crate::knowledge::parse_timestamp(&extract_str(&row[6])?)
                .unwrap_or_else(jiff::Timestamp::now);
            results.push(crate::dedup::MergeRecord {
                canonical_entity_id: crate::id::EntityId::new_unchecked(extract_str(&row[0])?),
                merged_entity_id: crate::id::EntityId::new_unchecked(extract_str(&row[1])?),
                merged_entity_name: extract_str(&row[2])?,
                merge_score: extract_float(&row[3])?,
                facts_transferred: u32::try_from(extract_int(&row[4])?).unwrap_or(0),
                relationships_redirected: u32::try_from(extract_int(&row[5])?).unwrap_or(0),
                merged_at,
            });
        }
        Ok(results)
    }

    /// Run the full entity deduplication pipeline for a nous.
    ///
    /// 1. Generate candidates
    /// 2. Classify into auto-merge vs review
    /// 3. Execute auto-merges, store review candidates as pending
    ///
    /// Returns the list of completed merge records.
    #[instrument(skip(self))]
    pub fn run_entity_dedup(
        &self,
        nous_id: &str,
    ) -> crate::error::Result<Vec<crate::dedup::MergeRecord>> {
        let entities = self.load_entity_infos(nous_id)?;
        if entities.is_empty() {
            return Ok(Vec::new());
        }

        let candidates = crate::dedup::generate_candidates(&entities, &|_a, _b| 0.0);
        let (auto_merge, review) = crate::dedup::classify_candidates(candidates);

        // Store review candidates
        for c in &review {
            self.store_pending_merge(c)?;
        }

        // Execute auto-merges
        let entity_map: std::collections::HashMap<&str, &crate::dedup::EntityInfo> =
            entities.iter().map(|e| (e.id.as_str(), e)).collect();

        let mut records = Vec::new();
        let mut merged_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        for c in &auto_merge {
            // Skip if either entity was already merged in this run
            if merged_ids.contains(c.entity_a.as_str()) || merged_ids.contains(c.entity_b.as_str())
            {
                continue;
            }

            let info_a = entity_map.get(c.entity_a.as_str());
            let info_b = entity_map.get(c.entity_b.as_str());

            if let (Some(a), Some(b)) = (info_a, info_b) {
                let (canonical, merged_info) = crate::dedup::pick_canonical(a, b);
                match self.execute_merge(&canonical.id, &merged_info.id) {
                    Ok(mut record) => {
                        record.merge_score = c.merge_score;
                        merged_ids.insert(merged_info.id.as_str().to_owned());
                        records.push(record);
                    }
                    Err(e) => {
                        tracing::warn!(
                            canonical = %canonical.id,
                            merged = %merged_info.id,
                            error = %e,
                            "entity merge failed, skipping"
                        );
                    }
                }
            }
        }

        Ok(records)
    }

    // --- Internal entity dedup helpers ---

    /// Load all entities as lightweight `EntityInfo` structs.
    pub(super) fn load_entity_infos(
        &self,
        _nous_id: &str,
    ) -> crate::error::Result<Vec<crate::dedup::EntityInfo>> {
        use std::collections::BTreeMap;

        let script = r"?[id, name, entity_type, aliases, created_at] :=
            *entities{id, name, entity_type, aliases, created_at}";
        let rows = self.run_read(script, BTreeMap::new())?;

        let mut entities = Vec::new();
        for row in &rows.rows {
            if row.len() < 5 {
                continue;
            }
            let id_str = extract_str(&row[0])?;
            let name = extract_str(&row[1])?;
            let entity_type = extract_str(&row[2])?;
            let aliases_str = extract_str(&row[3])?;
            let aliases: Vec<String> = if aliases_str.is_empty() {
                Vec::new()
            } else {
                aliases_str
                    .split(',')
                    .map(|s| s.trim().to_owned())
                    .collect()
            };
            let created_at = crate::knowledge::parse_timestamp(&extract_str(&row[4])?)
                .unwrap_or_else(jiff::Timestamp::now);

            // Count relationships for this entity
            let rel_count = self.count_relationships(&id_str)?;

            entities.push(crate::dedup::EntityInfo {
                id: crate::id::EntityId::new_unchecked(&id_str),
                name,
                entity_type,
                aliases,
                relationship_count: u32::try_from(rel_count).unwrap_or(0),
                created_at,
            });
        }
        Ok(entities)
    }

    /// Load a single entity by ID.
    pub(super) fn load_entity(
        &self,
        entity_id: &crate::id::EntityId,
    ) -> crate::error::Result<crate::knowledge::Entity> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

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

        let aliases_str = extract_str(&row[3])?;
        let aliases: Vec<String> = if aliases_str.is_empty() {
            Vec::new()
        } else {
            aliases_str
                .split(',')
                .map(|s| s.trim().to_owned())
                .collect()
        };

        let created_at = crate::knowledge::parse_timestamp(&extract_str(&row[4])?)
            .unwrap_or_else(jiff::Timestamp::now);
        let updated_at = crate::knowledge::parse_timestamp(&extract_str(&row[5])?)
            .unwrap_or_else(jiff::Timestamp::now);

        Ok(crate::knowledge::Entity {
            id: entity_id.clone(),
            name: extract_str(&row[1])?,
            entity_type: extract_str(&row[2])?,
            aliases,
            created_at,
            updated_at,
        })
    }

    /// Count relationships involving an entity (as src or dst).
    fn count_relationships(&self, entity_id: &str) -> crate::error::Result<i64> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

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

    /// Redirect relationships where merged entity is the source.
    fn redirect_relationships_src(
        &self,
        from_id: &crate::id::EntityId,
        to_id: &crate::id::EntityId,
    ) -> crate::error::Result<u32> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        // Read all relationships where src = from_id
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
            if row.len() < 5 {
                continue;
            }
            let dst = extract_str(&row[1])?;
            let relation = extract_str(&row[2])?;
            let weight = extract_float(&row[3])?;
            let created_at = extract_str(&row[4])?;

            // Skip self-referential edges that would be created
            if dst == to_id.as_str() {
                // Remove the old edge only
                let mut rm_params = BTreeMap::new();
                rm_params.insert("src".to_owned(), DataValue::Str(from_id.as_str().into()));
                rm_params.insert("dst".to_owned(), DataValue::Str(dst.into()));
                let _ = self.run_mut(
                    r"?[src, dst] <- [[$src, $dst]] :rm relationships {src, dst}",
                    rm_params,
                );
                continue;
            }

            // Insert redirected edge
            let mut put_params = BTreeMap::new();
            put_params.insert("src".to_owned(), DataValue::Str(to_id.as_str().into()));
            put_params.insert("dst".to_owned(), DataValue::Str(dst.into()));
            put_params.insert("relation".to_owned(), DataValue::Str(relation.into()));
            put_params.insert("weight".to_owned(), DataValue::from(weight));
            put_params.insert("created_at".to_owned(), DataValue::Str(created_at.into()));
            self.run_mut(
                r"?[src, dst, relation, weight, created_at] <- [[$src, $dst, $relation, $weight, $created_at]]
                :put relationships {src, dst => relation, weight, created_at}",
                put_params,
            )?;

            // Remove old edge
            let mut rm_params = BTreeMap::new();
            rm_params.insert("src".to_owned(), DataValue::Str(from_id.as_str().into()));
            rm_params.insert(
                "dst".to_owned(),
                DataValue::Str(extract_str(&row[1])?.into()),
            );
            let _ = self.run_mut(
                r"?[src, dst] <- [[$src, $dst]] :rm relationships {src, dst}",
                rm_params,
            );
        }

        Ok(u32::try_from(count).unwrap_or(0))
    }

    /// Redirect relationships where merged entity is the destination.
    fn redirect_relationships_dst(
        &self,
        from_id: &crate::id::EntityId,
        to_id: &crate::id::EntityId,
    ) -> crate::error::Result<u32> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

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
            if row.len() < 5 {
                continue;
            }
            let src = extract_str(&row[0])?;
            let relation = extract_str(&row[2])?;
            let weight = extract_float(&row[3])?;
            let created_at = extract_str(&row[4])?;

            // Skip self-referential
            if src == to_id.as_str() {
                let mut rm_params = BTreeMap::new();
                rm_params.insert("src".to_owned(), DataValue::Str(src.into()));
                rm_params.insert("dst".to_owned(), DataValue::Str(from_id.as_str().into()));
                let _ = self.run_mut(
                    r"?[src, dst] <- [[$src, $dst]] :rm relationships {src, dst}",
                    rm_params,
                );
                continue;
            }

            // Insert redirected edge
            let mut put_params = BTreeMap::new();
            put_params.insert("src".to_owned(), DataValue::Str(src.into()));
            put_params.insert("dst".to_owned(), DataValue::Str(to_id.as_str().into()));
            put_params.insert("relation".to_owned(), DataValue::Str(relation.into()));
            put_params.insert("weight".to_owned(), DataValue::from(weight));
            put_params.insert("created_at".to_owned(), DataValue::Str(created_at.into()));
            self.run_mut(
                r"?[src, dst, relation, weight, created_at] <- [[$src, $dst, $relation, $weight, $created_at]]
                :put relationships {src, dst => relation, weight, created_at}",
                put_params,
            )?;

            // Remove old edge
            let mut rm_params = BTreeMap::new();
            rm_params.insert(
                "src".to_owned(),
                DataValue::Str(extract_str(&row[0])?.into()),
            );
            rm_params.insert("dst".to_owned(), DataValue::Str(from_id.as_str().into()));
            let _ = self.run_mut(
                r"?[src, dst] <- [[$src, $dst]] :rm relationships {src, dst}",
                rm_params,
            );
        }

        Ok(u32::try_from(count).unwrap_or(0))
    }

    /// Transfer `fact_entities` mappings from merged entity to canonical.
    fn transfer_fact_entities(
        &self,
        from_id: &crate::id::EntityId,
        to_id: &crate::id::EntityId,
    ) -> crate::error::Result<u32> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert(
            "from_id".to_owned(),
            DataValue::Str(from_id.as_str().into()),
        );
        let script = r"?[fact_id, entity_id, created_at] :=
            *fact_entities{fact_id, entity_id, created_at},
            entity_id = $from_id";
        let rows = self.run_read(script, params)?;

        let count = rows.rows.len();
        for row in &rows.rows {
            if row.len() < 3 {
                continue;
            }
            let fact_id = extract_str(&row[0])?;
            let created_at = extract_str(&row[2])?;

            // Insert mapping to canonical
            let mut put_params = BTreeMap::new();
            put_params.insert(
                "fact_id".to_owned(),
                DataValue::Str(fact_id.as_str().into()),
            );
            put_params.insert(
                "entity_id".to_owned(),
                DataValue::Str(to_id.as_str().into()),
            );
            put_params.insert("created_at".to_owned(), DataValue::Str(created_at.into()));
            self.run_mut(
                r"?[fact_id, entity_id, created_at] <- [[$fact_id, $entity_id, $created_at]]
                :put fact_entities {fact_id, entity_id => created_at}",
                put_params,
            )?;

            // Remove old mapping
            let mut rm_params = BTreeMap::new();
            rm_params.insert("fact_id".to_owned(), DataValue::Str(fact_id.into()));
            rm_params.insert(
                "entity_id".to_owned(),
                DataValue::Str(from_id.as_str().into()),
            );
            let _ = self.run_mut(
                r"?[fact_id, entity_id] <- [[$fact_id, $entity_id]]
                :rm fact_entities {fact_id, entity_id}",
                rm_params,
            );
        }

        Ok(u32::try_from(count).unwrap_or(0))
    }

    /// Add an alias to an entity's alias list.
    fn add_alias_to_entity(
        &self,
        entity_id: &crate::id::EntityId,
        new_alias: &str,
    ) -> crate::error::Result<()> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let entity = self.load_entity(entity_id)?;
        let lower_new = new_alias.to_lowercase();

        // Skip if already present (case-insensitive) or same as current name
        if entity.name.to_lowercase() == lower_new
            || entity.aliases.iter().any(|a| a.to_lowercase() == lower_new)
        {
            return Ok(());
        }

        let mut aliases = entity.aliases;
        aliases.push(new_alias.to_owned());
        let aliases_str = aliases.join(",");

        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(entity_id.as_str().into()));
        params.insert("aliases".to_owned(), DataValue::Str(aliases_str.into()));
        params.insert(
            "updated_at".to_owned(),
            DataValue::Str(crate::knowledge::format_timestamp(&jiff::Timestamp::now()).into()),
        );
        // Read current fields first to preserve them
        params.insert("name".to_owned(), DataValue::Str(entity.name.into()));
        params.insert(
            "entity_type".to_owned(),
            DataValue::Str(entity.entity_type.into()),
        );
        params.insert(
            "created_at".to_owned(),
            DataValue::Str(crate::knowledge::format_timestamp(&entity.created_at).into()),
        );
        self.run_mut(
            r"?[id, name, entity_type, aliases, created_at, updated_at] <- [[
                $id, $name, $entity_type, $aliases, $created_at, $updated_at
            ]]
            :put entities {id => name, entity_type, aliases, created_at, updated_at}",
            params,
        )
    }

    /// Delete an entity from the entities relation.
    fn delete_entity(&self, entity_id: &crate::id::EntityId) -> crate::error::Result<()> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(entity_id.as_str().into()));
        self.run_mut(r"?[id] <- [[$id]] :rm entities {id}", params)
    }

    /// Store a pending merge candidate for review.
    fn store_pending_merge(
        &self,
        candidate: &crate::dedup::EntityMergeCandidate,
    ) -> crate::error::Result<()> {
        use crate::engine::DataValue;
        use std::collections::BTreeMap;

        let now = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
        let mut params = BTreeMap::new();
        params.insert(
            "entity_a".to_owned(),
            DataValue::Str(candidate.entity_a.as_str().into()),
        );
        params.insert(
            "entity_b".to_owned(),
            DataValue::Str(candidate.entity_b.as_str().into()),
        );
        params.insert(
            "name_a".to_owned(),
            DataValue::Str(candidate.name_a.as_str().into()),
        );
        params.insert(
            "name_b".to_owned(),
            DataValue::Str(candidate.name_b.as_str().into()),
        );
        params.insert(
            "name_similarity".to_owned(),
            DataValue::from(candidate.name_similarity),
        );
        params.insert(
            "embed_similarity".to_owned(),
            DataValue::from(candidate.embed_similarity),
        );
        params.insert(
            "type_match".to_owned(),
            DataValue::Bool(candidate.type_match),
        );
        params.insert(
            "alias_overlap".to_owned(),
            DataValue::Bool(candidate.alias_overlap),
        );
        params.insert(
            "merge_score".to_owned(),
            DataValue::from(candidate.merge_score),
        );
        params.insert("created_at".to_owned(), DataValue::Str(now.into()));
        self.run_mut(
            r"?[entity_a, entity_b, name_a, name_b, name_similarity, embed_similarity, type_match, alias_overlap, merge_score, created_at] <- [[
                $entity_a, $entity_b, $name_a, $name_b, $name_similarity, $embed_similarity, $type_match, $alias_overlap, $merge_score, $created_at
            ]]
            :put pending_merges {entity_a, entity_b => name_a, name_b, name_similarity, embed_similarity, type_match, alias_overlap, merge_score, created_at}",
            params,
        )
    }
}
