use std::collections::{BTreeMap, HashMap, HashSet};

use snafu::ResultExt;

use super::marshal::{
    build_hybrid_query, build_scoped_hybrid_query, embedding_to_params, extract_int, extract_str,
    rows_to_hybrid_results, rows_to_recall_results, sanitize_fts_query, scoped_visibility_rules,
};
use tracing::instrument;

use super::{HybridQuery, HybridResult, KnowledgeStore, queries};

#[cfg(feature = "mneme-engine")]
fn truncate_recall_results(results: &mut Vec<crate::knowledge::RecallResult>, k: i64) {
    let limit = usize::try_from(k.max(0)).unwrap_or(usize::MAX);
    results.truncate(limit);
}

#[cfg(feature = "mneme-engine")]
fn fact_result_ids(results: &[crate::knowledge::RecallResult]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for result in results.iter().filter(|r| r.source_type == "fact") {
        if seen.insert(result.source_id.clone()) {
            ids.push(result.source_id.clone());
        }
    }
    ids
}

#[cfg(feature = "mneme-engine")]
fn datalog_string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

#[cfg(feature = "mneme-engine")]
fn datalog_string_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| datalog_string_literal(value))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(feature = "mneme-engine")]
struct RecallPolicyFields {
    scope: Option<crate::knowledge::MemoryScope>,
    project_id: Option<eidos::workspace::ProjectId>,
    visibility: crate::knowledge::Visibility,
    sensitivity: Option<crate::knowledge::FactSensitivity>,
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
        let graph_ctx = self.load_graph_context().unwrap_or_default();
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

        let script = format!(
            r"
            {visible}
            ?[id, content, source_type, source_id, dist, scope, project_id, visibility, nous_id, sensitivity] :=
                ~embeddings:semantic_idx {{id: embedding_id, content: _embedding_content, source_type, source_id |
                    query: $query_vec, k: $k, ef: $ef, bind_distance: dist}},
                source_type == 'fact',
                visible_fact[source_id],
                *facts{{id: source_id, content, is_forgotten, superseded_by, scope, project_id, visibility, nous_id, sensitivity}},
                is_forgotten == false,
                is_null(superseded_by),
                id = source_id
            :order dist
            :limit $k
            ",
            visible = scoped_visibility_rules()
        );
        let mut results = rows_to_recall_results(self.run_read(&script, params)?)?;
        // WHY (#5663): load GraphContext once and share across enrichment + cluster expansion.
        let graph_ctx = self.load_graph_context().unwrap_or_default();
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
        let graph_ctx = self.load_graph_context().unwrap_or_default();
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

