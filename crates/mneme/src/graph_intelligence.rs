//! Graph-enhanced recall scoring: wires `PageRank`, `Louvain` community detection,
//! and bounded BFS proximity into the 6-factor recall pipeline.
//!
//! This module provides:
//! - [`GraphContext`](crate::graph_intelligence::GraphContext): per-query snapshot of graph scores loaded from `graph_scores` relation
//! - Enhanced scoring functions that augment epistemic tier, relationship proximity, and access frequency
//! - Background recomputation of `PageRank` + `Louvain` stored in `graph_scores`
//! - Cache invalidation via [`GraphDirtyFlag`](crate::graph_intelligence::GraphDirtyFlag)
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "module internals; only exercised by crate-level tests"
    )
)]

use std::collections::{HashMap, HashSet};

/// Datalog DDL for the `graph_scores` relation.
///
/// Caches `PageRank` scores, community (cluster) assignments, and the `PageRank` max
/// meta-entry. Updated by background recomputation.
pub const GRAPH_SCORES_DDL: &str = r":create graph_scores {
    entity_id: String, score_type: String =>
    score: Float default 0.0, cluster_id: Int default -1, updated_at: String
}";

/// Per-query snapshot of graph intelligence data.
///
/// Loaded once from the `graph_scores` relation at query entry. All fields
/// default to empty when no graph data is available, producing identical
/// scores to the base 6-factor formula (backward compatible).
#[derive(Debug, Clone, Default)]
pub struct GraphContext {
    /// `entity_id` → normalized `PageRank` score [0.0, 1.0]
    pub pageranks: HashMap<String, f64>,
    /// `entity_id` → `Louvain` `cluster_id`
    pub clusters: HashMap<String, i64>,
    /// Clusters that contain the query's context entities
    pub context_clusters: HashSet<i64>,
    /// `fact_id` → minimum hops from query context entities (via bounded BFS)
    pub proximity: HashMap<String, Option<u32>>,
    /// `fact_id` → length of supersession chain (number of predecessors)
    pub chain_lengths: HashMap<String, u32>,
}

impl GraphContext {
    /// Returns `true` when no graph data has been loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pageranks.is_empty() && self.clusters.is_empty()
    }

    /// Look up the normalized `PageRank` importance for an entity.
    /// Returns 0.0 if the entity has no `PageRank` score.
    #[must_use]
    pub fn importance(&self, entity_id: &str) -> f64 {
        self.pageranks.get(entity_id).copied().unwrap_or(0.0)
    }

    /// Check whether a given entity is in the same cluster as any query context entity.
    #[must_use]
    pub fn same_cluster(&self, entity_id: &str) -> bool {
        self.clusters
            .get(entity_id)
            .is_some_and(|cid| self.context_clusters.contains(cid))
    }

    /// Get the supersession chain length for a fact.
    #[must_use]
    pub fn chain_length(&self, fact_id: &str) -> u32 {
        self.chain_lengths.get(fact_id).copied().unwrap_or(0)
    }

    /// Get the BFS hop count for a fact.
    #[must_use]
    pub fn hops(&self, fact_id: &str) -> Option<u32> {
        self.proximity.get(fact_id).copied().flatten()
    }
}

/// `PageRank`-boosted epistemic tier scoring.
///
/// `PageRank` acts as a multiplier on the base epistemic tier score (weight 0.15).
/// A verified fact about a hub entity is worth more than one about a peripheral entity.
///
/// `importance` is the normalized `PageRank` in [0.0, 1.0].
/// Boost range: [1.0, 1.5]: at importance=0.0 the score is unchanged.
#[must_use]
pub fn score_epistemic_tier_with_importance(base_tier_score: f64, importance: f64) -> f64 {
    let boost = 1.0 + (importance.clamp(0.0, 1.0) * 0.5); // [1.0, 1.5]
    (base_tier_score * boost).min(1.0)
}

/// Community-aware relationship proximity scoring.
///
/// Same-cluster facts get a proximity floor of 0.3 even if no direct path exists.
/// This reflects that entities in the same community are semantically related.
#[must_use]
pub fn score_relationship_proximity_with_cluster(base_hop_score: f64, same_cluster: bool) -> f64 {
    if same_cluster {
        base_hop_score.max(0.3)
    } else {
        base_hop_score
    }
}

