use tracing::instrument;

use super::marshal::extract_str;
use super::{HybridQuery, HybridResult, KnowledgeStore};

#[cfg(feature = "mneme-engine")]
impl KnowledgeStore {
    /// Tiered search: fast path -> enhanced -> graph-enhanced.
    ///
    /// Escalates through tiers until sufficient results are found.
    /// Requires a `QueryRewriter` and `RewriteProvider` for tier 2+.
    ///
    /// # Complexity
    ///
    /// Best case O(log n * ef) for fast path. Worst case adds query rewriting
    /// O(RW) plus enhanced search O(V * `search_hybrid`) plus graph expansion O(E).
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
    pub(crate) fn search_tiered_scoped(
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
    #[expect(
        dead_code,
        reason = "unscoped recall bridge retained for crate-local callers"
    )]
    pub(crate) fn search_tiered_for_recall(
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
    // kanon:ignore RUST/pub-visibility — consumed by nous recall search integration
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
                    let visible_fact_ids = match self
                        .visible_fact_ids_for_entity(neighbor_entity_id, requester_nous_id)
                    {
                        Ok(ids) => ids,
                        Err(error) => {
                            tracing::warn!(
                                %error,
                                neighbor_entity_id,
                                requester_nous_id,
                                "failed to load visible facts for graph-expanded entity"
                            );
                            continue;
                        }
                    };
                    for id in visible_fact_ids {
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
