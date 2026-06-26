//! Louvain hierarchical community detection.
//!
//! Two-phase iterative algorithm: (1) greedily move nodes between communities
//! to maximise modularity gain, (2) coarsen the graph by collapsing each
//! community into a single super-node.  Repeats until modularity converges.
//!
//! Reference: Blondel, V.D. et al. (2008). "Fast Unfolding of Communities
//! in Large Networks." *Journal of Statistical Mechanics*, P10008.
use std::collections::{BTreeMap, BTreeSet};

use compact_str::CompactString;
use itertools::Itertools;
use tracing::debug;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::csr::{CsrBuilder, DirectedCsrGraph};
use crate::fixed_rule::error::GraphAlgorithmSnafu;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Louvain hierarchical community detection.
///
/// **Complexity:** O(P * I * V * log V) where P is hierarchy levels, I is
/// max iterations, V is vertices.  Typically converges in 2--5 levels for
/// real-world graphs.
///
/// **When to use:** Detecting multi-scale community structure in weighted
/// or unweighted graphs.  More stable than label propagation.
pub(crate) struct CommunityDetectionLouvain;

#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "graph Louvain community indices are bounds-checked by the CSR node count and community arrays"
)]
impl FixedRule for CommunityDetectionLouvain {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let undirected = payload.bool_option("undirected", Some(false))?;
        let max_iterations = payload.pos_integer_option("max_iter", Some(10))?;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "intentional f64 to f32 reduction"
        )]
        let delta = payload.unit_interval_option("delta", Some(0.0001))? as f32;
        let keep_depth = payload.non_neg_integer_option("keep_depth", None).ok(); // WHY: optional parameter; absence means no depth limit

        let (graph, indices, _inv_indices) = edges.as_directed_weighted_graph(undirected, false)?;
        let result = louvain(&graph, delta, max_iterations, poison)?;
        for (idx, node) in indices.into_iter().enumerate() {
            let mut labels = vec![];
            #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
            let mut current_idx = idx as u32;
            for hierarchy in &result {
                let next_idx = hierarchy[current_idx as usize];
                labels.push(DataValue::from(i64::from(next_idx)));
                current_idx = next_idx;
            }
            labels.reverse();
            if let Some(depth_limit) = keep_depth {
                labels.truncate(depth_limit);
            }
            out.put(vec![DataValue::List(labels), node]);
        }

        Ok(())
    }

    fn arity(
        &self,
        _options: &BTreeMap<CompactString, Expr>,
        _rule_head: &[Symbol],
        _span: SourceSpan,
    ) -> Result<usize> {
        Ok(2)
    }
}

/// Top-level Louvain loop: repeatedly run `louvain_step` until the graph
/// stops shrinking.
///
/// **Complexity:** O(P * I * V * log V) where P is hierarchy levels.
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Poison is lightweight and passed by value — cloned per iteration level"
)]
fn louvain(
    graph: &DirectedCsrGraph<f32>,
    delta: f32,
    max_iterations: usize,
    poison: Poison,
) -> Result<Vec<Vec<u32>>> {
    let mut current = graph;
    let mut collected = vec![];
    while current.node_count() > 2 {
        let (node_to_community, new_graph) =
            louvain_step(current, delta, max_iterations, poison.clone())?;
        debug!(
            "before size: {}, after size: {}",
            current.node_count(),
            new_graph.node_count()
        );
        if new_graph.node_count() == current.node_count() {
            break;
        }
        collected.push((node_to_community, new_graph));
        // SAFETY: we just pushed to `collected`, so `.last()` always succeeds.
        current = &collected
            .last()
            .ok_or_else(|| {
                GraphAlgorithmSnafu {
                    algorithm: "louvain",
                    message: "collected hierarchy is unexpectedly empty after push",
                }
                .build()
            })?
            .1;
    }
    Ok(collected
        .into_iter()
        .map(|(mapping, _)| mapping)
        .collect_vec())
}