/// Supersession chain bonus on access frequency scoring.
///
/// Facts at the end of long supersession chains are in actively-maintained domains.
/// Each predecessor adds 0.05 to the access score, capped at +0.2 (`chain_length=4`).
#[must_use]
pub fn score_access_with_evolution(base_access_score: f64, chain_length: u32) -> f64 {
    let evolution_bonus = (f64::from(chain_length) * 0.05).min(0.2);
    (base_access_score + evolution_bonus).min(1.0)
}

/// Combined Datalog script for `PageRank` + `Louvain` community detection.
///
/// Reads the `relationships` relation, computes `PageRank` and `Louvain` communities,
/// and stores results into `graph_scores`.
///
/// Parameters: `$now` (ISO 8601 timestamp string).
pub const RECOMPUTE_GRAPH_SCORES: &str = r"
edges[src, dst] := *relationships{src, dst}
edges_w[src, dst, weight] := *relationships{src, dst, weight}

pr[entity_id, score] <~ PageRank(edges[])

pr_max[max(score)] := pr[_, score]

comm[labels, entity_id] <~ CommunityDetectionLouvain(edges_w[])

?[entity_id, score_type, score, cluster_id, updated_at] :=
    pr[entity_id, raw_score], pr_max[m], m > 0,
    score = raw_score / m, score_type = 'pagerank', cluster_id = -1, updated_at = $now

?[entity_id, score_type, score, cluster_id, updated_at] :=
    comm[labels, entity_id], length(labels) > 0, cid = first(labels),
    score_type = 'cluster', score = 0.0, cluster_id = cid, updated_at = $now

?[entity_id, score_type, score, cluster_id, updated_at] :=
    pr_max[m], m > 0, entity_id = '__meta__', score_type = 'pagerank_max',
    score = m, cluster_id = -1, updated_at = $now

:put graph_scores { entity_id, score_type => score, cluster_id, updated_at }
";

/// Datalog script to load all graph scores into memory.
pub const LOAD_GRAPH_SCORES: &str = r"
?[entity_id, score_type, score, cluster_id] :=
    *graph_scores{entity_id, score_type, score, cluster_id}
";

/// Bounded BFS Datalog script for proximity computation.
///
/// Computes hop distances from a set of seed entity IDs up to 4 hops.
/// Parameters: `$seeds` (list of entity IDs).
///
/// Returns rows of `[entity_id, hops]`.
pub const BFS_PROXIMITY_4HOP: &str = r"
seed[id] := id in $seeds

hop0[id, h] := seed[id], h = 0
hop1[dst, h] := hop0[src, _], *relationships{src, dst}, not hop0[dst, _], h = 1
hop2[dst, h] := hop1[src, _], *relationships{src, dst}, not hop0[dst, _], not hop1[dst, _], h = 2
hop3[dst, h] := hop2[src, _], *relationships{src, dst}, not hop0[dst, _], not hop1[dst, _], not hop2[dst, _], h = 3
hop4[dst, h] := hop3[src, _], *relationships{src, dst}, not hop0[dst, _], not hop1[dst, _], not hop2[dst, _], not hop3[dst, _], h = 4

?[entity_id, hops] := hop0[entity_id, hops]
?[entity_id, hops] := hop1[entity_id, hops]
?[entity_id, hops] := hop2[entity_id, hops]
?[entity_id, hops] := hop3[entity_id, hops]
?[entity_id, hops] := hop4[entity_id, hops]
";

/// Datalog script for computing supersession chain lengths.
///
/// Counts how many predecessors each fact has in its supersession chain.
pub const SUPERSESSION_CHAIN_LENGTHS: &str = r"
chain[id, d] := *facts{id, superseded_by}, is_null(superseded_by), d = 0
chain[id, n] := *facts{id, superseded_by}, superseded_by = next_id, not is_null(next_id),
    chain[next_id, prev_n], n = prev_n + 1

?[id, max(depth)] := chain[id, depth]
";

/// Atomic flag indicating the knowledge graph has been mutated since the
/// last `graph_scores` recomputation.
///
/// Set to `true` by `insert_entity` / `insert_relationship`. Cleared by the
/// background recomputation task.
pub struct GraphDirtyFlag {
    inner: std::sync::atomic::AtomicBool,
}

impl GraphDirtyFlag {
    /// Create a new flag, initially clean.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Mark the graph as dirty (needs recomputation).
    pub fn mark_dirty(&self) {
        self.inner.store(true, std::sync::atomic::Ordering::Release);
    }

    /// Check if the graph is dirty and clear the flag atomically.
    /// Returns `true` if it was dirty.
    pub fn take_dirty(&self) -> bool {
        self.inner.swap(false, std::sync::atomic::Ordering::AcqRel)
    }

