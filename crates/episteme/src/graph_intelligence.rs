//! Graph-enhanced recall scoring: wires `PageRank`, `Louvain` community detection,
//! and bounded BFS proximity into the 11-factor recall pipeline.
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
        reason = "graph intelligence API wired through daemon maintenance; unused outside test harness"
    )
)]
#![cfg_attr(
    feature = "mneme-engine",
    expect(
        clippy::as_conversions,
        reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
    )
)]

use std::collections::{HashMap, HashSet};

#[cfg(feature = "mneme-engine")]
use std::collections::{BTreeMap, BinaryHeap, VecDeque};

#[cfg(feature = "mneme-engine")]
use snafu::ResultExt;

/// Canonical labels stored in `graph_scores.score_type`.
#[cfg(feature = "mneme-engine")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GraphScoreType {
    /// Normalized `PageRank` score.
    PageRank,
    /// Louvain community assignment.
    LouvainCluster,
    /// Raw maximum `PageRank` value used for normalization metadata.
    PageRankMax,
    /// Domain volatility score.
    Volatility,
}

#[cfg(feature = "mneme-engine")]
impl GraphScoreType {
    /// Stable storage label for this graph score type.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::PageRank => "pagerank",
            Self::LouvainCluster => "cluster",
            Self::PageRankMax => "pagerank_max",
            Self::Volatility => "volatility",
        }
    }
}

/// Pre-#4678 Louvain score type accepted only by the cleanup migration.
#[cfg(feature = "mneme-engine")]
pub(crate) const LEGACY_LOUVAIN_CLUSTER_SCORE_TYPE: &str = "louvain";

#[cfg(feature = "mneme-engine")]
/// Wrapper for `(cost, node)` that implements `Ord` so it can live in a
/// `BinaryHeap` for Dijkstra's algorithm.
#[derive(Debug, Clone)]
struct DistState(f64, String);

#[cfg(feature = "mneme-engine")]
impl PartialEq for DistState {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits() && self.1 == other.1
    }
}

#[cfg(feature = "mneme-engine")]
impl Eq for DistState {}

#[cfg(feature = "mneme-engine")]
impl PartialOrd for DistState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(feature = "mneme-engine")]
impl Ord for DistState {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .total_cmp(&other.0)
            .then_with(|| self.1.cmp(&other.1))
    }
}

/// Datalog DDL for the `graph_scores` relation.
///
/// Caches `PageRank` scores, community (cluster) assignments, and the `PageRank` max
/// meta-entry. Updated by background recomputation.
#[cfg_attr(
    not(feature = "mneme-engine"),
    expect(dead_code, reason = "DDL used by mneme-engine schema setup")
)]
pub(crate) const GRAPH_SCORES_DDL: &str = r":create graph_scores {
    entity_id: String, score_type: String =>
    score: Float default 0.0, cluster_id: Int default -1, updated_at: String
}";

/// Per-query snapshot of graph intelligence data.
///
/// Loaded once from the `graph_scores` relation at query entry. All fields
/// default to empty when no graph data is available, producing identical
/// scores to the non-graph baseline formula.
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
    /// Latest `updated_at` timestamp seen in loaded graph scores.
    /// Used for staleness detection.
    pub updated_at: Option<jiff::Timestamp>,
}

