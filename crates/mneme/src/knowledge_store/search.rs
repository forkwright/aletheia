use super::marshal::{
    build_hybrid_query, embedding_to_params, rows_to_hybrid_results, rows_to_recall_results,
};
use super::{HybridQuery, HybridResult, KnowledgeStore, queries};
use tracing::instrument;

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
        use crate::engine::{Array1, DataValue, Vector};
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert(
            "query_vec".to_owned(),
            DataValue::Vec(Vector::F32(Array1::from(query_vec))),
        );
        params.insert("k".to_owned(), DataValue::from(k));
        params.insert("ef".to_owned(), DataValue::from(ef));

        let rows = self.run_read(queries::SEMANTIC_SEARCH, params)?;
        let results = rows_to_recall_results(rows)?;

        let source_ids: Vec<crate::id::FactId> = results
            .iter()
            .filter(|r| r.source_type == "fact")
            .map(|r| crate::id::FactId::new_unchecked(&r.source_id))
            .collect();
        if let Err(e) = self.increment_access(&source_ids) {
            tracing::warn!(error = %e, "failed to increment access counts");
        }

        Ok(results)
    }

    /// Async `search_vectors` — wraps sync call in `spawn_blocking`.
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

    /// Hybrid BM25 + HNSW vector + graph retrieval fused via `ReciprocalRankFusion`.
    ///
    /// Runs a single Datalog query combining all three signals in the engine.
    /// When `seed_entities` is empty, the graph signal contributes zero to RRF.
    #[instrument(skip(self, q), fields(limit = q.limit, ef = q.ef))]
    pub fn search_hybrid(&self, q: &HybridQuery) -> crate::error::Result<Vec<HybridResult>> {
        use crate::engine::{Array1, DataValue, Vector};
        use std::collections::BTreeMap;

        let mut params = BTreeMap::new();
        params.insert(
            "query_text".to_owned(),
            DataValue::Str(q.text.as_str().into()),
        );
        params.insert(
            "query_vec".to_owned(),
            DataValue::Vec(Vector::F32(Array1::from(q.embedding.clone()))),
        );
        // usize -> i64: limit/ef are user-controlled small values; truncate at i64::MAX for safety
        let limit_i64 = i64::try_from(q.limit).unwrap_or(i64::MAX);
        let ef_i64 = i64::try_from(q.ef).unwrap_or(i64::MAX);
        params.insert("k".to_owned(), DataValue::from(limit_i64));
        params.insert("ef".to_owned(), DataValue::from(ef_i64));
        params.insert("limit".to_owned(), DataValue::from(limit_i64));

        let script = build_hybrid_query(q);
        let rows = self.run_read(&script, params)?;
        let results = rows_to_hybrid_results(rows)?;

        // Filter out forgotten facts — search indices don't carry is_forgotten.
        let results = self.filter_forgotten_results(results)?;

        let fact_ids: Vec<crate::id::FactId> = results.iter().map(|r| r.id.clone()).collect();
        if let Err(e) = self.increment_access(&fact_ids) {
            tracing::warn!(error = %e, "failed to increment access counts");
        }

        Ok(results)
    }

    /// Async `search_hybrid` — wraps sync call in `spawn_blocking`.
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
    pub fn search_enhanced(
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
    pub fn search_tiered(
        &self,
        base_query: &HybridQuery,
        rewriter: &crate::query_rewrite::QueryRewriter,
        provider: &dyn crate::query_rewrite::RewriteProvider,
        context: Option<&str>,
        config: &crate::query_rewrite::TieredSearchConfig,
    ) -> crate::error::Result<crate::query_rewrite::TieredSearchResult<HybridResult>> {
        let start = std::time::Instant::now();

        // Tier 1: Fast path — single-query hybrid search
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

        // Tier 2: Enhanced — LLM query rewrite + multi-query
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

        // Tier 3: Graph-enhanced — expand via entity relationships
        let graph_results = self.expand_via_graph(&enhanced_results, config);
        let final_results = if graph_results.is_empty() {
            enhanced_results
        } else {
            // Merge enhanced + graph results
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
        clippy::cast_precision_loss,
        reason = "rank indices fit in f64 mantissa"
    )]
    #[expect(
        clippy::cast_possible_wrap,
        reason = "rank indices are small positive values"
    )]
    fn expand_via_graph(
        &self,
        existing: &[HybridResult],
        config: &crate::query_rewrite::TieredSearchConfig,
    ) -> Vec<HybridResult> {
        // Collect unique fact IDs from existing results
        let fact_ids: Vec<&str> = existing
            .iter()
            .take(config.graph_expansion_limit)
            .map(|r| r.id.as_str())
            .collect();

        if fact_ids.is_empty() {
            return vec![];
        }

        // For each fact ID, look up which entities it references, then get their neighborhoods
        let mut expanded_ids = std::collections::HashSet::new();
        let existing_ids: std::collections::HashSet<&str> =
            existing.iter().map(|r| r.id.as_str()).collect();

        for fact_id in &fact_ids {
            // Try to find entity connections for this fact by checking entity neighborhoods
            // Use the fact_id as a potential entity_id (facts often share IDs with their subject entities)
            let entity_id = crate::id::EntityId::new_unchecked(*fact_id);
            if let Ok(neighborhood) = self.entity_neighborhood(&entity_id) {
                for row in &neighborhood.rows {
                    // Extract neighbor entity IDs and find their associated facts
                    if let Some(neighbor_id) = row.first().and_then(|v| v.get_str()) {
                        if !existing_ids.contains(neighbor_id) {
                            expanded_ids.insert(neighbor_id.to_owned());
                        }
                    }
                }
            }
        }

        // Create synthetic results for expanded facts with lower base scores
        let mut graph_results = Vec::new();
        for (rank, id) in expanded_ids.iter().enumerate() {
            graph_results.push(HybridResult {
                id: crate::id::FactId::new_unchecked(id.as_str()),
                rrf_score: 1.0 / (60.0 + rank as f64 + 1.0),
                bm25_rank: -1,
                vec_rank: -1,
                graph_rank: (rank + 1) as i64,
            });
        }

        graph_results
    }

    /// Async tiered search — wraps sync call in `spawn_blocking`.
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
    pub fn search_temporal(
        &self,
        q: &HybridQuery,
        at_time: &str,
    ) -> crate::error::Result<Vec<HybridResult>> {
        let all_results = self.search_hybrid(q)?;

        // Get the set of fact IDs valid at the given time
        // We query with an empty nous_id filter to get all facts across all agents
        let valid_facts = self.query_facts_at_time_all(at_time)?;
        let valid_ids: std::collections::HashSet<&str> =
            valid_facts.iter().map(|f| f.id.as_str()).collect();

        let filtered: Vec<HybridResult> = all_results
            .into_iter()
            .filter(|r| valid_ids.contains(r.id.as_str()))
            .collect();

        Ok(filtered)
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