/// Compute the modularity gain from moving `node` into `target_community`.
#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "graph Louvain modularity delta indices are bounds-checked by the community membership sets"
)]
#[expect(
    clippy::explicit_iter_loop,
    reason = "explicit .iter() on BTreeSet clarifies read-only traversal over community membership"
)]
fn calculate_modularity_delta(
    node: u32,
    target_community: u32,
    graph: &DirectedCsrGraph<f32>,
    community_members: &[BTreeSet<u32>],
    out_weights: &[f32],
    in_weights: &[f32],
    total_weight: f32,
) -> f32 {
    let mut sigma_out_total = 0.;
    let mut sigma_in_total = 0.;
    let mut edges_to_community = 0.;
    let members = &community_members[target_community as usize];
    for member in members.iter() {
        if *member == node {
            continue;
        }
        sigma_out_total += out_weights[*member as usize];
        sigma_in_total += in_weights[*member as usize];
        for target in graph.out_neighbors_with_values(node) {
            if target.target == *member {
                edges_to_community += target.value;
                break;
            }
        }
        for target in graph.out_neighbors_with_values(*member) {
            if target.target == node {
                edges_to_community += target.value;
                break;
            }
        }
    }
    edges_to_community
        - (sigma_out_total * in_weights[node as usize]
            + sigma_in_total * out_weights[node as usize])
            / total_weight
}

/// One Louvain iteration: phase 1 (node moves) + phase 2 (graph coarsening).
#[expect(
    clippy::too_many_lines,
    reason = "Louvain phase 1 (node moves) + phase 2 (graph coarsening) kept together for algorithmic clarity"
)]
#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "graph Louvain step indices are bounds-checked by the CSR node count and community arrays"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Poison is consumed by .check() and must be cloneable — owned value matches the pattern"
)]
#[expect(
    clippy::default_trait_access,
    reason = "Default::default() is idiomatic for type-inferred BTreeMap initialization"
)]
#[expect(
    clippy::redundant_else,
    reason = "explicit else branch clarifies the modularity convergence control flow"
)]
fn louvain_step(
    graph: &DirectedCsrGraph<f32>,
    delta: f32,
    max_iterations: usize,
    poison: Poison,
) -> Result<(Vec<u32>, DirectedCsrGraph<f32>)> {
    let node_count = graph.node_count();
    let mut total_weight = 0.;
    let mut out_weights = vec![0.; node_count as usize];
    let mut in_weights = vec![0.; node_count as usize];

    for from in 0..node_count {
        for target in graph.out_neighbors_with_values(from) {
            let to = target.target;
            let weight = target.value;

            total_weight += weight;
            out_weights[from as usize] += weight;
            in_weights[to as usize] += weight;
        }
    }

    let mut node_to_community = (0..node_count).collect_vec();
    let mut community_members = (0..node_count).map(|i| BTreeSet::from([i])).collect_vec();

    let mut last_modularity = f32::NEG_INFINITY;

    for _ in 0..max_iterations {
        let modularity = {
            let mut modularity = 0.;
            for from in 0..node_count {
                for to in &community_members[node_to_community[from as usize] as usize] {
                    for target in graph.out_neighbors_with_values(from) {
                        if target.target == *to {
                            modularity += target.value;
                        }
                    }
                    modularity -=
                        in_weights[from as usize] * out_weights[*to as usize] / total_weight;
                }
            }
            modularity /= total_weight;
            debug!("modularity {}", modularity);
            modularity
        };
        if modularity <= last_modularity + delta {
            break;
        } else {
            last_modularity = modularity;
        }

        let mut moved = false;
        for node in 0..node_count {
            let current_community = node_to_community[node as usize];

            let original_delta_q = calculate_modularity_delta(
                node,
                current_community,
                graph,
                &community_members,
                &out_weights,
                &in_weights,
                total_weight,
            );
            let mut best_community = current_community;
            let mut best_improvement = 0.;

            let mut considered_communities = BTreeSet::from([current_community]);
            for target in graph.out_neighbors_with_values(node) {
                let neighbor = target.target;

                let neighbor_community = node_to_community[neighbor as usize];
                if neighbor_community == current_community
                    || considered_communities.contains(&neighbor_community)
                {
                    continue;
                }
                considered_communities.insert(neighbor_community);

                let delta_q = calculate_modularity_delta(
                    node,
                    neighbor_community,
                    graph,
                    &community_members,
                    &out_weights,
                    &in_weights,
                    total_weight,
                );
                if delta_q - original_delta_q > best_improvement {
                    best_improvement = delta_q - original_delta_q;
                    best_community = neighbor_community;
                }
            }
            if best_improvement > 0. {
                moved = true;
                node_to_community[node as usize] = best_community;
                community_members[current_community as usize].remove(&node);
                community_members[best_community as usize].insert(node);
            }
            poison.check()?;
        }
        if !moved {
            break;
        }
    }
    let mut new_community_indices: BTreeMap<u32, u32> = Default::default();
    let mut new_community_count: u32 = 0;

    for community_id in &mut node_to_community {
        if let Some(new_id) = new_community_indices.get(community_id) {
            *community_id = *new_id;
        } else {
            new_community_indices.insert(*community_id, new_community_count);
            *community_id = new_community_count;
            new_community_count += 1;
        }
    }

    let mut coarsened_adjacency: Vec<BTreeMap<u32, f32>> =
        vec![BTreeMap::new(); new_community_count as usize];
    for (node, community) in node_to_community.iter().enumerate() {
        let target_map = &mut coarsened_adjacency[*community as usize];
        #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
        let node_u32 = node as u32;
        for target in graph.out_neighbors_with_values(node_u32) {
            let neighbor = target.target;
            let weight = target.value;
            let neighbor_community = node_to_community[neighbor as usize];
            *target_map.entry(neighbor_community).or_default() += weight;
        }
    }

    let coarsened_graph: DirectedCsrGraph<f32> = CsrBuilder::new()
        .sorted()
        .edges_with_values(coarsened_adjacency.into_iter().enumerate().flat_map(
            move |(from_community, neighbors)| {
                neighbors.into_iter().map(move |(to_community, weight)| {
                    #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
                    let from_u32 = from_community as u32;
                    (from_u32, to_community, weight)
                })
            },
        ))
        .build();

    Ok((node_to_community, coarsened_graph))
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::cast_possible_truncation,
    clippy::as_conversions,
    reason = "test: node indices fit in u32 for small fixed graphs"
)]
mod tests {
    use std::collections::BTreeSet;