impl GraphContext {
    /// Returns `true` when no graph data has been loaded.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.pageranks.is_empty() && self.clusters.is_empty()
    }

    /// Returns `true` if the cached graph scores are older than the given
    /// threshold. Returns `false` when no scores have been computed.
    #[must_use]
    pub fn is_stale(&self, threshold: jiff::SignedDuration) -> bool {
        let Some(updated) = self.updated_at else {
            return true;
        };
        let now = jiff::Timestamp::now();
        let elapsed = now.duration_since(updated);
        elapsed >= threshold
    }

    /// Look up the normalized `PageRank` importance for an entity.
    /// Returns 0.0 if the entity has no `PageRank` score.
    #[must_use]
    pub(crate) fn importance(&self, entity_id: &str) -> f64 {
        self.pageranks.get(entity_id).copied().unwrap_or(0.0)
    }

    /// Check whether a given entity is in the same cluster as any query context entity.
    #[must_use]
    pub(crate) fn same_cluster(&self, entity_id: &str) -> bool {
        self.clusters
            .get(entity_id)
            .is_some_and(|cid| self.context_clusters.contains(cid))
    }

    /// Get the supersession chain length for a fact.
    #[must_use]
    pub(crate) fn chain_length(&self, fact_id: &str) -> u32 {
        self.chain_lengths.get(fact_id).copied().unwrap_or(0)
    }

    /// Get the BFS hop count for a fact.
    #[must_use]
    pub(crate) fn hops(&self, fact_id: &str) -> Option<u32> {
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
pub(crate) fn score_epistemic_tier_with_importance(base_tier_score: f64, importance: f64) -> f64 {
    let boost = 1.0 + (importance.clamp(0.0, 1.0) * 0.5); // [1.0, 1.5]
    (base_tier_score * boost).min(1.0)
}

/// Community-aware relationship proximity scoring.
///
/// Same-cluster facts get a proximity floor of 0.3 even if no direct path exists.
/// This reflects that entities in the same community are semantically related.
#[must_use]
pub(crate) fn score_relationship_proximity_with_cluster(
    base_hop_score: f64,
    same_cluster: bool,
) -> f64 {
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
pub(crate) fn score_access_with_evolution(base_access_score: f64, chain_length: u32) -> f64 {
    let evolution_bonus = (f64::from(chain_length) * 0.05).min(0.2);
    (base_access_score + evolution_bonus).min(1.0)
}

/// Combined Datalog script for `PageRank` + `Louvain` community detection.
///
/// Reads the `relationships` relation, computes `PageRank` and `Louvain` communities,
/// and stores results into `graph_scores`.
///
/// Parameters: `$now` (ISO 8601 timestamp string), plus canonical graph score
/// type labels.
#[cfg_attr(
    not(feature = "mneme-engine"),
    expect(
        dead_code,
        reason = "Datalog query used by mneme-engine graph pipeline"
    )
)]
pub(crate) const RECOMPUTE_GRAPH_SCORES: &str = r"
edges[src, dst] := *relationships{src, dst}
edges_w[src, dst, weight] := *relationships{src, dst, weight}

pr[entity_id, score] <~ PageRank(edges[])

pr_max[max(score)] := pr[_, score]

comm[labels, entity_id] <~ CommunityDetectionLouvain(edges_w[])

?[entity_id, score_type, score, cluster_id, updated_at] :=
    pr[entity_id, raw_score], pr_max[m], m > 0,
    score = raw_score / m, score_type = $pagerank_score_type, cluster_id = -1, updated_at = $now

?[entity_id, score_type, score, cluster_id, updated_at] :=
    comm[labels, entity_id], length(labels) > 0, cid = first(labels),
    score_type = $cluster_score_type, score = 0.0, cluster_id = cid, updated_at = $now

?[entity_id, score_type, score, cluster_id, updated_at] :=
    pr_max[m], m > 0, entity_id = '__meta__', score_type = $pagerank_max_score_type,
    score = m, cluster_id = -1, updated_at = $now

:put graph_scores { entity_id, score_type => score, cluster_id, updated_at }
";

/// Datalog script to load all graph scores into memory.
#[cfg_attr(
    not(feature = "mneme-engine"),
    expect(
        dead_code,
        reason = "Datalog query used by mneme-engine graph pipeline"
    )
)]
pub(crate) const LOAD_GRAPH_SCORES: &str = r"
?[entity_id, score_type, score, cluster_id, updated_at] :=
    *graph_scores{entity_id, score_type, score, cluster_id, updated_at}
";

/// Maximum hop distance returned by [`KnowledgeStore::compute_bfs_proximity`].
///
/// Entities reachable at a greater distance from any seed are excluded from
/// the result. Matches the historical 4-hop Datalog bound (`hop0`..`hop4`).
#[cfg_attr(
    not(feature = "mneme-engine"),
    expect(
        dead_code,
        reason = "bound consumed by mneme-engine BFS proximity traversal"
    )
)]
pub(crate) const BFS_PROXIMITY_MAX_HOPS: u32 = 4;

