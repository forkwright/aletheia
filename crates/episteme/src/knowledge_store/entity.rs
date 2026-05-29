#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use snafu::ResultExt;

use super::marshal::{
    entity_to_params, extract_bool, extract_float, extract_int, extract_str, relationship_to_params,
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "entity operations for knowledge store")
    )]
    pub(crate) fn insert_fact_entity(
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

        let mut entities = Vec::new();
        for row in &rows.rows {
            if row.len() < 6 {
                continue;
            }
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

            let id = crate::id::EntityId::new(extract_str(&row[0])?)
                .context(crate::error::InvalidIdSnafu)?;
            entities.push(crate::knowledge::Entity {
                id,
                name: extract_str(&row[1])?,
                entity_type: extract_str(&row[2])?,
                aliases,
                created_at,
                updated_at,
            });
        }
        Ok(entities)
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

        let mut relationships = Vec::new();
        for row in &rows.rows {
            if row.len() < 5 {
                continue;
            }
            let src = crate::id::EntityId::new(extract_str(&row[0])?)
                .context(crate::error::InvalidIdSnafu)?;
            let dst = crate::id::EntityId::new(extract_str(&row[1])?)
                .context(crate::error::InvalidIdSnafu)?;
            let relation = extract_str(&row[2])?;
            let weight = extract_float(&row[3])?;
            let created_at = crate::knowledge::parse_timestamp(&extract_str(&row[4])?)
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

    /// Find duplicate entity candidates for a given nous using default tuning.
    ///
    /// Convenience wrapper around
    /// [`find_duplicate_entities_with_tuning`](Self::find_duplicate_entities_with_tuning)
    /// for callers that have no operator-configured
    /// [`DedupTuning`](crate::dedup::DedupTuning) in scope.
    #[instrument(skip(self))]
    pub fn find_duplicate_entities(
        &self,
        nous_id: &str,
    ) -> crate::error::Result<Vec<crate::dedup::EntityMergeCandidate>> {
        self.find_duplicate_entities_with_tuning(nous_id, &crate::dedup::DedupTuning::DEFAULT)
    }

    /// Find duplicate entity candidates for a given nous under explicit tuning.
    ///
    /// Loads all entities scoped to `nous_id` (via the `fact_entities` →
    /// `facts.nous_id` join in
    /// [`load_entity_infos`](Self::load_entity_infos)), groups by type, and
    /// runs the 3-phase candidate generation + scoring pipeline. Returns
    /// all candidates (auto-merge + review).
    ///
    /// Cosine similarity is computed over the entities' cached
    /// `name_embedding` column (schema v13+). Entities without a stored
    /// embedding contribute `embed_sim = 0.0` for any pair they participate
    /// in — i.e. the pre-#4165 behaviour for degraded-mode installs.
    /// Callers that want to populate embeddings first should use
    /// [`KnowledgeStore::backfill_entity_name_embeddings`].
    ///
    /// `tuning` provides operator-configurable weights and thresholds
    /// (#4165 D); pass [`DedupTuning::DEFAULT`](crate::dedup::DedupTuning::DEFAULT)
    /// for the historical defaults.
    #[instrument(skip(self, tuning))]
    pub fn find_duplicate_entities_with_tuning(
        &self,
        nous_id: &str,
        tuning: &crate::dedup::DedupTuning,
    ) -> crate::error::Result<Vec<crate::dedup::EntityMergeCandidate>> {
        let entities = self.load_entity_infos(nous_id)?;
        let embed_lookup = crate::dedup::make_embedding_lookup(&entities);
        let candidates = crate::dedup::generate_candidates(&entities, &embed_lookup, tuning);
        Ok(candidates)
    }

    /// Execute a merge: transfer edges, aliases, `fact_entities`, and record audit.
    ///
    /// The entity with `canonical_id` survives; `merged_id` is removed.
    #[instrument(skip(self))]
    pub(crate) fn execute_merge(
        &self,
        canonical_id: &crate::id::EntityId,
        merged_id: &crate::id::EntityId,
    ) -> crate::error::Result<crate::dedup::MergeRecord> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let canonical = self.load_entity(canonical_id)?;
        let merged = self.load_entity(merged_id)?;

        let redirected_src = self.redirect_relationships_src(merged_id, canonical_id)?;
        let redirected_dst = self.redirect_relationships_dst(merged_id, canonical_id)?;
        let relationships_redirected = redirected_src + redirected_dst;

        let facts_transferred = self.transfer_fact_entities(merged_id, canonical_id)?;

        self.add_alias_to_entity(canonical_id, &merged.name)?;

        self.delete_entity(merged_id)?;

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
        self.run_mut(&queries::put_merge_audit(), params)?;

        let mut rm_params = BTreeMap::new();
        rm_params.insert(
            "entity_a".to_owned(),
            DataValue::Str(canonical_id.as_str().into()),
        );
        rm_params.insert(
            "entity_b".to_owned(),
            DataValue::Str(merged_id.as_str().into()),
        );
        // WHY: Try both orderings; pending_merges may store (a,b) or (b,a).
        if let Err(e) = self.run_mut(&queries::rm_pending_merges(), rm_params) {
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
        if let Err(e) = self.run_mut(&queries::rm_pending_merges(), rm_params2) {
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
            let entity_a = crate::id::EntityId::new(extract_str(&row[0])?)
                .context(crate::error::InvalidIdSnafu)?;
            let entity_b = crate::id::EntityId::new(extract_str(&row[1])?)
                .context(crate::error::InvalidIdSnafu)?;
            results.push(crate::dedup::EntityMergeCandidate {
                entity_a,
                entity_b,
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

    /// Approve a pending merge by executing it.
    ///
    /// Drains a candidate that
    /// [`KnowledgeStore::run_entity_dedup`] left in the `pending_merges`
    /// queue (score in `[0.70, 0.90)`); the operator picks which side
    /// survives and `execute_merge` redirects edges, transfers
    /// `fact_entities`, preserves the merged name as an alias, and clears
    /// the pending-merge row (#4165 Path A).
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
            let canonical_entity_id = crate::id::EntityId::new(extract_str(&row[0])?)
                .context(crate::error::InvalidIdSnafu)?;
            let merged_entity_id = crate::id::EntityId::new(extract_str(&row[1])?)
                .context(crate::error::InvalidIdSnafu)?;
            results.push(crate::dedup::MergeRecord {
                canonical_entity_id,
                merged_entity_id,
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
    ///
    /// Cosine similarity is computed over each entity's cached
    /// `name_embedding` column (schema v13+) — entities without a stored
    /// embedding contribute `embed_sim = 0.0`. Callers that want
    /// `AutoMerge` to be reachable in production (the design weights
    /// `embed_sim` at 0.30 and the `AutoMerge` threshold is 0.90) should
    /// populate embeddings first via
    /// [`KnowledgeStore::run_entity_dedup_with_embeddings`] or
    /// [`KnowledgeStore::backfill_entity_name_embeddings`].
    #[instrument(skip(self))]
    pub fn run_entity_dedup(
        &self,
        nous_id: &str,
    ) -> crate::error::Result<Vec<crate::dedup::MergeRecord>> {
        self.run_entity_dedup_with_tuning(nous_id, &crate::dedup::DedupTuning::DEFAULT)
    }

    /// Execute the dedup pipeline under the supplied `tuning`.
    ///
    /// Same semantics as [`run_entity_dedup`](Self::run_entity_dedup) but
    /// with operator-configurable weights and thresholds (#4165 D). CLI
    /// and maintenance callers build a [`DedupTuning`](crate::dedup::DedupTuning)
    /// from `taxis::config::AgentBehaviorDefaults::knowledge_dedup_*` and
    /// pass it through so config knobs actually take effect.
    #[instrument(skip(self, tuning))]
    pub fn run_entity_dedup_with_tuning(
        &self,
        nous_id: &str,
        tuning: &crate::dedup::DedupTuning,
    ) -> crate::error::Result<Vec<crate::dedup::MergeRecord>> {
        let entities = self.load_entity_infos(nous_id)?;
        if entities.is_empty() {
            return Ok(Vec::new());
        }

        let embed_lookup = crate::dedup::make_embedding_lookup(&entities);
        let candidates = crate::dedup::generate_candidates(&entities, &embed_lookup, tuning);
        let (auto_merge, review) = crate::dedup::classify_candidates(candidates, tuning);

        for c in &review {
            self.store_pending_merge(c)?;
        }

        let entity_map: std::collections::HashMap<&str, &crate::dedup::EntityInfo> =
            entities.iter().map(|e| (e.id.as_str(), e)).collect();

        let mut records = Vec::new();
        let mut merged_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        for c in &auto_merge {
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

    /// Set (or clear) the `name_embedding` for a single entity.
    ///
    /// Writes only the embedding column — the entity's name, type,
    /// aliases, and timestamps are preserved as-is. Pass `None` to clear
    /// a stored embedding, or `Some(vec)` to install one whose length
    /// matches the configured `KnowledgeConfig::dim`.
    ///
    /// Returns
    /// `Err(EmbeddingDimensionMismatch)` if the supplied vector's length
    /// does not match `self.dim` (a wrong-dim write would corrupt the
    /// stored column type and silently break subsequent dedup runs).
    ///
    /// Wires the at-creation half of the #4165 Path A lifecycle: callers
    /// in the extraction pipeline that hold an
    /// [`EmbeddingProvider`](crate::embedding::EmbeddingProvider) compute
    /// the name embedding once and call this method directly after
    /// `insert_entity`.
    #[instrument(skip(self, name_embedding), fields(entity_id = %entity_id))]
    pub fn update_entity_name_embedding(
        &self,
        entity_id: &crate::id::EntityId,
        name_embedding: Option<Vec<f32>>,
    ) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{Array1, DataValue, Vector};

        if let Some(ref v) = name_embedding {
            snafu::ensure!(
                v.len() == self.dim,
                crate::error::EmbeddingDimensionMismatchSnafu {
                    expected: self.dim,
                    actual: v.len(),
                }
            );
        }

        let entity = self.load_entity(entity_id)?;
        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(entity_id.as_str().into()));
        params.insert(
            "name".to_owned(),
            DataValue::Str(entity.name.as_str().into()),
        );
        params.insert(
            "entity_type".to_owned(),
            DataValue::Str(entity.entity_type.as_str().into()),
        );
        params.insert(
            "aliases".to_owned(),
            DataValue::Str(entity.aliases.join(",").into()),
        );
        params.insert(
            "created_at".to_owned(),
            DataValue::Str(crate::knowledge::format_timestamp(&entity.created_at).into()),
        );
        params.insert(
            "updated_at".to_owned(),
            DataValue::Str(crate::knowledge::format_timestamp(&jiff::Timestamp::now()).into()),
        );
        let emb_value = name_embedding.map_or(DataValue::Null, |v| {
            DataValue::Vec(Vector::F32(Array1::from(v)))
        });
        params.insert("name_embedding".to_owned(), emb_value);
        self.run_mut(&queries::upsert_entity(), params)
    }

    /// Read the stored `name_embedding` for a single entity, if any.
    ///
    /// Returns `Ok(None)` when the column is NULL (never populated, or
    /// the entity predates the v13 migration). Returns `Err` when the
    /// entity does not exist.
    #[instrument(skip(self), fields(entity_id = %entity_id))]
    pub fn get_entity_name_embedding(
        &self,
        entity_id: &crate::id::EntityId,
    ) -> crate::error::Result<Option<Vec<f32>>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(entity_id.as_str().into()));
        let script = r"?[name_embedding] :=
            *entities{id, name_embedding},
            id = $id";
        let rows = self.run_read(script, params)?;
        let row = rows.rows.into_iter().next().ok_or_else(|| {
            crate::error::EngineQuerySnafu {
                message: format!("entity not found: {entity_id}"),
            }
            .build()
        })?;
        super::marshal::extract_optional_f32_vec(&row[0])
    }

    /// Populate `name_embedding` for entities whose column is NULL.
    ///
    /// Walks every entity returned by [`load_entity_infos`], embeds those
    /// whose `name_embedding` is `None` via `provider`, and writes the
    /// result back through [`update_entity_name_embedding`]. Returns the
    /// number of rows that were filled in.
    ///
    /// This is the lazy half of the #4165 Path A lifecycle: degraded-mode
    /// installs and rows that predate v13 stay at `embed_sim = 0.0` until
    /// a future dedup run is invoked with a provider in scope. Individual
    /// embedding failures are logged and counted but do not abort the
    /// scan — partial backfill is more useful than no backfill when only
    /// a subset of names trips the embedding model.
    ///
    /// [`load_entity_infos`]: Self::load_entity_infos
    /// [`update_entity_name_embedding`]: Self::update_entity_name_embedding
    #[instrument(skip(self, provider))]
    pub fn backfill_entity_name_embeddings(
        &self,
        provider: &dyn crate::embedding::EmbeddingProvider,
        nous_id: &str,
    ) -> crate::error::Result<u64> {
        // WHY: avoid backfilling against a degraded sentinel — every
        // call would return an error and inflate the failure counter
        // without making the dedup pipeline any more accurate.
        if crate::embedding::is_degraded_provider(provider) {
            tracing::warn!(
                nous_id,
                "backfill_entity_name_embeddings: provider is degraded; skipping"
            );
            return Ok(0);
        }

        let entities = self.load_entity_infos(nous_id)?;
        let mut filled: u64 = 0;
        let mut failures: u32 = 0;
        for e in &entities {
            if e.name_embedding.is_some() {
                continue;
            }
            match provider.embed(&e.name) {
                Ok(vec) => {
                    if let Err(err) = self.update_entity_name_embedding(&e.id, Some(vec)) {
                        tracing::warn!(
                            entity_id = %e.id,
                            error = %err,
                            "backfill: failed to write name_embedding"
                        );
                        failures = failures.saturating_add(1);
                    } else {
                        filled = filled.saturating_add(1);
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        entity_id = %e.id,
                        error = %err,
                        "backfill: failed to embed entity name"
                    );
                    failures = failures.saturating_add(1);
                }
            }
        }
        if failures > 0 {
            tracing::info!(
                filled,
                failures,
                "backfill_entity_name_embeddings complete (with failures)"
            );
        }
        Ok(filled)
    }

    /// Run the dedup pipeline after backfilling missing name embeddings.
    ///
    /// When `provider` is `Some`, calls
    /// [`backfill_entity_name_embeddings`](Self::backfill_entity_name_embeddings)
    /// to populate any NULL `name_embedding`s before delegating to
    /// [`run_entity_dedup`](Self::run_entity_dedup). When `provider` is
    /// `None`, behaves identically to `run_entity_dedup` — degraded-mode
    /// installs continue to produce review-tier candidates only, since
    /// the maximum composite score without embeddings is 0.70 (#4165).
    ///
    /// Backfill failure (e.g. the provider rate-limited) is non-fatal:
    /// the dedup scan still runs over whatever embeddings did land, so
    /// callers always get *some* progress. Whichever entities were
    /// successfully embedded contribute real `embed_sim` values; the rest
    /// stay at 0.0 for this pass.
    #[instrument(skip(self, provider))]
    pub fn run_entity_dedup_with_embeddings(
        &self,
        nous_id: &str,
        provider: Option<&dyn crate::embedding::EmbeddingProvider>,
    ) -> crate::error::Result<Vec<crate::dedup::MergeRecord>> {
        self.run_entity_dedup_with_embeddings_and_tuning(
            nous_id,
            provider,
            &crate::dedup::DedupTuning::DEFAULT,
        )
    }

    /// Backfill embeddings (if `provider` is `Some`) then run dedup under
    /// the supplied `tuning`.
    ///
    /// This is the entry point the scheduled `entity-dedup` maintenance
    /// task uses so that operator-configured
    /// [`AgentBehaviorDefaults::knowledge_dedup_*`](https://docs.rs/taxis)
    /// knobs actually flow through the merge decision (#4165 D).
    #[instrument(skip(self, provider, tuning))]
    pub fn run_entity_dedup_with_embeddings_and_tuning(
        &self,
        nous_id: &str,
        provider: Option<&dyn crate::embedding::EmbeddingProvider>,
        tuning: &crate::dedup::DedupTuning,
    ) -> crate::error::Result<Vec<crate::dedup::MergeRecord>> {
        if let Some(p) = provider
            && let Err(e) = self.backfill_entity_name_embeddings(p, nous_id)
        {
            tracing::warn!(
                nous_id,
                error = %e,
                "backfill_entity_name_embeddings failed; falling back to embedded-or-null dedup"
            );
        }
        self.run_entity_dedup_with_tuning(nous_id, tuning)
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
            if row.len() < 6 {
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
            let name_embedding = super::marshal::extract_optional_f32_vec(&row[5])?;

            let rel_count = self.count_relationships(&id_str)?;

            let id = crate::id::EntityId::new(&id_str).context(crate::error::InvalidIdSnafu)?;
            entities.push(crate::dedup::EntityInfo {
                id,
                name,
                entity_type,
                aliases,
                relationship_count: u32::try_from(rel_count).unwrap_or(0),
                created_at,
                name_embedding,
            });
        }
        Ok(entities)
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

    /// Redirect relationships where merged entity is the source.
    fn redirect_relationships_src(
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
            if row.len() < 5 {
                continue;
            }
            let dst = extract_str(&row[1])?;
            let relation = extract_str(&row[2])?;
            let weight = extract_float(&row[3])?;
            let created_at = extract_str(&row[4])?;

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
                DataValue::Str(extract_str(&row[1])?.into()),
            );
            // kanon:ignore RUST/no-silent-result-swallow — stale row cleanup after merge; non-fatal if missing
            let _ = self.run_mut(&queries::rm_relationship(), rm_params);
        }

        Ok(u32::try_from(count).unwrap_or(0))
    }

    /// Redirect relationships where merged entity is the destination.
    fn redirect_relationships_dst(
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
            if row.len() < 5 {
                continue;
            }
            let src = extract_str(&row[0])?;
            let relation = extract_str(&row[2])?;
            let weight = extract_float(&row[3])?;
            let created_at = extract_str(&row[4])?;

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
                DataValue::Str(extract_str(&row[0])?.into()),
            );
            rm_params.insert("dst".to_owned(), DataValue::Str(from_id.as_str().into()));
            // kanon:ignore RUST/no-silent-result-swallow — stale row cleanup after merge; non-fatal if missing
            let _ = self.run_mut(&queries::rm_relationship(), rm_params);
        }

        Ok(u32::try_from(count).unwrap_or(0))
    }

    /// Transfer `fact_entities` mappings from merged entity to canonical.
    fn transfer_fact_entities(
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
            self.run_mut(&queries::upsert_fact_entity(), put_params)?;

            let mut rm_params = BTreeMap::new();
            rm_params.insert("fact_id".to_owned(), DataValue::Str(fact_id.into()));
            rm_params.insert(
                "entity_id".to_owned(),
                DataValue::Str(from_id.as_str().into()),
            );
            // kanon:ignore RUST/no-silent-result-swallow — stale row cleanup after merge; non-fatal if missing
            let _ = self.run_mut(&queries::rm_fact_entity(), rm_params);
        }

        Ok(u32::try_from(count).unwrap_or(0))
    }

    /// Add an alias to an entity's alias list.
    fn add_alias_to_entity(
        &self,
        entity_id: &crate::id::EntityId,
        new_alias: &str,
    ) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::{Array1, DataValue, Vector};
        let entity = self.load_entity(entity_id)?;
        let lower_new = new_alias.to_lowercase();

        if entity.name.to_lowercase() == lower_new
            || entity.aliases.iter().any(|a| a.to_lowercase() == lower_new)
        {
            return Ok(());
        }

        let mut aliases = entity.aliases;
        aliases.push(new_alias.to_owned());
        let aliases_str = aliases.join(",");

        // WHY (#4165 Path A): the entities upsert now also requires
        // `$name_embedding` — preserve the existing column value so the
        // alias update does not silently clear a previously-populated
        // embedding.
        let existing_embedding = self.get_entity_name_embedding(entity_id)?;
        let emb_value = existing_embedding.map_or(DataValue::Null, |v| {
            DataValue::Vec(Vector::F32(Array1::from(v)))
        });

        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(entity_id.as_str().into()));
        params.insert("aliases".to_owned(), DataValue::Str(aliases_str.into()));
        params.insert(
            "updated_at".to_owned(),
            DataValue::Str(crate::knowledge::format_timestamp(&jiff::Timestamp::now()).into()),
        );
        params.insert("name".to_owned(), DataValue::Str(entity.name.into()));
        params.insert(
            "entity_type".to_owned(),
            DataValue::Str(entity.entity_type.into()),
        );
        params.insert(
            "created_at".to_owned(),
            DataValue::Str(crate::knowledge::format_timestamp(&entity.created_at).into()),
        );
        params.insert("name_embedding".to_owned(), emb_value);
        self.run_mut(&queries::upsert_entity(), params)
    }

    /// Delete an entity from the entities relation.
    fn delete_entity(&self, entity_id: &crate::id::EntityId) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert("id".to_owned(), DataValue::Str(entity_id.as_str().into()));
        self.run_mut(&queries::rm_entity(), params)
    }

    /// Store a pending merge candidate for review.
    fn store_pending_merge(
        &self,
        candidate: &crate::dedup::EntityMergeCandidate,
    ) -> crate::error::Result<()> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
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
        self.run_mut(&queries::put_pending_merge(), params)
    }
}
