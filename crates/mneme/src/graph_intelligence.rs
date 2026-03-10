//! Graph-enhanced recall scoring — wires `PageRank`, `Louvain` community detection,
//! and bounded BFS proximity into the 6-factor recall pipeline.
//!
//! This module provides:
//! - [`GraphContext`]: per-query snapshot of graph scores loaded from `graph_scores` relation
//! - Enhanced scoring functions that augment epistemic tier, relationship proximity, and access frequency
//! - Background recomputation of `PageRank` + `Louvain` stored in `graph_scores`
//! - Cache invalidation via [`GraphDirtyFlag`]

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
/// Boost range: [1.0, 1.5] — at importance=0.0 the score is unchanged.
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

    /// Load a [`GraphContext`] from the `graph_scores` relation.
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
                // pagerank_max meta entry — normalization already done in Datalog
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

    /// Build a full [`GraphContext`] for a recall query.
    ///
    /// Loads cached graph scores, computes BFS proximity from seed entities,
    /// populates context clusters, and computes supersession chain lengths.
    pub fn build_graph_context(
        &self,
        seed_entity_ids: &[String],
    ) -> crate::error::Result<GraphContext> {
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
mod tests {
    use super::*;

    // --- Pure scoring function tests (no engine needed) ---

    #[test]
    fn pagerank_boost_zero_importance_unchanged() {
        let base = 0.6; // inferred tier
        let result = score_epistemic_tier_with_importance(base, 0.0);
        assert!(
            (result - base).abs() < f64::EPSILON,
            "zero importance should not change score, got {result}"
        );
    }

    #[test]
    fn pagerank_boost_max_importance() {
        let base = 0.6;
        let result = score_epistemic_tier_with_importance(base, 1.0);
        // boost = 1.5, so 0.6 * 1.5 = 0.9
        assert!(
            (result - 0.9).abs() < f64::EPSILON,
            "max importance should give 1.5x boost, got {result}"
        );
    }

    #[test]
    fn pagerank_boost_clamped_to_one() {
        let base = 1.0; // verified tier
        let result = score_epistemic_tier_with_importance(base, 1.0);
        // 1.0 * 1.5 = 1.5, clamped to 1.0
        assert!(
            (result - 1.0).abs() < f64::EPSILON,
            "should clamp to 1.0, got {result}"
        );
    }

    #[test]
    fn pagerank_boost_hub_higher_than_peripheral() {
        let base = 0.6;
        let hub = score_epistemic_tier_with_importance(base, 0.9);
        let peripheral = score_epistemic_tier_with_importance(base, 0.1);
        assert!(
            hub > peripheral,
            "hub ({hub}) should score higher than peripheral ({peripheral})"
        );
    }

    #[test]
    fn pagerank_boost_range() {
        // Importance [0, 1] → boost [1.0, 1.5]
        for i in 0..=10 {
            let importance = f64::from(i) / 10.0;
            let result = score_epistemic_tier_with_importance(0.5, importance);
            let expected_boost = 1.0 + importance * 0.5;
            let expected = (0.5 * expected_boost).min(1.0);
            assert!(
                (result - expected).abs() < 1e-10,
                "importance={importance}: expected {expected}, got {result}"
            );
        }
    }

    #[test]
    fn cluster_floor_same_cluster_no_path() {
        // No direct path (base_hop_score = 0.0), but same cluster → floor 0.3
        let result = score_relationship_proximity_with_cluster(0.0, true);
        assert!(
            (result - 0.3).abs() < f64::EPSILON,
            "same-cluster with no path should get 0.3 floor, got {result}"
        );
    }

    #[test]
    fn cluster_floor_same_cluster_with_path() {
        // Direct neighbor (base = 1.0), same cluster → stays 1.0
        let result = score_relationship_proximity_with_cluster(1.0, true);
        assert!(
            (result - 1.0).abs() < f64::EPSILON,
            "same-cluster direct neighbor should stay 1.0, got {result}"
        );
    }

    #[test]
    fn cluster_floor_different_cluster() {
        // No path, different cluster → stays 0.0
        let result = score_relationship_proximity_with_cluster(0.0, false);
        assert!(
            (result).abs() < f64::EPSILON,
            "different-cluster with no path should stay 0.0, got {result}"
        );
    }

    #[test]
    fn cluster_floor_partial_path() {
        // 2-hop (0.5), same cluster → stays 0.5 (above floor)
        let result = score_relationship_proximity_with_cluster(0.5, true);
        assert!(
            (result - 0.5).abs() < f64::EPSILON,
            "same-cluster 2-hop should stay 0.5, got {result}"
        );
    }

    #[test]
    fn supersession_bonus_zero_chain() {
        let result = score_access_with_evolution(0.5, 0);
        assert!(
            (result - 0.5).abs() < f64::EPSILON,
            "zero chain should not change score, got {result}"
        );
    }

    #[test]
    fn supersession_bonus_chain_four() {
        let result = score_access_with_evolution(0.5, 4);
        // bonus = 4 * 0.05 = 0.2
        assert!(
            (result - 0.7).abs() < f64::EPSILON,
            "chain_length=4 should add 0.2, got {result}"
        );
    }

    #[test]
    fn supersession_bonus_capped() {
        // chain_length=10, bonus would be 0.5 but capped at 0.2
        let result = score_access_with_evolution(0.5, 10);
        assert!(
            (result - 0.7).abs() < f64::EPSILON,
            "bonus should be capped at 0.2, got {result}"
        );
    }

    #[test]
    fn supersession_bonus_higher_chain_scores_higher() {
        let base = 0.3;
        let short = score_access_with_evolution(base, 0);
        let long = score_access_with_evolution(base, 4);
        assert!(
            long > short,
            "chain_length=4 ({long}) should score higher than chain_length=0 ({short})"
        );
    }

    #[test]
    fn supersession_bonus_clamped_to_one() {
        let result = score_access_with_evolution(0.95, 4);
        assert!(
            (result - 1.0).abs() < f64::EPSILON,
            "should clamp to 1.0, got {result}"
        );
    }

    #[test]
    fn backward_compat_empty_context() {
        // With empty GraphContext, enhanced scores should equal base scores
        let ctx = GraphContext::default();
        assert!(ctx.is_empty());

        let base_tier = 0.6;
        let enhanced_tier =
            score_epistemic_tier_with_importance(base_tier, ctx.importance("any_entity"));
        assert!(
            (enhanced_tier - base_tier).abs() < f64::EPSILON,
            "empty context tier should match base"
        );

        let base_prox = 0.0;
        let enhanced_prox =
            score_relationship_proximity_with_cluster(base_prox, ctx.same_cluster("any_entity"));
        assert!(
            (enhanced_prox - base_prox).abs() < f64::EPSILON,
            "empty context proximity should match base"
        );

        let base_access = 0.5;
        let enhanced_access =
            score_access_with_evolution(base_access, ctx.chain_length("any_fact"));
        assert!(
            (enhanced_access - base_access).abs() < f64::EPSILON,
            "empty context access should match base"
        );
    }

    #[test]
    fn graph_context_same_cluster_populated() {
        let mut ctx = GraphContext::default();
        ctx.clusters.insert("alice".to_owned(), 1);
        ctx.clusters.insert("bob".to_owned(), 1);
        ctx.clusters.insert("charlie".to_owned(), 2);
        ctx.context_clusters.insert(1);

        assert!(ctx.same_cluster("alice"));
        assert!(ctx.same_cluster("bob"));
        assert!(!ctx.same_cluster("charlie"));
        assert!(!ctx.same_cluster("unknown"));
    }

    #[test]
    fn graph_dirty_flag_lifecycle() {
        let flag = GraphDirtyFlag::new();
        assert!(!flag.is_dirty());
        assert!(!flag.take_dirty());

        flag.mark_dirty();
        assert!(flag.is_dirty());
        assert!(flag.take_dirty());

        // After take, should be clean
        assert!(!flag.is_dirty());
        assert!(!flag.take_dirty());
    }

    #[test]
    fn graph_dirty_flag_multiple_marks() {
        let flag = GraphDirtyFlag::new();
        flag.mark_dirty();
        flag.mark_dirty();
        flag.mark_dirty();
        // Single take should clear
        assert!(flag.take_dirty());
        assert!(!flag.is_dirty());
    }

    // --- Engine-dependent tests ---

    #[cfg(feature = "mneme-engine")]
    mod engine_tests {
        use crate::knowledge::{Entity, Relationship};
        use crate::knowledge_store::KnowledgeStore;

        fn test_store() -> std::sync::Arc<KnowledgeStore> {
            // graph_scores is created by init_schema automatically
            KnowledgeStore::open_mem().expect("open_mem")
        }

        fn make_entity(id: &str, name: &str) -> Entity {
            Entity {
                id: crate::id::EntityId::new_unchecked(id),
                name: name.to_owned(),
                entity_type: "person".to_owned(),
                aliases: vec![],
                created_at: jiff::Timestamp::now(),
                updated_at: jiff::Timestamp::now(),
            }
        }

        fn make_relationship(src: &str, dst: &str, relation: &str, weight: f64) -> Relationship {
            Relationship {
                src: crate::id::EntityId::new_unchecked(src),
                dst: crate::id::EntityId::new_unchecked(dst),
                relation: relation.to_owned(),
                weight,
                created_at: jiff::Timestamp::now(),
            }
        }

        #[test]
        fn graph_scores_relation_created_by_init_schema() {
            let store = test_store();
            // graph_scores created during init_schema — query should succeed
            let ctx = store.load_graph_context().expect("load_graph_context");
            assert!(ctx.is_empty());
        }

        #[test]
        fn recompute_with_entities_and_relationships() {
            let store = test_store();

            // Insert entities
            store
                .insert_entity(&make_entity("alice", "Alice"))
                .expect("insert alice");
            store
                .insert_entity(&make_entity("bob", "Bob"))
                .expect("insert bob");
            store
                .insert_entity(&make_entity("charlie", "Charlie"))
                .expect("insert charlie");

            // Insert relationships forming a hub at alice
            store
                .insert_relationship(&make_relationship("alice", "bob", "KNOWS", 0.8))
                .expect("insert rel 1");
            store
                .insert_relationship(&make_relationship("alice", "charlie", "KNOWS", 0.7))
                .expect("insert rel 2");
            store
                .insert_relationship(&make_relationship("bob", "alice", "KNOWS", 0.8))
                .expect("insert rel 3");

            // Recompute
            store
                .recompute_graph_scores()
                .expect("recompute_graph_scores");

            // Load context
            let ctx = store.load_graph_context().expect("load_graph_context");
            assert!(!ctx.is_empty());

            // Alice should have highest pagerank (hub)
            let alice_pr = ctx.importance("alice");
            let bob_pr = ctx.importance("bob");
            let charlie_pr = ctx.importance("charlie");
            assert!(
                alice_pr > bob_pr,
                "alice ({alice_pr}) should have higher PR than bob ({bob_pr})"
            );
            assert!(
                alice_pr > charlie_pr,
                "alice ({alice_pr}) should have higher PR than charlie ({charlie_pr})"
            );
            // All pageranks should be in [0, 1]
            assert!(alice_pr <= 1.0);
            assert!(bob_pr >= 0.0);
        }

        #[test]
        fn bfs_proximity_hop_counts() {
            let store = test_store();

            // Chain: a -> b -> c -> d -> e
            for (id, name) in [("a", "A"), ("b", "B"), ("c", "C"), ("d", "D"), ("e", "E")] {
                store
                    .insert_entity(&make_entity(id, name))
                    .expect("insert entity");
            }
            for (src, dst) in [("a", "b"), ("b", "c"), ("c", "d"), ("d", "e")] {
                store
                    .insert_relationship(&make_relationship(src, dst, "NEXT", 0.5))
                    .expect("insert rel");
            }

            let proximity = store.compute_bfs_proximity(&["a".to_owned()]).expect("bfs");

            // a=0, b=1, c=2, d=3, e=4
            assert_eq!(proximity.get("a").copied(), Some(0));
            assert_eq!(proximity.get("b").copied(), Some(1));
            assert_eq!(proximity.get("c").copied(), Some(2));
            assert_eq!(proximity.get("d").copied(), Some(3));
            assert_eq!(proximity.get("e").copied(), Some(4));
        }

        #[test]
        fn bfs_proximity_empty_seeds() {
            let store = test_store();
            let proximity = store.compute_bfs_proximity(&[]).expect("bfs empty");
            assert!(proximity.is_empty());
        }

        #[test]
        fn bfs_proximity_single_entity_no_relationships() {
            let store = test_store();
            store
                .insert_entity(&make_entity("lonely", "Lonely"))
                .expect("insert");
            let proximity = store
                .compute_bfs_proximity(&["lonely".to_owned()])
                .expect("bfs");
            // Only the seed itself at hop 0
            assert_eq!(proximity.get("lonely").copied(), Some(0));
            assert_eq!(proximity.len(), 1);
        }

        #[test]
        fn build_graph_context_populates_clusters() {
            let store = test_store();

            // Create a small graph with two clusters
            for (id, name) in [("a1", "A1"), ("a2", "A2"), ("b1", "B1"), ("b2", "B2")] {
                store
                    .insert_entity(&make_entity(id, name))
                    .expect("insert entity");
            }
            // Cluster A: a1 <-> a2 (strongly connected)
            store
                .insert_relationship(&make_relationship("a1", "a2", "WORKS_WITH", 0.9))
                .expect("insert");
            store
                .insert_relationship(&make_relationship("a2", "a1", "WORKS_WITH", 0.9))
                .expect("insert");
            // Cluster B: b1 <-> b2
            store
                .insert_relationship(&make_relationship("b1", "b2", "WORKS_WITH", 0.9))
                .expect("insert");
            store
                .insert_relationship(&make_relationship("b2", "b1", "WORKS_WITH", 0.9))
                .expect("insert");
            // Weak link between clusters
            store
                .insert_relationship(&make_relationship("a2", "b1", "KNOWS", 0.1))
                .expect("insert");

            store.recompute_graph_scores().expect("recompute");

            // Build context with a1 as seed
            let ctx = store
                .build_graph_context(&["a1".to_owned()])
                .expect("build_graph_context");

            // a1 should be a seed → its cluster is in context_clusters
            assert!(!ctx.context_clusters.is_empty());
            // a2 should be in same cluster as a1
            assert!(
                ctx.same_cluster("a2"),
                "a2 should be in same cluster as seed a1"
            );
        }

        #[test]
        fn recompute_empty_graph() {
            let store = test_store();
            // Should not panic on empty graph
            store
                .recompute_graph_scores()
                .expect("recompute empty graph");
            let ctx = store.load_graph_context().expect("load");
            assert!(ctx.is_empty());
        }

        #[test]
        fn pagerank_boost_integration() {
            let store = test_store();

            // Hub entity with many connections
            store
                .insert_entity(&make_entity("hub", "Hub"))
                .expect("insert");
            store
                .insert_entity(&make_entity("leaf1", "Leaf1"))
                .expect("insert");
            store
                .insert_entity(&make_entity("leaf2", "Leaf2"))
                .expect("insert");
            store
                .insert_entity(&make_entity("leaf3", "Leaf3"))
                .expect("insert");

            for leaf in ["leaf1", "leaf2", "leaf3"] {
                store
                    .insert_relationship(&make_relationship(leaf, "hub", "KNOWS", 0.8))
                    .expect("insert rel");
            }

            store.recompute_graph_scores().expect("recompute");

            let ctx = store.load_graph_context().expect("load");
            let hub_importance = ctx.importance("hub");
            let leaf_importance = ctx.importance("leaf1");

            // Use the enhanced scoring
            let base_tier = 0.6; // inferred
            let hub_score = super::score_epistemic_tier_with_importance(base_tier, hub_importance);
            let leaf_score =
                super::score_epistemic_tier_with_importance(base_tier, leaf_importance);

            assert!(
                hub_score > leaf_score,
                "hub entity fact ({hub_score}) should score higher than leaf ({leaf_score})"
            );
        }
    }
}