/// Datalog script for computing supersession chain lengths.
///
/// Counts how many predecessors each fact has in its supersession chain.
#[cfg_attr(
    not(feature = "mneme-engine"),
    expect(
        dead_code,
        reason = "Datalog query used by mneme-engine graph pipeline"
    )
)]
pub(crate) const SUPERSESSION_CHAIN_LENGTHS: &str = r"
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
pub(crate) struct GraphDirtyFlag {
    inner: std::sync::atomic::AtomicBool,
}

impl GraphDirtyFlag {
    /// Create a new flag, initially clean.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            inner: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Mark the graph as dirty (needs recomputation).
    pub(crate) fn mark_dirty(&self) {
        self.inner.store(true, std::sync::atomic::Ordering::Release);
    }

    /// Check if the graph is dirty and clear the flag atomically.
    /// Returns `true` if it was dirty.
    pub(crate) fn take_dirty(&self) -> bool {
        self.inner.swap(false, std::sync::atomic::Ordering::AcqRel)
    }

    /// Check if the graph is dirty without clearing.
    #[must_use]
    pub(crate) fn is_dirty(&self) -> bool {
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
fn i64_to_u32(v: i64) -> u32 {
    // WHY: clamped to [0, u32::MAX] before cast; sign loss and truncation are impossible.
    #[expect(
        clippy::cast_sign_loss,
        clippy::cast_possible_truncation,
        reason = "value is clamped to [0, u32::MAX] before the as-cast"
    )]
    let result = v.clamp(0, i64::from(u32::MAX)) as u32;
    result
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
    #[expect(dead_code, reason = "called from schema setup, dead in lib test build")]
    /// Initialize the `graph_scores` relation. Called during schema setup.
    pub(crate) fn init_graph_scores(&self) -> crate::error::Result<()> {
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

            if let Some(ts) = row
                .get(4)
                .and_then(|v| v.get_str())
                .and_then(crate::knowledge::parse_timestamp)
            {
                ctx.updated_at = Some(
                    ctx.updated_at
                        .map_or(ts, |existing| if ts > existing { ts } else { existing }),
                );
            }

            if score_type == GraphScoreType::PageRank.as_str() {
                ctx.pageranks.insert(entity_id.to_owned(), score);
            } else if score_type == GraphScoreType::LouvainCluster.as_str() {
                ctx.clusters.insert(entity_id.to_owned(), cluster_id);
            } else {
                match score_type {
                    s if s == GraphScoreType::PageRankMax.as_str()
                        || s == GraphScoreType::Volatility.as_str() =>
                    {
                        // NOTE: pagerank_max is metadata and volatility is loaded
                        // through succession; neither populates recall clusters.
                    }
                    _ => {
                        // NOTE: unknown score types are ignored so forward-added
                        // graph metrics do not break recall context loading.
                    }
                }
            }
        }

