use snafu::ResultExt;

use super::marshal::{
    build_hybrid_query, build_scoped_hybrid_query, embedding_to_params, rows_to_hybrid_results,
    rows_to_recall_results, sanitize_fts_query,
};
use tracing::instrument;

use super::{HybridQuery, HybridResult, KnowledgeStore, queries};

#[cfg(feature = "mneme-engine")]
fn truncate_recall_results(results: &mut Vec<crate::knowledge::RecallResult>, k: i64) {
    let limit = usize::try_from(k.max(0)).unwrap_or(usize::MAX);
    results.truncate(limit);
}

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    /// Insert a vector embedding for semantic search.
    ///
    /// # Complexity
    ///
    /// O(log n * `ef_construction`) where n is index size and `ef_construction` is the
    /// HNSW construction beam width. Space: O(`dim`) for the vector storage.
    #[instrument(skip(self, chunk), fields(chunk_id = %chunk.id))]
    // kanon:ignore RUST/pub-visibility — consumed by aletheia ingestion and integration-test crates
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
    ///
    /// # Complexity
    ///
    /// O(log n * ef + k) where n is index size, ef is search beam width, k is results.
    /// The k factor includes post-filtering for forgotten facts.
    #[instrument(skip(self, query_vec))]
    // kanon:ignore RUST/pub-visibility — consumed by aletheia CLI and integration-test crates
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

        // WHY: Semantic search returns from the embeddings relation, which does
        // not carry scope, visibility, or sensitivity. Hydrate these fields from
        // the facts table for fact-type results so downstream filters see
        // accurate values.
        self.hydrate_recall_scope_visibility(&mut results);

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

        // WHY (#5663): load GraphContext once and share across enrichment + cluster expansion.
        let graph_ctx = self.load_graph_context_for_recall();
        self.enrich_recall_results(&mut results, &graph_ctx);
        self.enrich_source_counts(&mut results);
        // WHY (#5559): extract nous_id from the seed results so cluster expansion
        // cannot inject facts from other nouses in the same cohort store.
        let requester_nous_id_for_cluster: Option<String> = results
            .iter()
            .find(|r| r.source_type == "fact" && !r.nous_id.is_empty())
            .map(|r| r.nous_id.clone());
        self.expand_recall_by_cluster(
            &mut results,
            k,
            requester_nous_id_for_cluster.as_deref(),
            Some(&graph_ctx),
        )?;

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

    /// kNN semantic vector search scoped to facts visible to `requester_nous_id`.
    ///
    /// Visibility is enforced inside the query before result hydration, cluster
    /// expansion, or final truncation so foreign private facts cannot influence
    /// returned scores or expansion seeds.
    #[instrument(skip(self, query_vec))]
    // kanon:ignore RUST/pub-visibility — consumed by nous recall search integration
    pub fn search_vectors_scoped(
        &self,
        query_vec: Vec<f32>,
        k: i64,
        ef: i64,
        requester_nous_id: &str,
    ) -> crate::error::Result<Vec<crate::knowledge::RecallResult>> {
        use std::collections::BTreeMap;

        use crate::engine::{Array1, DataValue, Vector};
        let mut params = BTreeMap::new();
        params.insert(
            "query_vec".to_owned(),
            DataValue::Vec(Vector::F32(Array1::from(query_vec))),
        );
        params.insert("k".to_owned(), DataValue::from(k.saturating_mul(2)));
        params.insert("ef".to_owned(), DataValue::from(ef));
        params.insert(
            "requester_nous_id".to_owned(),
            DataValue::Str(requester_nous_id.into()),
        );

        let script = r"
            ?[id, content, source_type, source_id, dist, scope, project_id, visibility, nous_id, sensitivity] :=
                ~embeddings:semantic_idx {id: embedding_id, content: _embedding_content, source_type, source_id |
                    query: $query_vec, k: $k, ef: $ef, bind_distance: dist},
                source_type == 'fact',
                *facts{id: source_id, content, is_forgotten, superseded_by, scope, project_id, visibility, nous_id, sensitivity},
                nous_id == $requester_nous_id,
                is_forgotten == false,
                is_null(superseded_by),
                id = source_id
            ?[id, content, source_type, source_id, dist, scope, project_id, visibility, nous_id, sensitivity] :=
                ~embeddings:semantic_idx {id: embedding_id, content: _embedding_content, source_type, source_id |
                    query: $query_vec, k: $k, ef: $ef, bind_distance: dist},
                source_type == 'fact',
                *facts{id: source_id, content, is_forgotten, superseded_by, scope, project_id, visibility, nous_id, sensitivity},
                visibility == 'shared',
                is_forgotten == false,
                is_null(superseded_by),
                id = source_id
            ?[id, content, source_type, source_id, dist, scope, project_id, visibility, nous_id, sensitivity] :=
                ~embeddings:semantic_idx {id: embedding_id, content: _embedding_content, source_type, source_id |
                    query: $query_vec, k: $k, ef: $ef, bind_distance: dist},
                source_type == 'fact',
                *facts{id: source_id, content, is_forgotten, superseded_by, scope, project_id, visibility, nous_id, sensitivity},
                visibility == 'published',
                is_forgotten == false,
                is_null(superseded_by),
                id = source_id
            :order dist
            :limit $k
            ";
        let mut results = rows_to_recall_results(self.run_read(script, params)?)?;
        // WHY (#5663): load GraphContext once and share across enrichment + cluster expansion.
        let graph_ctx = self.load_graph_context_for_recall();
        self.enrich_recall_results(&mut results, &graph_ctx);
        self.enrich_source_counts(&mut results);
        self.expand_recall_by_cluster_scoped(&mut results, k, requester_nous_id, Some(&graph_ctx))?;
        truncate_recall_results(&mut results, k);
        self.increment_recall_access(&results);
        Ok(results)
    }

    /// Async `search_vectors`: wraps sync call in `spawn_blocking`.
    ///
    /// # Complexity
    ///
    /// Same as `search_vectors`: O(log n * ef + k).
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

    /// Async `search_vectors_scoped`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self, query_vec))]
    pub async fn search_vectors_scoped_async(
        self: &std::sync::Arc<Self>,
        query_vec: Vec<f32>,
        k: i64,
        ef: i64,
        requester_nous_id: String,
    ) -> crate::error::Result<Vec<crate::knowledge::RecallResult>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            this.search_vectors_scoped(query_vec, k, ef, &requester_nous_id)
        })
        .await
        .context(crate::error::JoinSnafu)?
    }

    /// BM25 full-text recall: returns `RecallResult` compatible rows without requiring embeddings.
    ///
    /// Used as a fallback when the embedding provider is unavailable or in mock mode.
    /// Distance is the reciprocal of the BM25 score (lower = more relevant).
    ///
    /// # Complexity
    ///
    /// O(Q * (log T + D) + D log D) where Q is query terms, T is unique terms,
    /// D is matching documents. BM25 scoring adds O(D) and ranking adds O(D log D).
    // kanon:ignore RUST/pub-visibility — consumed by diaporeia MCP tools and integration-test crates
    pub fn search_text_for_recall(
        &self,
        query_text: &str,
        k: i64,
    ) -> crate::error::Result<Vec<crate::knowledge::RecallResult>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        // WHY: bind sanitized bare terms, not raw user text — see sanitize_fts_query (#4156).
        // A text-only BM25 search with no terms cannot match anything and would be an
        // FTS parse error, so return empty rather than running an empty FTS query.
        let sanitized_text = sanitize_fts_query(query_text);
        if sanitized_text.is_empty() {
            return Ok(Vec::new());
        }
        let mut params = BTreeMap::new();
        params.insert(
            "query_text".to_owned(),
            DataValue::Str(sanitized_text.into()),
        );
        params.insert("k".to_owned(), DataValue::from(k));

        let rows = self.run_read(queries::BM25_RECALL, params)?;
        let mut results = rows_to_recall_results(rows)?;
        // WHY (#5663): load GraphContext once and share across enrichment + cluster expansion.
        let graph_ctx = self.load_graph_context_for_recall();
        self.enrich_recall_results(&mut results, &graph_ctx);
        self.enrich_source_counts(&mut results);
        // WHY (#5559): extract nous_id from the seed results so cluster expansion
        // cannot inject facts from other nouses in the same cohort store.
        let requester_nous_id_for_cluster: Option<String> = results
            .iter()
            .find(|r| r.source_type == "fact" && !r.nous_id.is_empty())
            .map(|r| r.nous_id.clone());
        self.expand_recall_by_cluster(
            &mut results,
            k,
            requester_nous_id_for_cluster.as_deref(),
            Some(&graph_ctx),
        )?;
        Ok(results)
    }

    /// BM25 full-text recall scoped to facts visible to `requester_nous_id`.
    // kanon:ignore RUST/pub-visibility — consumed by nous recall and aletheia-memory-mcp
    pub fn search_text_for_recall_scoped(
        &self,
        query_text: &str,
        k: i64,
        requester_nous_id: &str,
    ) -> crate::error::Result<Vec<crate::knowledge::RecallResult>> {
        use std::collections::BTreeMap;

        use crate::engine::DataValue;
        let sanitized_text = sanitize_fts_query(query_text);
        if sanitized_text.is_empty() {
            return Ok(Vec::new());
        }
        let mut params = BTreeMap::new();
        params.insert(
            "query_text".to_owned(),
            DataValue::Str(sanitized_text.into()),
        );
        params.insert("k".to_owned(), DataValue::from(k));
        params.insert(
            "requester_nous_id".to_owned(),
            DataValue::Str(requester_nous_id.into()),
        );

        let script = r"
            ?[id, content, source_type, source_id, dist, scope, project_id, visibility, nous_id, sensitivity] :=
                ~facts:content_fts{id | query: $query_text, k: $k, score_kind: 'bm25', bind_score: bm25_score},
                *facts{id, content, is_forgotten, superseded_by, scope, project_id, visibility, nous_id, sensitivity},
                nous_id == $requester_nous_id,
                is_forgotten == false,
                is_null(superseded_by),
                source_type = 'fact',
                source_id = id,
                dist = 1.0 / bm25_score
            ?[id, content, source_type, source_id, dist, scope, project_id, visibility, nous_id, sensitivity] :=
                ~facts:content_fts{id | query: $query_text, k: $k, score_kind: 'bm25', bind_score: bm25_score},
                *facts{id, content, is_forgotten, superseded_by, scope, project_id, visibility, nous_id, sensitivity},
                visibility == 'shared',
                is_forgotten == false,
                is_null(superseded_by),
                source_type = 'fact',
                source_id = id,
                dist = 1.0 / bm25_score
            ?[id, content, source_type, source_id, dist, scope, project_id, visibility, nous_id, sensitivity] :=
                ~facts:content_fts{id | query: $query_text, k: $k, score_kind: 'bm25', bind_score: bm25_score},
                *facts{id, content, is_forgotten, superseded_by, scope, project_id, visibility, nous_id, sensitivity},
                visibility == 'published',
                is_forgotten == false,
                is_null(superseded_by),
                source_type = 'fact',
                source_id = id,
                dist = 1.0 / bm25_score
            :order dist
            :limit $k
            ";
        let mut results = rows_to_recall_results(self.run_read(script, params)?)?;
        // WHY (#5663): load GraphContext once and share across enrichment + cluster expansion.
        let graph_ctx = self.load_graph_context_for_recall();
        self.enrich_recall_results(&mut results, &graph_ctx);
        self.enrich_source_counts(&mut results);
        self.expand_recall_by_cluster_scoped(&mut results, k, requester_nous_id, Some(&graph_ctx))?;
        truncate_recall_results(&mut results, k);
        Ok(results)
    }

    /// Hybrid BM25 + HNSW vector + graph retrieval fused via `ReciprocalRankFusion`.
    ///
    /// Runs a single Datalog query combining all three signals in the engine.
    /// When `seed_entities` is empty, the graph signal contributes zero to RRF.
    ///
    /// # Complexity
    ///
    /// O(log n * ef + Q*(log T + D) + G + R) where: n is HNSW size, ef is beam width,
    /// Q is query terms, T is unique terms, D is BM25 matches, G is graph neighbors,
    /// R is RRF merge cost (linear in result count).
    #[instrument(skip(self, q), fields(limit = q.limit, ef = q.ef))]
    pub(crate) fn search_hybrid(&self, q: &HybridQuery) -> crate::error::Result<Vec<HybridResult>> {
        use std::collections::BTreeMap;

        use crate::engine::{Array1, DataValue, Vector};
        let mut params = BTreeMap::new();
        // WHY: bind sanitized bare terms, not raw user text — see sanitize_fts_query (#4156).
        // Only bind $query_text when terms remain; otherwise build_hybrid_query emits an
        // empty `bm25` relation and the unreferenced param must be omitted.
        let sanitized_text = sanitize_fts_query(q.text.as_str());
        if !sanitized_text.is_empty() {
            params.insert(
                "query_text".to_owned(),
                DataValue::Str(sanitized_text.into()),
            );
        }
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

    /// Hybrid search scoped to facts visible to `requester_nous_id`.
    #[instrument(skip(self, q), fields(limit = q.limit, ef = q.ef))]
    pub(crate) fn search_hybrid_scoped(
        &self,
        q: &HybridQuery,
        requester_nous_id: &str,
    ) -> crate::error::Result<Vec<HybridResult>> {
        use std::collections::BTreeMap;

        use crate::engine::{Array1, DataValue, Vector};
        let mut params = BTreeMap::new();
        let sanitized_text = sanitize_fts_query(q.text.as_str());
        if !sanitized_text.is_empty() {
            params.insert(
                "query_text".to_owned(),
                DataValue::Str(sanitized_text.into()),
            );
        }
        params.insert(
            "query_vec".to_owned(),
            DataValue::Vec(Vector::F32(Array1::from(q.embedding.clone()))),
        );
        let limit_i64 = i64::try_from(q.limit).unwrap_or(i64::MAX);
        let ef_i64 = i64::try_from(q.ef).unwrap_or(i64::MAX);
        params.insert("k".to_owned(), DataValue::from(limit_i64.saturating_mul(2)));
        params.insert("ef".to_owned(), DataValue::from(ef_i64));
        params.insert(
            "limit".to_owned(),
            DataValue::from(limit_i64.saturating_mul(2)),
        );
        params.insert(
            "requester_nous_id".to_owned(),
            DataValue::Str(requester_nous_id.into()),
        );

        let script = build_scoped_hybrid_query(q);
        let rows = self.run_read(&script, params)?;
        let mut results = rows_to_hybrid_results(rows)?;

        // WHY (#5846): The BM25, vector, and graph indices do not carry the
        // `is_forgotten` flag. Filter out forgotten facts before truncating so
        // soft-deleted entries never surface in scoped recall results.
        results = self.filter_forgotten_results(results)?;
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

    /// Async `search_hybrid_scoped`: wraps sync call in `spawn_blocking`.
    #[instrument(skip(self, q), fields(limit = q.limit, ef = q.ef))]
    pub async fn search_hybrid_scoped_async(
        self: &std::sync::Arc<Self>,
        q: HybridQuery,
        requester_nous_id: String,
    ) -> crate::error::Result<Vec<HybridResult>> {
        use snafu::ResultExt;
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || this.search_hybrid_scoped(&q, &requester_nous_id))
            .await
            .context(crate::error::JoinSnafu)?
    }

    /// Multi-query hybrid search: run hybrid search for each query variant,
    /// then merge results via reciprocal rank fusion.
    ///
    /// The `base_query` provides the embedding and search parameters. Each variant
    /// replaces the `text` field for BM25 scoring while reusing the same embedding.
    ///
    /// # Complexity
    ///
    /// O(V * (log n * ef + Q*(log T + D))) where V is query variants. RRF merge
    /// across variants is O(V * R log R) where R is results per variant.
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
            return Err(crate::error::EnhancedSearchSnafu {
                message: format!("{} variants failed", query_variants.len()),
            }
            .build());
        }

        Ok(rrf_merge(&results_per_query, 60.0))
    }

    /// Multi-query hybrid search scoped to facts visible to `requester_nous_id`.
    pub(crate) fn search_enhanced_scoped(
        &self,
        base_query: &HybridQuery,
        query_variants: &[String],
        requester_nous_id: &str,
    ) -> crate::error::Result<Vec<HybridResult>> {
        use crate::query_rewrite::rrf_merge;

        if query_variants.is_empty() {
            return self.search_hybrid_scoped(base_query, requester_nous_id);
        }

        let mut results_per_query = Vec::with_capacity(query_variants.len());
        for variant in query_variants {
            let mut q = base_query.clone();
            q.text.clone_from(variant);
            match self.search_hybrid_scoped(&q, requester_nous_id) {
                Ok(results) => results_per_query.push(results),
                Err(e) => {
                    tracing::warn!(variant = %variant, error = %e, "scoped search variant failed, skipping");
                }
            }
        }

        if results_per_query.is_empty() {
            return Err(crate::error::EnhancedSearchSnafu {
                message: format!("{} variants failed", query_variants.len()),
            }
            .build());
        }

        Ok(rrf_merge(&results_per_query, 60.0))
    }
}
