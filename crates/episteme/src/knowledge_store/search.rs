use snafu::ResultExt;

use super::marshal::{
    build_hybrid_query, embedding_to_params, extract_str, rows_to_hybrid_results,
    rows_to_recall_results,
};
use tracing::instrument;

use super::{HybridQuery, HybridResult, KnowledgeStore, queries};
#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    /// Insert a vector embedding for semantic search.
    #[instrument(skip(self, chunk), fields(chunk_id = %chunk.id))]
    pub fn insert_embedding(
        &self,
        chunk: &crate::knowledge::EmbeddedChunk,
    ) -> crate::error::Result<()> {
        use snafu::ensure;
        ensure!(
            !chunk.content.is_empty(),
            crate::error::EmptyEmbeddingContentSnafu
        );
        ensure!(
            !chunk.embedding.is_empty(),
            crate::error::EmptyEmbeddingSnafu
        );
        // WHY: Validate dimension before storing; a mismatch corrupts the HNSW index.
        ensure!(
            chunk.embedding.len() == self.dim,
            crate::error::EmbeddingDimensionMismatchSnafu {
                expected: self.dim,
                actual: chunk.embedding.len(),
            }
        );
        let params = embedding_to_params(chunk, self.dim);
        self.run_mut(&queries::upsert_embedding(), params)
    }

    /// kNN semantic vector search.
    #[instrument(skip(self, query_vec))]
    pub fn search_vectors(
        &self,
        query_vec: Vec<f32>,
        k: i64,
        ef: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::RecallResult>> {
        use std::collections::BTreeMap;

        use crate::engine::{Array1, DataValue, Vector};
        let mut params = BTreeMap::new();
        params.insert(
            "query_vec".to_owned(),
            DataValue::Vec(Vector::F32(Array1::from(query_vec))),
        );
        // WHY: Over-fetch from HNSW so that post-filtering forgotten facts still
        // yields k results for the caller. Truncated to k after filtering.
        params.insert("k".to_owned(), DataValue::from(k.saturating_mul(2)));
        params.insert("ef".to_owned(), DataValue::from(ef));

        let rows = self.run_read(queries::SEMANTIC_SEARCH, params)?;
        let mut results = rows_to_recall_results(rows)?;

        // WHY: Filter out forgotten facts; the HNSW index does not carry is_forgotten.
        let forgotten_ids = {
            let ids: Vec<&str> = results
                .iter()
                .filter(|r| r.source_type == "fact")
                .map(|r| r.source_id.as_str())
                .collect();
            self.query_forgotten_ids(&ids)?
        };
        if !forgotten_ids.is_empty() {
            results.retain(|r| {
                r.source_type != "fact" || !forgotten_ids.contains(r.source_id.as_str())
            });
        }

        #[expect(
            clippy::cast_sign_loss,
            clippy::as_conversions,
            clippy::cast_possible_truncation,
            reason = "k is a user-supplied positive limit; truncating to usize is safe"
        )]
        results.truncate(k as usize);

        let source_ids: Vec<crate::id::FactId> = results
            .iter()
            .filter(|r| r.source_type == "fact")
            .map(|r| crate::id::FactId::new(&r.source_id))
            .collect::<std::result::Result<Vec<_>, _>>()
            .context(crate::error::InvalidIdSnafu)?;
        if let Err(e) = self.increment_access(&source_ids) {
            tracing::warn!(error = %e, "failed to increment access counts");
        }

        Ok(results)
    }

    /// Async `search_vectors`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self, query_vec))]
    pub async fn search_vectors_async(
        self: &std::sync::Arc<Self>,
        query_vec: Vec<f32>,
        k: i64,
        ef: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::RecallResult>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.search_vectors(query_vec, k, ef))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// BM25 full-text recall: returns `RecallResult` compatible rows without requiring embeddings.
    ///
    /// Used as a fallback when the embedding provider is unavailable or in mock mode.
    /// Distance is the reciprocal of the BM25 score (lower = more relevant).
    pub fn search_text_for_recall(
        &self,
        query_text: &str,
        k: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::RecallResult>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let mut params = BTreeMap::new();
        params.insert("query_text".to_owned(), DataValue::Str(query_text.into()));
        params.insert("k".to_owned(), DataValue::from(k));

        let rows = self.run_read(queries::BM25_RECALL, params)?;
        rows_to_recall_results(rows)
    }

    /// Hybrid BM25 + HNSW vector + graph retrieval fused via `ReciprocalRankFusion`.
    ///
    /// Runs a single Datalog query combining all three signals in the engine.
    /// When `seed_entities` is empty, the graph signal contributes zero to RRF.
    #[instrument(skip(self, q), fields(limit = q.limit, ef = q.ef))]
    pub(crate) fn search_hybrid(&self, q: &HybridQuery) -> crate::error::Result<Vec<HybridResult>> {
        use std::collections::BTreeMap;

        use crate::engine::{Array1, DataValue, Vector};
        let mut params = BTreeMap::new();
        params.insert(
            "query_text".to_owned(),
            DataValue::Str(q.text.as_str().into()),
        );
        params.insert(
            "query_vec".to_owned(),
            DataValue::Vec(Vector::F32(Array1::from(q.embedding.clone()))),
        );
        // NOTE: usize -> i64 cast; limit/ef are user-controlled small values, truncated at i64::MAX.
        let limit_i64 = i64::try_from(q.limit).unwrap_or(i64::MAX);
        let ef_i64 = i64::try_from(q.ef).unwrap_or(i64::MAX);
        // WHY: Over-fetch so that post-filtering forgotten facts still yields
        // limit results for the caller. Truncated after filtering.
        params.insert("k".to_owned(), DataValue::from(limit_i64.saturating_mul(2)));
        params.insert("ef".to_owned(), DataValue::from(ef_i64));
        params.insert(
            "limit".to_owned(),
            DataValue::from(limit_i64.saturating_mul(2)),
        );

        let script = build_hybrid_query(q);
        let rows = self.run_read(&script, params)?;
        let results = rows_to_hybrid_results(rows)?;

        // WHY: Filter out forgotten facts; search indices do not carry is_forgotten.
        let mut results = self.filter_forgotten_results(results)?;
        results.truncate(q.limit);

        let fact_ids: Vec<crate::id::FactId> = results.iter().map(|r| r.id.clone()).collect();
        if let Err(e) = self.increment_access(&fact_ids) {
            tracing::warn!(error = %e, "failed to increment access counts");
        }

        Ok(results)
    }

    /// Async `search_hybrid`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self, q), fields(limit = q.limit, ef = q.ef))]
    pub async fn search_hybrid_async(
        self: &std::sync::Arc<Self>,
        q: HybridQuery,
    ) -> crate::error::Result<Vec<HybridResult>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.search_hybrid(&q))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Multi-query hybrid search: run hybrid search for each query variant,
    /// then merge results via reciprocal rank fusion.
    ///
    /// The `base_query` provides the embedding and search parameters. Each variant
    /// replaces the `text` field for BM25 scoring while reusing the same embedding.
    pub(crate) fn search_enhanced(
        &self,
        base_query: &HybridQuery,
        query_variants: &[String],
    ) -> crate::error::Result<Vec<HybridResult>> {
        use crate::query_rewrite::rrf_merge;

        if query_variants.is_empty() {
            return self.search_hybrid(base_query);
        }

        let mut results_per_query = Vec::with_capacity(query_variants.len());
        for variant in query_variants {
            let mut q = base_query.clone();
            q.text.clone_from(variant);
            match self.search_hybrid(&q) {
                Ok(results) => results_per_query.push(results),
                Err(e) => {
                    tracing::warn!(variant = %variant, error = %e, "search variant failed, skipping");
                }
            }
        }

        if results_per_query.is_empty() {
            return Ok(vec![]);
        }

        Ok(rrf_merge(&results_per_query, 60.0))
    }

    /// Tiered search: fast path -> enhanced -> graph-enhanced.
    ///
    /// Escalates through tiers until sufficient results are found.
    /// Requires a `QueryRewriter` and `RewriteProvider` for tier 2+.
    pub(crate) fn search_tiered(
        &self,
        base_query: &HybridQuery,
        rewriter: &crate::query_rewrite::QueryRewriter,
        provider: &dyn crate::query_rewrite::RewriteProvider,
        context: Option<&str>,
        config: &crate::query_rewrite::TieredSearchConfig,
    ) -> crate::error::Result<crate::query_rewrite::TieredSearchResult<HybridResult>> {
        let start = std::time::Instant::now();

        let fast_results = self.search_hybrid(base_query)?;
        let sufficient = fast_results.len() >= config.fast_path_min_results
            && fast_results
                .iter()
                .any(|r| r.rrf_score >= config.fast_path_score_threshold);

        if sufficient {
            return Ok(crate::query_rewrite::TieredSearchResult {
                tier: crate::query_rewrite::SearchTier::Fast,
                results: fast_results,
                query_variants: None,
                total_latency_ms: start.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            });
        }

        let rewrite_result = rewriter.rewrite(&base_query.text, context, provider);
        let enhanced_results = self.search_enhanced(base_query, &rewrite_result.variants)?;
        let sufficient = enhanced_results.len() >= config.enhanced_min_results
            && enhanced_results
                .iter()
                .any(|r| r.rrf_score >= config.enhanced_score_threshold);

        if sufficient {
            return Ok(crate::query_rewrite::TieredSearchResult {
                tier: crate::query_rewrite::SearchTier::Enhanced,
                results: enhanced_results,
                query_variants: Some(rewrite_result.variants),
                total_latency_ms: start.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            });
        }

        let graph_results = self.expand_via_graph(&enhanced_results, config);
        let final_results = if graph_results.is_empty() {
            enhanced_results
        } else {
            use crate::query_rewrite::rrf_merge;
            rrf_merge(&[enhanced_results, graph_results], 60.0)
        };

        Ok(crate::query_rewrite::TieredSearchResult {
            tier: crate::query_rewrite::SearchTier::GraphEnhanced,
            results: final_results,
            query_variants: Some(rewrite_result.variants),
            total_latency_ms: start.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        })
    }

    /// Expand search results via entity graph neighborhood.
    ///
    /// Takes the top entity IDs from existing results, queries their neighborhoods,
    /// and returns related facts as additional results.
    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "rank indices fit in f64 mantissa"
    )]
    fn expand_via_graph(
        &self,
        existing: &[HybridResult],
        config: &crate::query_rewrite::TieredSearchConfig,
    ) -> Vec<HybridResult> {
        let fact_ids: Vec<&str> = existing
            .iter()
            .take(config.graph_expansion_limit)
            .map(|r| r.id.as_str())
            .collect();

        if fact_ids.is_empty() {
            return vec![];
        }

        let mut expanded_ids = std::collections::HashSet::new();
        let existing_ids: std::collections::HashSet<&str> =
            existing.iter().map(|r| r.id.as_str()).collect();

        for fact_id in &fact_ids {
            // WHY: Query fact_entities by fact_id; FactId and EntityId are distinct types.
            let script = "?[entity_id] := *fact_entities{fact_id: $fid, entity_id}";
            let mut fparams = std::collections::BTreeMap::new();
            fparams.insert(
                "fid".to_owned(),
                crate::engine::DataValue::Str((*fact_id).into()),
            );
            let Ok(entity_rows) = self.run_read(script, fparams) else {
                continue;
            };
            for entity_row in &entity_rows.rows {
                let Some(entity_id_str) = entity_row.first().and_then(|v| v.get_str()) else {
                    continue;
                };
                let Ok(entity_id) = crate::id::EntityId::new(entity_id_str) else {
                    continue;
                };
                if let Ok(neighborhood) = self.entity_neighborhood(&entity_id) {
                    for row in &neighborhood.rows {
                        if let Some(neighbor_id) = row.first().and_then(|v| v.get_str())
                            && !existing_ids.contains(neighbor_id)
                        {
                            expanded_ids.insert(neighbor_id.to_owned());
                        }
                    }
                }
            }
        }

        let mut graph_results = Vec::new();
        for (rank, id) in expanded_ids.iter().enumerate() {
            let Ok(fact_id) = crate::id::FactId::new(id.as_str()) else {
                continue;
            };
            graph_results.push(HybridResult {
                id: fact_id,
                rrf_score: 1.0 / (60.0 + rank as f64 + 1.0), // SAFETY: rank fits f64
                bm25_rank: -1,
                vec_rank: -1,
                graph_rank: i64::try_from(rank + 1).unwrap_or(i64::MAX),
            });
        }

        graph_results
    }

    /// Async tiered search: wraps sync call in `spawn_blocking`.
    ///
    /// Note: the `RewriteProvider` must be `Send + Sync + 'static` for async usage.
    pub async fn search_tiered_async(
        self: &std::sync::Arc<Self>,
        base_query: HybridQuery,
        rewriter: std::sync::Arc<crate::query_rewrite::QueryRewriter>,
        provider: std::sync::Arc<dyn crate::query_rewrite::RewriteProvider>,
        context: Option<String>,
        config: crate::query_rewrite::TieredSearchConfig,
    ) -> crate::error::Result<crate::query_rewrite::TieredSearchResult<HybridResult>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            this.search_tiered(
                &base_query,
                &rewriter,
                provider.as_ref(),
                context.as_deref(),
                &config,
            )
        })
        .await
        .context(crate::error::JoinSnafu)?
    }

    /// Search for facts relevant to a query, as they existed at a specific time.
    /// Filters hybrid search results through the temporal lens.
    pub(crate) fn search_temporal(
        &self,
        q: &HybridQuery,
        at_time: &str,
    ) -> crate::error::Result<Vec<HybridResult>> {
        let all_results = self.search_hybrid(q)?;
        if all_results.is_empty() {
            return Ok(all_results);
        }

        // WHY: Query only the O(k) candidate IDs for temporal validity rather than
        // loading all facts in the store. This replaces the former full-scan via
        // query_facts_at_time_all. The is_forgotten check is also inlined so there
        // is no separate N+1 query for forgotten filtering.
        let candidate_ids: Vec<&str> = all_results.iter().map(|r| r.id.as_str()).collect();
        let valid_ids = self.query_ids_valid_at(at_time, &candidate_ids)?;

        Ok(all_results
            .into_iter()
            .filter(|r| valid_ids.contains(r.id.as_str()))
            .collect())
    }

    /// Return the subset of `ids` that are not forgotten and whose validity
    /// window contains `at_time` (`valid_from <= at_time < valid_to`).
    fn query_ids_valid_at(
        &self,
        at_time: &str,
        ids: &[&str],
    ) -> crate::error::Result<std::collections::HashSet<String>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        if ids.is_empty() {
            return Ok(std::collections::HashSet::new());
        }

        let id_list: Vec<String> = ids
            .iter()
            .map(|id| format!("'{}'", id.replace('\'', "''")))
            .collect();

        let script = format!(
            "?[id] := *facts{{id, valid_from, valid_to, is_forgotten}},
                      is_forgotten == false,
                      valid_from <= $at_time,
                      valid_to > $at_time,
                      id in [{}]",
            id_list.join(", ")
        );

        let mut params = BTreeMap::new();
        params.insert("at_time".to_owned(), DataValue::Str(at_time.into()));

        let rows = self.run_read(&script, params)?;
        let mut result = std::collections::HashSet::new();
        for row in rows.rows {
            if let Some(val) = row.first()
                && let Ok(s) = extract_str(val)
            {
                result.insert(s);
            }
        }
        Ok(result)
    }

    /// Async `search_temporal` wrapper.
    pub async fn search_temporal_async(
        self: &std::sync::Arc<Self>,
        q: HybridQuery,
        at_time: String,
    ) -> crate::error::Result<Vec<HybridResult>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.search_temporal(&q, &at_time))
            .await
            .context(crate::error::JoinSnafu)?
    }
}