    /// Check if the graph is dirty without clearing.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.inner.load(std::sync::atomic::Ordering::Acquire)
    }
}

impl Default for GraphDirtyFlag {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert an `i64` hop/depth value to `u32`, clamping negatives to 0.
#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "hop counts are small non-negative values"
)]
fn i64_to_u32(v: i64) -> u32 {
    v.clamp(0, i64::from(u32::MAX)) as u32
}

/// Parse rows of `[entity_id: String, value: Int]` into a `HashMap<String, u32>`.
///
/// The `value_col` parameter specifies which column index holds the integer value.
/// For multiple rows with the same `entity_id`, keeps the minimum value.
#[cfg(feature = "mneme-engine")]
fn parse_hop_rows(
    rows: &[Vec<crate::engine::DataValue>],
    value_col: usize,
) -> HashMap<String, u32> {
    let mut map = HashMap::new();
    for row in rows {
        if let (Some(id), Some(val)) = (
            row.first().and_then(|v| v.get_str()),
            row.get(value_col)
                .and_then(crate::engine::DataValue::get_int),
        ) {
            let val_u32 = i64_to_u32(val);
            map.entry(id.to_owned())
                .and_modify(|existing: &mut u32| *existing = (*existing).min(val_u32))
                .or_insert(val_u32);
        }
    }
    map
}

#[cfg(feature = "mneme-engine")]
impl crate::knowledge_store::KnowledgeStore {
    /// Initialize the `graph_scores` relation. Called during schema setup.
    pub fn init_graph_scores(&self) -> crate::error::Result<()> {
        self.run_mut_query(GRAPH_SCORES_DDL, std::collections::BTreeMap::new())?;
        Ok(())
    }

    /// Load a `GraphContext` from the `graph_scores` relation.
    ///
    /// Populates pageranks and cluster assignments. Caller should then fill
    /// `context_clusters`, `proximity`, and `chain_lengths` based on query context.
    pub fn load_graph_context(&self) -> crate::error::Result<GraphContext> {
        let result = self.run_query(LOAD_GRAPH_SCORES, std::collections::BTreeMap::new())?;

        let mut ctx = GraphContext::default();
        for row in &result.rows {
            let Some(entity_id) = row.first().and_then(|v| v.get_str()) else {
                continue;
            };
            let Some(score_type) = row.get(1).and_then(|v| v.get_str()) else {
                continue;
            };
            let score = row
                .get(2)
                .and_then(crate::engine::DataValue::get_float)
                .unwrap_or(0.0);
            let cluster_id = row
                .get(3)
                .and_then(crate::engine::DataValue::get_int)
                .unwrap_or(-1);

            match score_type {
                "pagerank" => {
                    ctx.pageranks.insert(entity_id.to_owned(), score);
                }
                "cluster" => {
                    ctx.clusters.insert(entity_id.to_owned(), cluster_id);
                }
                // pagerank_max meta entry: normalization already done in Datalog
                _ => {}
            }
        }

        Ok(ctx)
    }

    /// Compute BFS proximity from seed entities up to 4 hops.
    ///
    /// Uses a 5ms timeout budget. Falls back to the existing 2-hop neighborhood
    /// query if the budget is exceeded.
    pub fn compute_bfs_proximity(
        &self,
        seed_entity_ids: &[String],
    ) -> crate::error::Result<HashMap<String, u32>> {
        use crate::engine::DataValue;

        if seed_entity_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let seeds_list: Vec<DataValue> = seed_entity_ids
            .iter()
            .map(|s| DataValue::Str(s.as_str().into()))
            .collect();

        let mut params = std::collections::BTreeMap::new();
        params.insert("seeds".to_owned(), DataValue::List(seeds_list));

        let timeout = std::time::Duration::from_millis(5);
        match self.run_query_with_timeout(BFS_PROXIMITY_4HOP, params, Some(timeout)) {
            Ok(result) => Ok(parse_hop_rows(&result.rows, 1)),
            Err(crate::error::Error::QueryTimeout { .. }) => {
                tracing::debug!("4-hop BFS exceeded 5ms budget, falling back to 2-hop");
                Ok(self.bfs_fallback_2hop(seed_entity_ids))
            }
            Err(e) => Err(e),
        }
    }