    use crate::fixed_rule::csr::CsrBuilder;

    use crate::fixed_rule::algos::louvain::louvain;
    use crate::runtime::db::Poison;

    #[test]
    fn sample() {
        let adjacency: Vec<Vec<u32>> = vec![
            vec![2, 3, 5],           // 0
            vec![2, 4, 7],           // 1
            vec![0, 1, 4, 5, 6],     // 2
            vec![0, 7],              // 3
            vec![1, 2, 10],          // 4
            vec![0, 2, 7, 11],       // 5
            vec![2, 7, 11],          // 6
            vec![1, 3, 5, 6],        // 7
            vec![9, 10, 11, 12, 15], // 8
            vec![8, 12, 14],         // 9
            vec![4, 8, 12, 13, 14],  // 10
            vec![5, 6, 8, 13],       // 11
            vec![9, 10],             // 12
            vec![10, 11],            // 13
            vec![8, 9, 10],          // 14
            vec![8],                 // 15
        ];
        let graph = CsrBuilder::new()
            .sorted()
            .edges_with_values(adjacency.into_iter().enumerate().flat_map(
                |(from_node, targets)| {
                    targets.into_iter().map(move |to_node| {
                        let from_u32 = from_node as u32;
                        (from_u32, to_node, 1.)
                    })
                },
            ))
            .build();
        let hierarchy = louvain(&graph, 0., 100, Poison::default()).unwrap();

        // INVARIANT: Louvain must produce at least one partition for a non-empty graph.
        assert!(
            !hierarchy.is_empty(),
            "Louvain must return at least one hierarchy level"
        );

        // INVARIANT: the first level assigns every original node to a community.
        let first = hierarchy.first().unwrap();
        assert_eq!(
            first.len(),
            graph.node_count() as usize,
            "first level must contain one community assignment per node"
        );

        // INVARIANT: community ids are a dense range [0, num_communities).
        let mut communities: BTreeSet<u32> = first.iter().copied().collect();
        assert!(
            communities.len() > 1,
            "sample graph has two clear communities, so more than one is expected"
        );
        assert!(
            communities.len() < first.len() as usize,
            "some nodes must share a community"
        );
        assert_eq!(
            *communities.iter().max().unwrap(),
            communities.len() as u32 - 1,
            "community ids must be a dense 0..n range"
        );

        // INVARIANT: each coarsened level is smaller than the level before it and
        // its length equals the number of distinct communities in the prior level.
        let mut prev_len = first.len();
        for level in hierarchy.iter().skip(1) {
            assert!(
                level.len() < prev_len,
                "coarsened level must be strictly smaller than the previous level"
            );
            assert_eq!(
                level.len(),
                communities.len(),
                "coarsened level length must equal the number of communities above it"
            );
            communities = level.iter().copied().collect();
            assert!(
                communities.len() < prev_len,
                "coarsened level must have fewer distinct communities"
            );
            prev_len = level.len();
        }
    }
}