        let script = format!(
            r"
            {visible}
            bm25[id, score] := ~facts:content_fts{{id | query: $query_text, k: $k, score_kind: 'bm25', bind_score: score}}

            ?[id, content, source_type, source_id, dist, scope, project_id, visibility, nous_id, sensitivity] :=
                bm25[id, bm25_score],
                visible_fact[id],
                *facts{{id, content, is_forgotten, superseded_by, scope, project_id, visibility, nous_id, sensitivity}},
                is_forgotten == false,
                is_null(superseded_by),
                source_type = 'fact',
                source_id = id,
                dist = 1.0 / bm25_score
            :order dist
            :limit $k
            ",
            visible = scoped_visibility_rules()
        );
        let mut results = rows_to_recall_results(self.run_read(&script, params)?)?;
        // WHY (#5663): load GraphContext once and share across enrichment + cluster expansion.
        let graph_ctx = self.load_graph_context().unwrap_or_default();
        self.enrich_recall_results(&mut results, &graph_ctx);
        self.enrich_source_counts(&mut results);
        self.expand_recall_by_cluster_scoped(&mut results, k, requester_nous_id, Some(&graph_ctx))?;
        truncate_recall_results(&mut results, k);
        Ok(results)
    }

    /// Enrich recall results with graph importance scores from cached `graph_scores`.
    ///
    /// For each fact result, looks up associated entities in `fact_entities`, then
    /// takes the maximum `PageRank` among those entities. Non-fact results are left
    /// unchanged (`graph_importance` stays 0.0).
    ///
    /// WHY (#5663): accepts a pre-loaded `GraphContext` so callers can load it once
    /// and share it across `enrich_recall_results` + `expand_recall_by_cluster`,
    /// eliminating redundant full-table scans of `graph_scores` per search call.
    pub(super) fn enrich_recall_results(
        &self,
        results: &mut [crate::knowledge::RecallResult],
        graph_ctx: &crate::graph_intelligence::GraphContext,
    ) {
        let fact_ids = fact_result_ids(results);
        if fact_ids.is_empty() {
            return;
        }

        let pageranks = &graph_ctx.pageranks;
        if pageranks.is_empty() {
            return;
        }

        let id_list = datalog_string_list(&fact_ids);
        let script = format!(
            r"
            ?[fact_id, entity_id] :=
                *fact_entities{{fact_id, entity_id}},
                fact_id in [{id_list}]
            "
        );
        let Ok(entity_rows) = self.run_read(&script, BTreeMap::new()) else {
            return;
        };
        let mut max_by_fact = HashMap::<String, f64>::new();
        for row in &entity_rows.rows {
            let Some(fact_id) = row.first().and_then(|v| v.get_str()) else {
                continue;
            };
            let Some(entity_id) = row.get(1).and_then(|v| v.get_str()) else {
                continue;
            };
            let Some(score) = pageranks.get(entity_id).copied() else {
                continue;
            };
            let current = max_by_fact.entry(fact_id.to_owned()).or_insert(0.0);
            *current = (*current).max(score);
        }

        for result in results.iter_mut().filter(|r| r.source_type == "fact") {
            if let Some(max_pr) = max_by_fact.get(&result.source_id) {
                result.graph_importance = *max_pr;
            }
        }
    }

    /// Populate `source_count` on fact results from the `fact_multiplicity`
    /// side-index so the recall pipeline can score the convergence factor (#4415).
    ///
    /// Non-consolidated / legacy facts have no multiplicity record and keep
    /// `source_count` 0 (convergence scores 0 for them).
    pub(super) fn enrich_source_counts(&self, results: &mut [crate::knowledge::RecallResult]) {
        let fact_ids = fact_result_ids(results);
        if fact_ids.is_empty() {
            return;
        }

        let id_list = datalog_string_list(&fact_ids);
        let script = format!(
            r"
            ?[fact_id, source_count] :=
                *fact_multiplicity{{fact_id, source_count}},
                fact_id in [{id_list}]
            "
        );
        let Ok(rows) = self.run_read(&script, BTreeMap::new()) else {
            return;
        };
        let mut counts_by_fact = HashMap::<String, u32>::new();
        for row in &rows.rows {
            let Some(fact_id) = row.first().and_then(|v| v.get_str()) else {
                continue;
            };
            let Some(source_count) = row
                .get(1)
                .and_then(|value| extract_int(value).ok())
                .and_then(|count| u32::try_from(count).ok())
            else {
                continue;
            };
            counts_by_fact.insert(fact_id.to_owned(), source_count);
        }

        for result in results.iter_mut().filter(|r| r.source_type == "fact") {
            if let Some(source_count) = counts_by_fact.get(&result.source_id) {
                result.source_count = *source_count;
            }
        }
    }

    /// Hydrate recall results with `scope`, `project_id`, `visibility`, and `sensitivity`.
    ///
    /// Semantic search returns from the `embeddings` relation, which does not
    /// carry these fields. This enrichment looks them up from `facts` for
    /// `source_type == "fact"` results so downstream quota and visibility
    /// filters see accurate values.
    pub(super) fn hydrate_recall_scope_visibility(
        &self,
        results: &mut [crate::knowledge::RecallResult],
    ) {
        let fact_ids = fact_result_ids(results);
        if fact_ids.is_empty() {
            return;
        }

        let id_list = datalog_string_list(&fact_ids);
        let script = format!(
            r"
            ?[id, scope, project_id, visibility, sensitivity] :=
                *facts{{id, scope, project_id, visibility, sensitivity}},
                id in [{id_list}]
            "
        );
        let Ok(rows) = self.run_read(&script, BTreeMap::new()) else {
            return;
        };
        let mut fields_by_fact = HashMap::<String, RecallPolicyFields>::new();
        for row in &rows.rows {
            let Some(fact_id) = row.first().and_then(|v| v.get_str()) else {
                continue;
            };
            let mut fields = RecallPolicyFields {
                scope: None,
                project_id: None,
                visibility: crate::knowledge::Visibility::default(),
                sensitivity: None,
            };
            if let Some(scope_str) = row.get(1).and_then(|v| v.get_str())
                && !scope_str.is_empty()
            {
                match scope_str.parse::<crate::knowledge::MemoryScope>() {
                    Ok(scope) => fields.scope = Some(scope),
                    Err(error) => tracing::warn!(
                        %error,
                        fact_id,
                        scope = scope_str,
                        "failed to parse recall result memory scope"
                    ),
                }
            }
            if let Some(project_id) = row
                .get(2)
                .and_then(|v| v.get_str())
                .and_then(|s| eidos::workspace::ProjectId::from_sha256_hex(s).ok())
            {
                fields.project_id = Some(project_id);
            }
            if let Some(vis_str) = row.get(3).and_then(|v| v.get_str())
                && !vis_str.is_empty()
            {
                // kanon:ignore RUST/no-result-unwrap-or-default — Visibility::default() IS the documented
                // fallback for unknown/legacy values from storage; clippy::manual_unwrap_or rejects an
                // explicit Ok/Err match here.
                fields.visibility = vis_str
                    .parse::<crate::knowledge::Visibility>()
                    .unwrap_or_default();
            }
            if let Some(sensitivity_str) = row.get(4).and_then(|v| v.get_str())
                && !sensitivity_str.is_empty()
            {
                if let Ok(s) = sensitivity_str.parse::<crate::knowledge::FactSensitivity>() {
                    fields.sensitivity = Some(s);
                } else {
                    tracing::warn!(
                        sensitivity = sensitivity_str,
                        fact_id,
                        "hydrated fact has undecodable sensitivity; leaving as-is to avoid widening to Public"
                    );
                }
            }
            fields_by_fact.insert(fact_id.to_owned(), fields);
        }

        for result in results.iter_mut().filter(|r| r.source_type == "fact") {
            let Some(fields) = fields_by_fact.get(&result.source_id) else {
                continue;
            };
            result.scope = fields.scope;
            result.project_id = fields.project_id.clone();
            result.visibility = fields.visibility;
            if let Some(sensitivity) = fields.sensitivity {
                result.sensitivity = sensitivity;
            }
        }
    }

    /// Expand recall results with cluster-mate facts.
    ///
    /// Takes the top results, finds their Louvain clusters, and queries for
    /// additional active facts linked to entities in those clusters. Adds
    /// new results with a neutral distance of 1.0, deduplicating by `source_id`.
    /// Limits expansion to at most `k` additional results.
    #[expect(
        clippy::too_many_lines,
        reason = "cluster expansion keeps query, hydration, and append logic together"
    )]
    fn expand_recall_by_cluster(
        &self,
        results: &mut Vec<crate::knowledge::RecallResult>,
        k: i64,
        requester_nous_id: Option<&str>,
        preloaded_ctx: Option<&crate::graph_intelligence::GraphContext>,
    ) -> crate::error::Result<()> {
        if results.is_empty() {
            return Ok(());
        }

        let owned_ctx: crate::graph_intelligence::GraphContext;
        let ctx = if let Some(c) = preloaded_ctx {
            c
        } else {
            owned_ctx = self.load_graph_context()?;
            &owned_ctx
        };
        if ctx.clusters.is_empty() {
            return Ok(());
        }

        // Collect clusters from top results.
        let top_n = results.len().min(5);
        let mut context_clusters = std::collections::HashSet::new();
        for result in results.iter().take(top_n) {
            if result.source_type != "fact" {
                continue;
            }
            let script = "?[entity_id] := *fact_entities{fact_id: $fid, entity_id}";
            let mut params = std::collections::BTreeMap::new();
            params.insert(
                "fid".to_owned(),
                crate::engine::DataValue::Str(result.source_id.as_str().into()),
            );
            let Ok(entity_rows) = self.run_read(script, params) else {
                continue;
            };
            for row in &entity_rows.rows {
                if let Some(cid) = row
                    .first()
                    .and_then(|v| v.get_str())
                    .and_then(|entity_id| ctx.clusters.get(entity_id))
                {
                    context_clusters.insert(*cid);
                }
            }
        }

        if context_clusters.is_empty() {
            return Ok(());
        }

        let existing_ids: std::collections::HashSet<String> =
            results.iter().map(|r| r.source_id.clone()).collect();
        let mut added = 0;
        let limit = usize::try_from(k.max(1)).unwrap_or(1);

        for cluster_id in context_clusters {
            // WHY (#5559): filter by nous_id so cross-nous facts cannot be injected
            // via cluster expansion in a shared cohort store.
            let nous_filter = if requester_nous_id.is_some() {
                ", nous_id == $nous_id"
            } else {
                ""
            };
            let script = format!(
                r"
                ?[fact_id, content, nous_id, sensitivity, scope, project_id, visibility] :=
                    *graph_scores{{entity_id, score_type: 'cluster', cluster_id: $cid}},
                    *fact_entities{{fact_id: fid, entity_id}},
                    *facts{{id: fid, content, nous_id, is_forgotten, superseded_by,
                           sensitivity, scope, project_id, visibility}},
                    is_forgotten == false,
                    is_null(superseded_by){nous_filter},
                    fact_id = fid
                :limit $limit
            "
            );
            let mut params = std::collections::BTreeMap::new();
            params.insert("cid".to_owned(), crate::engine::DataValue::from(cluster_id));
            params.insert(
                "limit".to_owned(),
                crate::engine::DataValue::from(i64::try_from(limit).unwrap_or(i64::MAX)),
            );
            if let Some(nid) = requester_nous_id {
                params.insert(
                    "nous_id".to_owned(),
                    crate::engine::DataValue::Str(nid.into()),
                );
            }
            let Ok(rows) = self.run_read(&script, params) else {
                continue;
            };
            for row in &rows.rows {
                let Some(fact_id) = row.first().and_then(|v| v.get_str()) else {
                    continue;
                };
                if existing_ids.contains(fact_id) {
                    continue;
                }
                let content = row
                    .get(1)
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_owned();
                let nous_id = row
                    .get(2)
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_owned();
                let sensitivity_str = row
                    .get(3)
                    .and_then(|v| v.get_str())
                    .filter(|s| !s.is_empty());
                let sensitivity = if let Some(s) = sensitivity_str {
                    if let Ok(v) = s.parse::<crate::knowledge::FactSensitivity>() {
                        v
                    } else {
                        tracing::warn!(
                            sensitivity = s,
                            fact_id = ?row.first().and_then(|v| v.get_str()),
                            "cluster-expanded fact has undecodable sensitivity; skipping to avoid widening to Public"
                        );
                        continue;
                    }
                } else {
                    crate::knowledge::FactSensitivity::default()
                };
                let scope = row
                    .get(4)
                    .and_then(|v| v.get_str())
                    .filter(|s| !s.is_empty())
                    .and_then(|s| s.parse::<crate::knowledge::MemoryScope>().ok());
                let project_id = row
                    .get(5)
                    .and_then(|v| v.get_str())
                    .and_then(|s| eidos::workspace::ProjectId::from_sha256_hex(s).ok());
                let visibility = row
                    .get(6)
                    .and_then(|v| v.get_str())
                    .and_then(|s| s.parse::<crate::knowledge::Visibility>().ok())
                    .unwrap_or_default();
                results.push(crate::knowledge::RecallResult {
                    content,
                    distance: 1.0,
                    source_type: "fact".to_owned(),
                    source_id: fact_id.to_owned(),
                    nous_id,
                    sensitivity,
                    graph_importance: 0.0,
                    scope,
                    project_id,
                    visibility,
                    source_count: 0,
                });
                added += 1;
                if added >= limit {
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "scoped cluster expansion mirrors the unscoped query, hydration, and append flow"
    )]
    fn expand_recall_by_cluster_scoped(
        &self,
        results: &mut Vec<crate::knowledge::RecallResult>,
        k: i64,
        requester_nous_id: &str,
        preloaded_ctx: Option<&crate::graph_intelligence::GraphContext>,
    ) -> crate::error::Result<()> {
        if results.is_empty() {
            return Ok(());
        }

        let owned_ctx: crate::graph_intelligence::GraphContext;
        let ctx = if let Some(c) = preloaded_ctx {
            c
        } else {
            owned_ctx = self.load_graph_context()?;
            &owned_ctx
        };
        if ctx.clusters.is_empty() {
            return Ok(());
        }

        let top_n = results.len().min(5);
        let mut context_clusters = std::collections::HashSet::new();
        for result in results.iter().take(top_n) {
            if result.source_type != "fact" {
                continue;
            }
            let script = "?[entity_id] := *fact_entities{fact_id: $fid, entity_id}";
            let mut params = std::collections::BTreeMap::new();
            params.insert(
                "fid".to_owned(),
                crate::engine::DataValue::Str(result.source_id.as_str().into()),
            );
            let Ok(entity_rows) = self.run_read(script, params) else {
                continue;
            };
            for row in &entity_rows.rows {
                if let Some(cid) = row
                    .first()
                    .and_then(|v| v.get_str())
                    .and_then(|entity_id| ctx.clusters.get(entity_id))
                {
                    context_clusters.insert(*cid);
                }
            }
        }

        if context_clusters.is_empty() {
            return Ok(());
        }

        let existing_ids: std::collections::HashSet<String> =
            results.iter().map(|r| r.source_id.clone()).collect();
        let mut added = 0;
        let limit = usize::try_from(k.max(1)).unwrap_or(1);

        for cluster_id in context_clusters {
            let script = format!(
                r"
                {visible}
                ?[fact_id, content, nous_id, sensitivity, scope, project_id, visibility] :=
                    *graph_scores{{entity_id, score_type: 'cluster', cluster_id: $cid}},
                    *fact_entities{{fact_id: fid, entity_id}},
                    visible_fact[fid],
                    *facts{{id: fid, content, nous_id, is_forgotten, superseded_by,
                           sensitivity, scope, project_id, visibility}},
                    is_forgotten == false,
                    is_null(superseded_by),
                    fact_id = fid
                :limit $limit
                ",
                visible = scoped_visibility_rules()
            );
            let mut params = std::collections::BTreeMap::new();
            params.insert("cid".to_owned(), crate::engine::DataValue::from(cluster_id));
            params.insert(
                "limit".to_owned(),
                crate::engine::DataValue::from(i64::try_from(limit).unwrap_or(i64::MAX)),
            );
            params.insert(
                "requester_nous_id".to_owned(),
                crate::engine::DataValue::Str(requester_nous_id.into()),
            );
            let Ok(rows) = self.run_read(&script, params) else {
                continue;
            };
            for row in &rows.rows {
                let Some(fact_id) = row.first().and_then(|v| v.get_str()) else {
                    continue;
                };
                if existing_ids.contains(fact_id) {
                    continue;
                }
                let content = row
                    .get(1)
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_owned();
                let nous_id = row
                    .get(2)
                    .and_then(|v| v.get_str())
                    .unwrap_or("")
                    .to_owned();
                let sensitivity_str = row
                    .get(3)
                    .and_then(|v| v.get_str())
                    .filter(|s| !s.is_empty());
                let sensitivity = if let Some(s) = sensitivity_str {
                    if let Ok(v) = s.parse::<crate::knowledge::FactSensitivity>() {
                        v
                    } else {
                        tracing::warn!(
                            sensitivity = s,
                            fact_id = ?row.first().and_then(|v| v.get_str()),
                            "cluster-expanded fact has undecodable sensitivity; skipping to avoid widening to Public"
                        );
                        continue;
                    }
                } else {
                    crate::knowledge::FactSensitivity::default()
                };
                let scope = row
                    .get(4)
                    .and_then(|v| v.get_str())
                    .filter(|s| !s.is_empty())
                    .and_then(|s| s.parse::<crate::knowledge::MemoryScope>().ok());
                let project_id = row
                    .get(5)
                    .and_then(|v| v.get_str())
                    .and_then(|s| eidos::workspace::ProjectId::from_sha256_hex(s).ok());
                let visibility = row
                    .get(6)
                    .and_then(|v| v.get_str())
                    .and_then(|s| s.parse::<crate::knowledge::Visibility>().ok())
                    .unwrap_or_default();
                results.push(crate::knowledge::RecallResult {
                    content,
                    distance: 1.0,
                    source_type: "fact".to_owned(),
                    source_id: fact_id.to_owned(),
                    nous_id,
                    sensitivity,
                    graph_importance: 0.0,
                    scope,
                    project_id,
                    visibility,
                    source_count: 0,
                });
                added += 1;
                if added >= limit {
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    fn increment_recall_access(&self, results: &[crate::knowledge::RecallResult]) {
        let source_ids: Vec<crate::id::FactId> = results
            .iter()
            .filter(|r| r.source_type == "fact")
            .filter_map(|r| crate::id::FactId::new(&r.source_id).ok())
            .collect();
        if let Err(e) = self.increment_access(&source_ids) {
            tracing::warn!(error = %e, "failed to increment access counts");
        }
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
    pub fn search_hybrid_scoped(
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
            return Err(crate::error::EnhancedSearchSnafu {
                message: format!("{} variants failed", query_variants.len()),
            }
            .build());
        }

        Ok(rrf_merge(&results_per_query, 60.0))
    }

    /// Multi-query hybrid search scoped to facts visible to `requester_nous_id`.
    pub fn search_enhanced_scoped(
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

    /// Tiered search: fast path -> enhanced -> graph-enhanced.
    ///
    /// Escalates through tiers until sufficient results are found.
    /// Requires a `QueryRewriter` and `RewriteProvider` for tier 2+.
    ///
    /// # Complexity
    ///
    /// Best case O(log n * ef) for fast path. Worst case adds query rewriting
    /// O(RW) plus enhanced search O(V * `search_hybrid`) plus graph expansion O(E).
    pub fn search_tiered(
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

        let rewrite_result = rewriter
            .rewrite(&base_query.text, context, provider)
            .map_err(|e| {
                crate::error::QueryRewriteSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
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

    /// Tiered search scoped to facts visible to `requester_nous_id`.
    pub fn search_tiered_scoped(
        &self,
        base_query: &HybridQuery,
        rewriter: &crate::query_rewrite::QueryRewriter,
        provider: &dyn crate::query_rewrite::RewriteProvider,
        context: Option<&str>,
        config: &crate::query_rewrite::TieredSearchConfig,
        requester_nous_id: &str,
    ) -> crate::error::Result<crate::query_rewrite::TieredSearchResult<HybridResult>> {
        let start = std::time::Instant::now();

        let fast_results = self.search_hybrid_scoped(base_query, requester_nous_id)?;
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

        let rewrite_result = rewriter
            .rewrite(&base_query.text, context, provider)
            .map_err(|e| {
                crate::error::QueryRewriteSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
        let enhanced_results =
            self.search_enhanced_scoped(base_query, &rewrite_result.variants, requester_nous_id)?;
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

        let graph_results =
            self.expand_via_graph_scoped(&enhanced_results, config, requester_nous_id);
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

    /// Tiered search with hydrated recall rows for the production recall pipeline.
    ///
    /// This is the bridge from the low-level tiered retrieval orchestration
    /// (which operates on IDs and RRF scores) to `RecallResult` records that
    /// nous can score, filter, and inject.
    pub fn search_tiered_for_recall(
        &self,
        base_query: &HybridQuery,
        rewriter: &crate::query_rewrite::QueryRewriter,
        provider: &dyn crate::query_rewrite::RewriteProvider,
        context: Option<&str>,
        config: &crate::query_rewrite::TieredSearchConfig,
    ) -> crate::error::Result<
        crate::query_rewrite::TieredSearchResult<crate::knowledge::RecallResult>,
    > {
        let tiered = self.search_tiered(base_query, rewriter, provider, context, config)?;
        let mut recalled = Vec::with_capacity(tiered.results.len());
        for result in &tiered.results {
            for fact in self.read_facts_by_id(result.id.as_str())? {
                if fact.lifecycle.is_forgotten || fact.lifecycle.superseded_by.is_some() {
                    continue;
                }
                recalled.push(crate::knowledge::RecallResult {
                    content: fact.content,
                    distance: (1.0 - result.rrf_score).max(0.0),
                    source_type: "fact".to_owned(),
                    source_id: fact.id.as_str().to_owned(),
                    nous_id: fact.nous_id,
                    sensitivity: fact.sensitivity,
                    graph_importance: 0.0,
                    scope: fact.scope,
                    project_id: fact.project_id,
                    visibility: fact.visibility,
                    source_count: self
                        .get_fact_multiplicity(&fact.id)
                        .ok()
                        .flatten()
                        .map_or(0, |m| m.source_count),
                });
                break;
            }
        }

        Ok(crate::query_rewrite::TieredSearchResult {
            tier: tiered.tier,
            results: recalled,
            query_variants: tiered.query_variants,
            total_latency_ms: tiered.total_latency_ms,
        })
    }

    /// Tiered search with hydrated recall rows, scoped before ranking and graph expansion.
    pub fn search_tiered_for_recall_scoped(
        &self,
        base_query: &HybridQuery,
        rewriter: &crate::query_rewrite::QueryRewriter,
        provider: &dyn crate::query_rewrite::RewriteProvider,
        context: Option<&str>,
        config: &crate::query_rewrite::TieredSearchConfig,
        requester_nous_id: &str,
    ) -> crate::error::Result<
        crate::query_rewrite::TieredSearchResult<crate::knowledge::RecallResult>,
    > {
        let tiered = self.search_tiered_scoped(
            base_query,
            rewriter,
            provider,
            context,
            config,
            requester_nous_id,
        )?;
        let mut recalled = Vec::with_capacity(tiered.results.len());
        for result in &tiered.results {
            for fact in self.read_visible_facts_by_id(result.id.as_str(), requester_nous_id)? {
                if fact.lifecycle.is_forgotten || fact.lifecycle.superseded_by.is_some() {
                    continue;
                }
                recalled.push(crate::knowledge::RecallResult {
                    content: fact.content,
                    distance: (1.0 - result.rrf_score).max(0.0),
                    source_type: "fact".to_owned(),
                    source_id: fact.id.as_str().to_owned(),
                    nous_id: fact.nous_id,
                    sensitivity: fact.sensitivity,
                    graph_importance: 0.0,
                    scope: fact.scope,
                    project_id: fact.project_id,
                    visibility: fact.visibility,
                    source_count: self
                        .get_fact_multiplicity(&fact.id)
                        .ok()
                        .flatten()
                        .map_or(0, |m| m.source_count),
                });
                break;
            }
        }

        Ok(crate::query_rewrite::TieredSearchResult {
            tier: tiered.tier,
            results: recalled,
            query_variants: tiered.query_variants,
            total_latency_ms: tiered.total_latency_ms,
        })
    }

    /// Expand search results via entity graph neighborhood.
    ///
    /// Takes the top entity IDs from existing results, queries their neighborhoods,
    /// and returns related facts as additional results.
    ///
    /// # Complexity
    ///
    /// O(K * N) where K is top results expanded, N is average neighborhood size.
    /// Each entity neighborhood query is O(log E) where E is entity count.
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

    #[expect(
        clippy::as_conversions,
        clippy::cast_precision_loss,
        reason = "rank indices fit in f64 mantissa"
    )]
    fn expand_via_graph_scoped(
        &self,
        existing: &[HybridResult],
        config: &crate::query_rewrite::TieredSearchConfig,
        requester_nous_id: &str,
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
                let Ok(neighborhood) = self.entity_neighborhood(&entity_id) else {
                    continue;
                };
                for row in &neighborhood.rows {
                    let Some(neighbor_entity_id) = row.first().and_then(|v| v.get_str()) else {
                        continue;
                    };
                    for id in self
                        .visible_fact_ids_for_entity(neighbor_entity_id, requester_nous_id)
                        .unwrap_or_default()
                    {
                        if !existing_ids.contains(id.as_str()) {
                            expanded_ids.insert(id);
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

    /// Search for facts relevant to a query, as they existed at a specific time.
    /// Filters hybrid search results through the temporal lens.
    ///
    /// # Complexity
    ///
    /// O(`search_hybrid` + C) where C is candidate count for temporal validation.
    /// Temporal check is O(C) using in-clause filtering.
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
    ///
    /// # Complexity
    ///
    /// O(C) where C is the number of candidate IDs. Uses a single query with
    /// an IN clause for batch validation.
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
    ///
    /// # Complexity
    ///
    /// Same as `search_temporal`: O(`search_hybrid` + C).
    #[instrument(skip(self, q, at_time))]
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