    /// 2-hop fallback when 4-hop BFS exceeds the time budget.
    fn bfs_fallback_2hop(&self, seed_entity_ids: &[String]) -> HashMap<String, u32> {
        let mut proximity = HashMap::new();
        for seed_id in seed_entity_ids {
            let entity_id = crate::id::EntityId::new_unchecked(seed_id);
            if let Ok(neighborhood) = self.entity_neighborhood(&entity_id) {
                for row in &neighborhood.rows {
                    if let Some(neighbor_id) = row.first().and_then(|v| v.get_str()) {
                        let hops = row
                            .get(2)
                            .and_then(crate::engine::DataValue::get_int)
                            .unwrap_or(2);
                        let hops_u32 = i64_to_u32(hops);
                        proximity
                            .entry(neighbor_id.to_owned())
                            .and_modify(|existing: &mut u32| {
                                *existing = (*existing).min(hops_u32);
                            })
                            .or_insert(hops_u32);
                    }
                }
            }
        }
        proximity
    }

    /// Compute supersession chain lengths for all facts.
    pub fn compute_chain_lengths(&self) -> crate::error::Result<HashMap<String, u32>> {
        let result = self.run_query(
            SUPERSESSION_CHAIN_LENGTHS,
            std::collections::BTreeMap::new(),
        )?;
        Ok(parse_hop_rows(&result.rows, 1))
    }

    /// Run the combined `PageRank` + `Louvain` recomputation and store to `graph_scores`.
    pub fn recompute_graph_scores(&self) -> crate::error::Result<()> {
        let now = crate::knowledge::format_timestamp(&jiff::Timestamp::now());
        let mut params = std::collections::BTreeMap::new();
        params.insert("now".to_owned(), crate::engine::DataValue::Str(now.into()));
        self.run_mut_query(RECOMPUTE_GRAPH_SCORES, params)?;
        Ok(())
    }

    /// Compute per-entity domain volatility from supersession patterns.
    ///
    /// Joins `facts`, `fact_entities`, and supersession chain data to produce
    /// `DomainVolatility` scores for all entities with linked facts.
    pub fn compute_domain_volatility(
        &self,
    ) -> crate::error::Result<Vec<crate::succession::DomainVolatility>> {
        let result = self.run_query(
            crate::succession::ENTITY_VOLATILITY_METRICS,
            std::collections::BTreeMap::new(),
        )?;

        let now = jiff::Timestamp::now();
        let mut volatilities = Vec::new();

        for row in &result.rows {
            let Some(entity_id) = row.first().and_then(|v| v.get_str()) else {
                continue;
            };
            let total_facts = row
                .get(1)
                .and_then(crate::engine::DataValue::get_int)
                .unwrap_or(0);
            let superseded_facts = row
                .get(2)
                .and_then(crate::engine::DataValue::get_int)
                .unwrap_or(0);
            let avg_chain_length = row
                .get(3)
                .and_then(crate::engine::DataValue::get_float)
                .unwrap_or(0.0);

            let total = i64_to_u32(total_facts);
            let superseded = i64_to_u32(superseded_facts);
            let volatility_score =
                crate::succession::compute_volatility(total, superseded, avg_chain_length);

            volatilities.push(crate::succession::DomainVolatility {
                entity_id: crate::id::EntityId::new_unchecked(entity_id),
                total_facts: total,
                superseded_facts: superseded,
                avg_chain_length,
                volatility_score,
                computed_at: now,
            });
        }

        Ok(volatilities)
    }

    /// Compute domain volatility and store scores in `graph_scores`.
    ///
    /// Intended for background scheduling: runs the volatility Datalog,
    /// computes scores, and upserts them into `graph_scores` with
    /// `score_type = "volatility"`.
    pub fn compute_and_store_volatility(&self) -> crate::error::Result<()> {
        let volatilities = self.compute_domain_volatility()?;
        let now = crate::knowledge::format_timestamp(&jiff::Timestamp::now());

        for vol in &volatilities {
            let mut params = std::collections::BTreeMap::new();
            params.insert(
                "entity_id".to_owned(),
                crate::engine::DataValue::Str(vol.entity_id.as_str().into()),
            );
            params.insert(
                "volatility".to_owned(),
                crate::engine::DataValue::from(vol.volatility_score),
            );
            params.insert(
                "now".to_owned(),
                crate::engine::DataValue::Str(now.clone().into()),
            );
            self.run_mut_query(crate::succession::STORE_VOLATILITY_SCORE, params)?;
        }

        Ok(())
    }