        Ok(ctx)
    }

    /// Compute BFS proximity from seed entities up to [`BFS_PROXIMITY_MAX_HOPS`]
    /// hops.
    ///
    /// Loads the full relationship set once, then runs a standard queue-based
    /// multi-source BFS in-process. Seeds are included at hop 0. First-visit
    /// wins, so cycles terminate naturally and shortest-path hop counts are
    /// preserved.
    ///
    /// WHY in-process rather than Datalog: the previous implementation issued a
    /// 4-hop recursive Datalog query under a 5 ms wall-clock budget and, on
    /// timeout, degraded to a buggy 2-hop fallback (wrong column index, wrong
    /// horizon). The timeout fired on slower CI runners (notably macOS),
    /// producing platform-dependent hop counts. A direct traversal over the
    /// materialized adjacency list is both faster for small/medium graphs and
    /// platform-independent.
    pub(crate) fn compute_bfs_proximity(
        &self,
        seed_entity_ids: &[String],
    ) -> crate::error::Result<HashMap<String, u32>> {
        if seed_entity_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let adj = self.build_adjacency_list()?;

        let mut hops: HashMap<String, u32> = HashMap::new();
        let mut queue: VecDeque<(String, u32)> = VecDeque::new();

        for seed in seed_entity_ids {
            // Multi-source BFS: every seed starts at hop 0. `or_insert(0)` keeps
            // duplicate seeds idempotent.
            if hops.insert(seed.clone(), 0).is_none() {
                queue.push_back((seed.clone(), 0));
            }
        }

        while let Some((node, dist)) = queue.pop_front() {
            if dist >= BFS_PROXIMITY_MAX_HOPS {
                // Respect the 4-hop bound: do not expand past the horizon so
                // entities at hop 5+ never appear in the result.
                continue;
            }
            let Some(neighbors) = adj.get(&node) else {
                continue;
            };
            let next = dist + 1;
            for (neighbor, _weight) in neighbors {
                if let std::collections::hash_map::Entry::Vacant(slot) =
                    hops.entry(neighbor.clone())
                {
                    slot.insert(next);
                    queue.push_back((neighbor.clone(), next));
                }
            }
        }

        Ok(hops)
    }

    /// Compute supersession chain lengths for all facts.
    pub(crate) fn compute_chain_lengths(&self) -> crate::error::Result<HashMap<String, u32>> {
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
        params.insert(
            "pagerank_score_type".to_owned(),
            crate::engine::DataValue::Str(GraphScoreType::PageRank.as_str().into()),
        );
        params.insert(
            "cluster_score_type".to_owned(),
            crate::engine::DataValue::Str(GraphScoreType::LouvainCluster.as_str().into()),
        );
        params.insert(
            "pagerank_max_score_type".to_owned(),
            crate::engine::DataValue::Str(GraphScoreType::PageRankMax.as_str().into()),
        );
        self.run_mut_query(RECOMPUTE_GRAPH_SCORES, params)?;
        Ok(())
    }

    /// Compute per-entity domain volatility from supersession patterns.
    ///
    /// Joins `facts`, `fact_entities`, and supersession chain data to produce
    /// `DomainVolatility` scores for all entities with linked facts.
    pub(crate) fn compute_domain_volatility(
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

            let eid = crate::id::EntityId::new(entity_id).context(crate::error::InvalidIdSnafu)?;
            volatilities.push(crate::succession::DomainVolatility {
                entity_id: eid,
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
            params.insert(
                "volatility_score_type".to_owned(),
                crate::engine::DataValue::Str(GraphScoreType::Volatility.as_str().into()),
            );
            self.run_mut_query(crate::succession::STORE_VOLATILITY_SCORE, params)?;
        }

        Ok(())
    }

    /// Load volatility scores from `graph_scores`.
    ///
    /// Returns a map of `entity_id → volatility_score` for all entities
    /// that have stored volatility data.
    pub(crate) fn load_volatility_scores(&self) -> crate::error::Result<HashMap<String, f64>> {
        let result = self.run_query(
            r"?[entity_id, score] := *graph_scores{entity_id, score_type, score}, score_type == $volatility_score_type",
            {
                let mut params = std::collections::BTreeMap::new();
                params.insert(
                    "volatility_score_type".to_owned(),
                    crate::engine::DataValue::Str(GraphScoreType::Volatility.as_str().into()),
                );
                params
            },
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
    pub(crate) fn nous_knowledge_profile(
        &self,
        nous_id: &str,
    ) -> crate::error::Result<crate::succession::KnowledgeProfile> {
        use crate::engine::DataValue;

        let mut params = std::collections::BTreeMap::new();
        params.insert("nous_id".to_owned(), DataValue::Str(nous_id.into()));

        let result = self.run_query(crate::succession::NOUS_KNOWLEDGE_PROFILE, params)?;

        // kanon:ignore RUST/no-result-unwrap-or-default — knowledge profile is best-effort context; if volatility load fails we proceed with empty scores rather than propagate.
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

            let eid = crate::id::EntityId::new(entity_id).context(crate::error::InvalidIdSnafu)?;
            top_entities.push(crate::succession::EntityProfile {
                entity_id: eid,
                entity_name,
                fact_count: i64_to_u32(fact_count),
                avg_stability_hours: avg_stability,
                volatility_score: volatility_scores.get(entity_id).copied(),
            });
        }

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
    /// Graph data is loaded regardless of weight settings so that callers
    /// can always apply graph signals when scores are available (#3432).
    pub(crate) fn build_graph_context(
        &self,
        seed_entity_ids: &[String],
    ) -> crate::error::Result<GraphContext> {
        let mut ctx = self.load_graph_context()?;

        for seed_id in seed_entity_ids {
            if let Some(cluster_id) = ctx.clusters.get(seed_id) {
                ctx.context_clusters.insert(*cluster_id);
            }
        }

        let bfs_hops = self.compute_bfs_proximity(seed_entity_ids)?;
        for (entity_id, hops) in bfs_hops {
            ctx.proximity.insert(entity_id, Some(hops));
        }

        ctx.chain_lengths = self.compute_chain_lengths()?;

        Ok(ctx)
    }

    /// Compute betweenness centrality for all entities in the knowledge graph.
    ///
    /// Uses Brandes' algorithm on an undirected view of the relationship graph.
    /// Higher scores indicate entities that act as "hubs" — they lie on many
    /// shortest paths between other entities.
    ///
    /// Returns a map of `entity_id → normalized centrality score` in [0.0, 1.0].
    pub fn compute_centrality(&self) -> BTreeMap<crate::id::EntityId, f64> {
        let Ok(entities) = self.list_entities() else {
            return BTreeMap::new();
        };
        let Ok(adj) = self.build_undirected_adjacency_list() else {
            return BTreeMap::new();
        };

        let mut centrality: HashMap<String, f64> = entities
            .iter()
            .map(|e| (e.id.as_str().to_owned(), 0.0))
            .collect();

        let n = entities.len();
        if n < 3 {
            return centrality
                .into_iter()
                .filter_map(|(k, v)| crate::id::EntityId::new(k).ok().map(|id| (id, v)))
                .collect();
        }

        for source in &entities {
            let s = source.id.as_str();

            let mut dist: HashMap<String, i32> = HashMap::new();
            let mut sigma: HashMap<String, f64> = HashMap::new();
            let mut pred: HashMap<String, Vec<String>> = HashMap::new();
            let mut queue = VecDeque::new();
            let mut stack = Vec::new();

            dist.insert(s.to_owned(), 0);
            sigma.insert(s.to_owned(), 1.0);
            queue.push_back(s.to_owned());

            while let Some(v) = queue.pop_front() {
                stack.push(v.clone());
                let dist_v = *dist.get(&v).unwrap_or(&0);
                if let Some(neighbors) = adj.get(&v) {
                    for (w, _weight) in neighbors {
                        if !dist.contains_key(w) {
                            dist.insert(w.clone(), dist_v + 1);
                            queue.push_back(w.clone());
                        }
                        let dist_w = *dist.get(w).unwrap_or(&-1);
                        if dist_w == dist_v + 1 {
                            let sigma_v = sigma.get(&v).copied().unwrap_or(0.0);
                            *sigma.entry(w.clone()).or_insert(0.0) += sigma_v;
                            pred.entry(w.clone()).or_default().push(v.clone());
                        }
                    }
                }
            }

            let mut delta: HashMap<String, f64> = entities
                .iter()
                .map(|e| (e.id.as_str().to_owned(), 0.0))
                .collect();

            while let Some(w) = stack.pop() {
                let delta_w = delta.get(&w).copied().unwrap_or(0.0);
                if let Some(preds) = pred.get(&w) {
                    let sigma_w = sigma.get(&w).copied().unwrap_or(1.0);
                    if sigma_w > 0.0 {
                        for v in preds {
                            let sigma_v = sigma.get(v).copied().unwrap_or(0.0);
                            let coeff = (sigma_v / sigma_w) * (1.0 + delta_w);
                            if let Some(dv) = delta.get_mut(v) {
                                *dv += coeff;
                            }
                        }
                    }
                }
                if w != s
                    && let Some(cv) = centrality.get_mut(&w)
                {
                    *cv += delta_w;
                }
            }
        }

        // For undirected graphs each pair is counted twice.
        for score in centrality.values_mut() {
            *score /= 2.0;
        }

        // Normalize to [0.0, 1.0] using the theoretical maximum for undirected graphs.
        let max_possible = if n > 2 {
            #[expect(
                clippy::cast_precision_loss,
                reason = "n is entity count; precision loss for graphs > 2^52 nodes is impossible"
            )]
            let n_f64 = n as f64;
            (n_f64 - 1.0) * (n_f64 - 2.0) / 2.0
        } else {
            1.0
        };

        if max_possible > 0.0 {
            for score in centrality.values_mut() {
                *score /= max_possible;
            }
        }

        centrality
            .into_iter()
            .filter_map(|(k, v)| crate::id::EntityId::new(k).ok().map(|id| (id, v)))
            .collect()
    }

    /// Find the shortest directed path between two entities weighted by
    /// relationship confidence.
    ///
    /// Edge cost is `1.0 - confidence` so that stronger relationships produce
    /// shorter paths. Returns `None` when no path exists.
    pub fn shortest_path(
        &self,
        from: &crate::id::EntityId,
        to: &crate::id::EntityId,
    ) -> Option<Vec<crate::id::EntityId>> {
        if from == to {
            return Some(vec![from.clone()]);
        }

        let adj = self.build_adjacency_list().ok()?;
        let start = from.as_str();
        let goal = to.as_str();

        let mut dist: HashMap<String, f64> = HashMap::new();
        let mut prev: HashMap<String, String> = HashMap::new();
        let mut heap = BinaryHeap::new();

        dist.insert(start.to_owned(), 0.0);
        heap.push(DistState(0.0, start.to_owned()));

        while let Some(DistState(d, u)) = heap.pop() {
            if u == goal {
                break;
            }
            if d > *dist.get(&u).unwrap_or(&f64::INFINITY) {
                continue;
            }
            if let Some(neighbors) = adj.get(&u) {
                for (v, weight) in neighbors {
                    let cost = 1.0 - weight.clamp(0.0, 1.0);
                    let alt = d + cost;
                    if alt < *dist.get(v).unwrap_or(&f64::INFINITY) {
                        dist.insert(v.clone(), alt);
                        prev.insert(v.clone(), u.clone());
                        heap.push(DistState(alt, v.clone()));
                    }
                }
            }
        }

        if !prev.contains_key(goal) {
            return None;
        }

        let mut path = Vec::new();
        let mut current = goal.to_owned();
        path.push(current.clone());
        while let Some(p) = prev.get(&current) {
            current = p.clone();
            path.push(current.clone());
            if current == start {
                break;
            }
        }

        path.reverse();
        path.into_iter()
            .map(|s| crate::id::EntityId::new(s).ok())
            .collect::<Option<Vec<_>>>()
    }

    /// Find weakly connected components in the knowledge graph.
    ///
    /// Treats relationships as undirected edges. Isolated entities form
    /// singleton components.
    pub fn connected_components(&self) -> Vec<Vec<crate::id::EntityId>> {
        let Ok(entities) = self.list_entities() else {
            return Vec::new();
        };
        let Ok(adj) = self.build_undirected_adjacency_list() else {
            return Vec::new();
        };

        let mut visited: HashSet<String> = HashSet::new();
        let mut components: Vec<Vec<crate::id::EntityId>> = Vec::new();

        for entity in &entities {
            let id = entity.id.as_str();
            if visited.contains(id) {
                continue;
            }

            let mut component = Vec::new();
            let mut stack = vec![id.to_owned()];

            while let Some(node) = stack.pop() {
                if !visited.insert(node.clone()) {
                    continue;
                }
                if let Ok(eid) = crate::id::EntityId::new(&node) {
                    component.push(eid);
                }

                if let Some(neighbors) = adj.get(&node) {
                    for (neighbor, _weight) in neighbors {
                        if !visited.contains(neighbor) {
                            stack.push(neighbor.clone());
                        }
                    }
                }
            }

            component.sort_by(|a, b| a.as_str().cmp(b.as_str()));
            components.push(component);
        }

        components.sort_by(|a, b| {
            let a_first = a.first().map_or("", eidos::id::EntityId::as_str);
            let b_first = b.first().map_or("", eidos::id::EntityId::as_str);
            a_first.cmp(b_first)
        });

        components
    }

    /// Compute multi-source BFS proximity with exponential distance decay.
    ///
    /// Each seed starts with score `1.0`. Every hop multiplies the score by
    /// `decay`. The result is a soft-recall map where distant entities still
    /// receive a non-zero score that falls off geometrically.
    ///
    /// Cycles are handled via a visited set; the first (shortest) distance
    /// from any seed wins.
    pub fn compute_bfs_proximity_decay(
        &self,
        seeds: &[crate::id::EntityId],
        decay: f64,
    ) -> BTreeMap<crate::id::EntityId, f64> {
        if seeds.is_empty() {
            return BTreeMap::new();
        }

        let Ok(adj) = self.build_adjacency_list() else {
            return BTreeMap::new();
        };

        let decay = decay.clamp(0.0, 1.0);
        let mut scores = BTreeMap::new();
        let mut visited: HashMap<String, u32> = HashMap::new();
        let mut queue = VecDeque::new();

        for seed in seeds {
            let s = seed.as_str().to_owned();
            scores.insert(seed.clone(), 1.0);
            visited.insert(s.clone(), 0);
            queue.push_back((s, 0));
        }

        while let Some((node, dist)) = queue.pop_front() {
            if let Some(neighbors) = adj.get(&node) {
                for (neighbor, _weight) in neighbors {
                    if !visited.contains_key(neighbor) {
                        let new_dist = dist + 1;
                        visited.insert(neighbor.clone(), new_dist);
                        let score = decay.powi(new_dist.cast_signed());
                        if let Ok(eid) = crate::id::EntityId::new(neighbor) {
                            scores.insert(eid, score);
                        }
                        queue.push_back((neighbor.clone(), new_dist));
                    }
                }
            }
        }

        scores
    }

    /// Load all relationships from the store.
    fn load_relationships(&self) -> crate::error::Result<Vec<(String, String, f64)>> {
        let script = r"?[src, dst, weight] := *relationships{src, dst, relation, weight}";
        let result = self.run_query(script, std::collections::BTreeMap::new())?;
        let mut rels = Vec::new();
        for row in &result.rows {
            if row.len() < 3 {
                continue;
            }
            let src = row.first().and_then(|v| v.get_str()).unwrap_or("");
            let dst = row.get(1).and_then(|v| v.get_str()).unwrap_or("");
            let weight = row
                .get(2)
                .and_then(crate::engine::DataValue::get_float)
                .unwrap_or(0.0);
            rels.push((src.to_owned(), dst.to_owned(), weight));
        }
        Ok(rels)
    }

    /// Build a directed adjacency list from the relationship table.
    fn build_adjacency_list(&self) -> crate::error::Result<HashMap<String, Vec<(String, f64)>>> {
        let rels = self.load_relationships()?;
        let mut adj = HashMap::<String, Vec<(String, f64)>>::new();
        for (src, dst, weight) in rels {
            adj.entry(src).or_default().push((dst, weight));
        }
        Ok(adj)
    }

    /// Build an undirected adjacency list from the relationship table.
    fn build_undirected_adjacency_list(
        &self,
    ) -> crate::error::Result<HashMap<String, Vec<(String, f64)>>> {
        let rels = self.load_relationships()?;
        let mut adj = HashMap::new();
        for (src, dst, weight) in rels {
            adj.entry(src.clone())
                .or_insert_with(Vec::new)
                .push((dst.clone(), weight));
            adj.entry(dst).or_insert_with(Vec::new).push((src, weight));
        }
        Ok(adj)
    }
}

#[cfg(test)]
#[path = "graph_intelligence_tests/mod.rs"]
mod tests;
