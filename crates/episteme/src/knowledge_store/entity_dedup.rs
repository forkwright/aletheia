#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use snafu::ResultExt;
use tracing::instrument;

use super::entity_dedup_support::{checked_u32, short_row_error, strict_timestamp};
use super::marshal::{extract_bool, extract_float, extract_int, extract_str};
use super::{KnowledgeStore, queries};

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    /// Find duplicate entity candidates for a given nous using default tuning.
    ///
    /// Convenience wrapper around
    /// [`find_duplicate_entities_with_tuning`](Self::find_duplicate_entities_with_tuning)
    /// for callers that have no operator-configured
    /// [`DedupTuning`](crate::dedup::DedupTuning) in scope.
    #[instrument(skip(self))]
    #[cfg_attr(not(test), expect(dead_code, reason = "test-only convenience wrapper"))]
    pub(crate) fn find_duplicate_entities(
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
    // kanon:ignore RUST/pub-visibility — consumed by the aletheia CLI memory dedup command
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
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "low-level merge primitive is exercised by property tests"
        )
    )]
    pub(crate) fn execute_merge(
        &self,
        canonical_id: &crate::id::EntityId,
        merged_id: &crate::id::EntityId,
    ) -> crate::error::Result<crate::dedup::MergeRecord> {
        self.execute_merge_for_nous(super::UNOWNED_MERGE_NOUS_ID, canonical_id, merged_id, 0.0)
    }

    #[instrument(skip(self))]
    pub(super) fn execute_merge_for_nous(
        &self,
        nous_id: &str,
        canonical_id: &crate::id::EntityId,
        merged_id: &crate::id::EntityId,
        merge_score: f64,
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
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
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
        params.insert("merge_score".to_owned(), DataValue::from(merge_score));
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
        rm_params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
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
        rm_params2.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
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
            merge_score,
            facts_transferred,
            relationships_redirected,
            merged_at: now,
        })
    }

    /// Get pending merge candidates (review queue) for a nous.
    #[instrument(skip(self))]
    // kanon:ignore RUST/pub-visibility — consumed by the aletheia CLI pending-review commands
    pub fn get_pending_merges(
        &self,
        nous_id: &str,
    ) -> crate::error::Result<Vec<crate::dedup::EntityMergeCandidate>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let script = r"?[entity_a, entity_b, name_a, name_b, name_similarity, embed_similarity, type_match, alias_overlap, merge_score] :=
            *pending_merges{nous_id: $nous_id, entity_a, entity_b, name_a, name_b, name_similarity, embed_similarity, type_match, alias_overlap, merge_score}";
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        let rows = self.run_read(script, params)?;

        let mut results = Vec::new();
        for row in &rows.rows {
            if row.len() < 9 {
                return Err(short_row_error("pending merge row", 9, row.len()));
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
    // kanon:ignore RUST/pub-visibility — consumed by aletheia CLI and pylon knowledge entity API
    pub fn approve_merge(
        &self,
        canonical_id: &crate::id::EntityId,
        merged_id: &crate::id::EntityId,
    ) -> crate::error::Result<crate::dedup::MergeRecord> {
        let nous_id = self.unique_pending_merge_nous_id(canonical_id, merged_id)?;
        self.approve_merge_for_nous(&nous_id, canonical_id, merged_id)
    }

    /// Approve a pending merge for the requesting nous.
    #[instrument(skip(self))]
    pub(crate) fn approve_merge_for_nous(
        &self,
        nous_id: &str,
        canonical_id: &crate::id::EntityId,
        merged_id: &crate::id::EntityId,
    ) -> crate::error::Result<crate::dedup::MergeRecord> {
        let Some(merge_score) = self.pending_merge_score(nous_id, canonical_id, merged_id)? else {
            return Err(crate::error::EngineQuerySnafu {
                message: format!(
                    "pending merge not found for nous_id '{nous_id}': {canonical_id} + {merged_id}"
                ),
            }
            .build());
        };
        self.ensure_merge_pair_belongs_to_nous(nous_id, canonical_id, merged_id)?;
        self.execute_merge_for_nous(nous_id, canonical_id, merged_id, merge_score)
    }

    fn unique_pending_merge_nous_id(
        &self,
        canonical_id: &crate::id::EntityId,
        merged_id: &crate::id::EntityId,
    ) -> crate::error::Result<String> {
        let ids = self.pending_merge_nous_ids(canonical_id, merged_id)?;
        match ids.as_slice() {
            [nous_id] => Ok(nous_id.clone()),
            [] => Err(crate::error::EngineQuerySnafu {
                message: format!("pending merge not found: {canonical_id} + {merged_id}"),
            }
            .build()),
            _ => Err(crate::error::EngineQuerySnafu {
                message: format!(
                    "pending merge is ambiguous across nouses: {canonical_id} + {merged_id}"
                ),
            }
            .build()),
        }
    }

    fn pending_merge_nous_ids(
        &self,
        canonical_id: &crate::id::EntityId,
        merged_id: &crate::id::EntityId,
    ) -> crate::error::Result<Vec<String>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let script = r"
            ?[nous_id] :=
                *pending_merges{nous_id, entity_a: $entity_a, entity_b: $entity_b}
            ?[nous_id] :=
                *pending_merges{nous_id, entity_a: $entity_b, entity_b: $entity_a}
        ";
        let mut params = BTreeMap::new();
        params.insert(
            "entity_a".to_owned(),
            DataValue::Str(canonical_id.as_str().into()),
        );
        params.insert(
            "entity_b".to_owned(),
            DataValue::Str(merged_id.as_str().into()),
        );
        let rows = self.run_read(script, params)?;
        let mut ids = Vec::new();
        for row in &rows.rows {
            if let Some(id) = row.first().and_then(crate::engine::DataValue::get_str) {
                ids.push(id.to_owned());
            }
        }
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    fn pending_merge_score(
        &self,
        nous_id: &str,
        canonical_id: &crate::id::EntityId,
        merged_id: &crate::id::EntityId,
    ) -> crate::error::Result<Option<f64>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let script = r"
            ?[merge_score] :=
                *pending_merges{nous_id: $nous_id, entity_a: $entity_a, entity_b: $entity_b, merge_score}
            ?[merge_score] :=
                *pending_merges{nous_id: $nous_id, entity_a: $entity_b, entity_b: $entity_a, merge_score}
        ";
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert(
            "entity_a".to_owned(),
            DataValue::Str(canonical_id.as_str().into()),
        );
        params.insert(
            "entity_b".to_owned(),
            DataValue::Str(merged_id.as_str().into()),
        );
        let rows = self.run_read(script, params)?;
        rows.rows
            .first()
            .map(|row| {
                let val = row.first().ok_or_else(|| {
                    crate::error::EngineQuerySnafu {
                        message: "pending merge score row is empty".to_owned(),
                    }
                    .build()
                })?;
                extract_float(val)
            })
            .transpose()
    }

    fn ensure_merge_pair_belongs_to_nous(
        &self,
        nous_id: &str,
        canonical_id: &crate::id::EntityId,
        merged_id: &crate::id::EntityId,
    ) -> crate::error::Result<()> {
        for entity_id in [canonical_id, merged_id] {
            if !self.entity_belongs_to_nous(nous_id, entity_id)? {
                return Err(crate::error::EngineQuerySnafu {
                    message: format!("entity {entity_id} is not linked to nous_id '{nous_id}'"),
                }
                .build());
            }
        }
        Ok(())
    }

    pub(super) fn common_nous_ids_for_entity_pair(
        &self,
        entity_a: &str,
        entity_b: &str,
    ) -> crate::error::Result<Vec<String>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let script = r"
            ?[nous_id] :=
                *fact_entities{fact_id: fact_a, entity_id: $entity_a},
                *facts{id: fact_a, nous_id},
                *fact_entities{fact_id: fact_b, entity_id: $entity_b},
                *facts{id: fact_b, nous_id}
        ";
        let mut params = BTreeMap::new();
        params.insert("entity_a".to_owned(), DataValue::Str(entity_a.into()));
        params.insert("entity_b".to_owned(), DataValue::Str(entity_b.into()));
        let rows = self.run_read(script, params)?;
        let mut ids = Vec::new();
        for row in &rows.rows {
            if let Some(id) = row.first().and_then(crate::engine::DataValue::get_str) {
                ids.push(id.to_owned());
            }
        }
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    fn entity_belongs_to_nous(
        &self,
        nous_id: &str,
        entity_id: &crate::id::EntityId,
    ) -> crate::error::Result<bool> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let script = r"?[count(fact_id)] :=
            *facts{id: fact_id, nous_id: $nous_id},
            *fact_entities{fact_id, entity_id: $entity_id}";
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        params.insert(
            "entity_id".to_owned(),
            DataValue::Str(entity_id.as_str().into()),
        );
        let rows = self.run_read(script, params)?;
        let count = rows
            .rows
            .first()
            .ok_or_else(|| short_row_error("entity ownership count", 1, 0))?
            .first()
            .ok_or_else(|| short_row_error("entity ownership count", 1, 0))
            .and_then(extract_int)?;
        Ok(count > 0)
    }

    /// Get the full merge history.
    #[instrument(skip(self))]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "test-only merge history reader")
    )]
    pub(crate) fn get_merge_history(
        &self,
        nous_id: &str,
    ) -> crate::error::Result<Vec<crate::dedup::MergeRecord>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let script = r"?[canonical_id, merged_id, merged_name, merge_score, facts_transferred, relationships_redirected, merged_at] :=
            *merge_audit{nous_id: $nous_id, canonical_id, merged_id, merged_name, merge_score, facts_transferred, relationships_redirected, merged_at}";
        let mut params = BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));
        let rows = self.run_read(script, params)?;

        let mut results = Vec::new();
        for row in &rows.rows {
            if row.len() < 7 {
                return Err(short_row_error("merge audit row", 7, row.len()));
            }
            let merged_at = strict_timestamp(&extract_str(&row[6])?, "merge audit merged_at")?;
            let canonical_entity_id = crate::id::EntityId::new(extract_str(&row[0])?)
                .context(crate::error::InvalidIdSnafu)?;
            let merged_entity_id = crate::id::EntityId::new(extract_str(&row[1])?)
                .context(crate::error::InvalidIdSnafu)?;
            results.push(crate::dedup::MergeRecord {
                canonical_entity_id,
                merged_entity_id,
                merged_entity_name: extract_str(&row[2])?,
                merge_score: extract_float(&row[3])?,
                facts_transferred: checked_u32(
                    extract_int(&row[4])?,
                    "merge audit facts_transferred",
                )?,
                relationships_redirected: checked_u32(
                    extract_int(&row[5])?,
                    "merge audit relationships_redirected",
                )?,
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
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "test-only default-tuning wrapper")
    )]
    pub(crate) fn run_entity_dedup(
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
    // kanon:ignore RUST/pub-visibility — consumed by the aletheia CLI memory dedup command
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
            self.store_pending_merge(nous_id, c)?;
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
                match self.execute_merge_for_nous(
                    nous_id,
                    &canonical.id,
                    &merged_info.id,
                    c.merge_score,
                ) {
                    Ok(record) => {
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
    pub(crate) fn update_entity_name_embedding(
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
    pub(crate) fn get_entity_name_embedding(
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
        let val = row.first().ok_or_else(|| {
            crate::error::EngineQuerySnafu {
                message: format!("embedding row for {entity_id} is empty"),
            }
            .build()
        })?;
        super::marshal::extract_optional_f32_vec(val)
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
    pub(crate) fn backfill_entity_name_embeddings(
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
    #[expect(dead_code, reason = "crate-local default-tuning wrapper")]
    pub(crate) fn run_entity_dedup_with_embeddings(
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
    // kanon:ignore RUST/pub-visibility — consumed by aletheia maintenance scheduling
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
}