    /// Load volatility scores from `graph_scores`.
    ///
    /// Returns a map of `entity_id → volatility_score` for all entities
    /// that have stored volatility data.
    pub fn load_volatility_scores(&self) -> crate::error::Result<HashMap<String, f64>> {
        let result = self.run_query(
            r"?[entity_id, score] := *graph_scores{entity_id, score_type, score}, score_type = 'volatility'",
            std::collections::BTreeMap::new(),
        )?;

        let mut scores = HashMap::new();
        for row in &result.rows {
            if let (Some(eid), Some(score)) = (
                row.first().and_then(|v| v.get_str()),
                row.get(1).and_then(crate::engine::DataValue::get_float),
            ) {
                scores.insert(eid.to_owned(), score);
            }
        }

        Ok(scores)
    }

    /// Get the knowledge profile for a specific nous.
    ///
    /// Returns the top entities by fact count, average stability, and
    /// volatility scores. Useful for understanding what each nous "knows about."
    pub fn nous_knowledge_profile(
        &self,
        nous_id: &str,
    ) -> crate::error::Result<crate::succession::KnowledgeProfile> {
        use crate::engine::DataValue;

        // Get top entities
        let mut params = std::collections::BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));

        let result = self.run_query(crate::succession::NOUS_KNOWLEDGE_PROFILE, params)?;

        // Load volatility scores for enrichment
        let volatility_scores = self.load_volatility_scores().unwrap_or_default();

        let mut top_entities = Vec::new();
        for row in &result.rows {
            let Some(entity_id) = row.first().and_then(|v| v.get_str()) else {
                continue;
            };
            let entity_name = row
                .get(1)
                .and_then(|v| v.get_str())
                .unwrap_or("")
                .to_owned();
            let fact_count = row.get(2).and_then(DataValue::get_int).unwrap_or(0);
            let avg_stability = row.get(3).and_then(DataValue::get_float).unwrap_or(0.0);

            top_entities.push(crate::succession::EntityProfile {
                entity_id: crate::id::EntityId::new_unchecked(entity_id),
                entity_name,
                fact_count: i64_to_u32(fact_count),
                avg_stability_hours: avg_stability,
                volatility_score: volatility_scores.get(entity_id).copied(),
            });
        }

        // Get overall stats
        let mut params2 = std::collections::BTreeMap::new();
        params2.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));

        let stats_result = self.run_query(crate::succession::NOUS_ACTIVE_FACT_STATS, params2)?;

        let (total_active_facts, avg_stability_hours) =
            stats_result.rows.first().map_or((0, 0.0), |row| {
                let total = row.first().and_then(DataValue::get_int).unwrap_or(0);
                let avg = row.get(1).and_then(DataValue::get_float).unwrap_or(0.0);
                (i64_to_u32(total), avg)
            });

        Ok(crate::succession::KnowledgeProfile {
            nous_id: nous_id.to_owned(),
            top_entities,
            avg_stability_hours,
            total_active_facts,
        })
    }

    /// Build a full `GraphContext` for a recall query.
    ///
    /// Loads cached graph scores, computes BFS proximity from seed entities,
    /// populates context clusters, and computes supersession chain lengths.
    ///
    /// Pass the `relationship_proximity_weight` from [`RecallWeights`](crate::recall::RecallWeights)
    /// so the pipeline can skip all graph traversal when the weight is zero.
    /// Use [`RecallWeights::graph_recall_active`](crate::recall::RecallWeights::graph_recall_active)
    /// to pre-check.
    pub fn build_graph_context(
        &self,
        seed_entity_ids: &[String],
        relationship_proximity_weight: f64,
    ) -> crate::error::Result<GraphContext> {
        // PERF: skip all graph traversal when the weight is effectively zero.
        if relationship_proximity_weight < f64::EPSILON {
            return Ok(GraphContext::default());
        }

        let mut ctx = self.load_graph_context()?;

        // Populate context_clusters from seed entities
        for seed_id in seed_entity_ids {
            if let Some(cluster_id) = ctx.clusters.get(seed_id) {
                ctx.context_clusters.insert(*cluster_id);
            }
        }

        // Compute BFS proximity
        let bfs_hops = self.compute_bfs_proximity(seed_entity_ids)?;
        for (entity_id, hops) in bfs_hops {
            ctx.proximity.insert(entity_id, Some(hops));
        }

        // Compute supersession chain lengths
        ctx.chain_lengths = self.compute_chain_lengths()?;

        Ok(ctx)
    }
}

#[cfg(test)]
#[path = "graph_intelligence_tests.rs"]
mod tests;
