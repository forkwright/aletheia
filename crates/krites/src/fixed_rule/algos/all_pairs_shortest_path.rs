//! Betweenness and closeness centrality via all-pairs shortest paths.
//!
//! **Betweenness centrality** measures how often a node lies on shortest paths
//! between other nodes.  Uses a Brandes-style accumulation of dependency
//! scores on top of Dijkstra's algorithm.
//!
//! Reference: Brandes, U. (2001). "A Faster Algorithm for Betweenness
//! Centrality." *Journal of Mathematical Sociology*, 25(2), 163--177.
//!
//! **Closeness centrality** is the reciprocal of the mean shortest-path
//! distance from a node to all reachable nodes, normalised by the square of
//! the reachable-node count.
//!
//! Reference: Freeman, L.C. (1978). "Centrality in Social Networks:
//! Conceptual Clarification." *Social Networks*, 1(3), 215--239.
use std::cmp::Reverse;
use std::collections::BTreeMap;

use compact_str::CompactString;
use itertools::Itertools;
use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;
use rayon::prelude::*;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::algos::shortest_path_dijkstra::dijkstra_keep_ties;
use crate::fixed_rule::csr::DirectedCsrGraph;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Betweenness centrality via Brandes' algorithm.
///
/// Computes for each node the fraction of all shortest paths between other
/// node pairs that pass through it.  Higher values indicate bridge or
/// bottleneck nodes.
///
/// **Complexity:** O(V * (E log V)) — runs Dijkstra from every node and
/// accumulates dependency scores.  Parallelised across starting nodes.
///
/// **When to use:** Identifying bottleneck or bridge nodes in weighted
/// directed graphs.
pub(crate) struct BetweennessCentrality;

#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "graph BFS/Dijkstra indices are bounds-checked by the CSR adjacency structure"
)]
#[expect(
    clippy::default_trait_access,
    reason = "Default::default() is idiomatic for type-inferred HashMap initialization"
)]
#[expect(
    clippy::explicit_into_iter_loop,
    reason = "explicit .into_iter() clarifies ownership transfer of collected results"
)]
impl FixedRule for BetweennessCentrality {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let undirected = payload.bool_option("undirected", Some(false))?;

        let (graph, indices, _inv_indices) = edges.as_directed_weighted_graph(undirected, false)?;

        let node_count = graph.node_count();
        if node_count == 0 {
            return Ok(());
        }

        let iterator = (0..node_count).into_par_iter();

        #[expect(
            clippy::result_large_err,
            reason = "InternalError carries structured context — boxing deferred to avoid API churn"
        )]
        let centrality_segments: Vec<_> = iterator
            .map(|start| -> Result<BTreeMap<u32, f32>> {
                let paths_for_start =
                    dijkstra_keep_ties(&graph, start, &(), &(), &(), poison.clone())?;
                let mut accumulator: BTreeMap<u32, f32> = Default::default();
                let grouped = paths_for_start
                    .into_iter()
                    .chunk_by(|(target, _, _)| *target);
                for (_, group) in grouped.into_iter() {
                    let group = group.collect_vec();
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "path group count acceptable as approximate float"
                    )]
                    let group_size = group.len() as f32;
                    for (_, _, path) in group {
                        if path.len() < 3 {
                            continue;
                        }
                        for middle in path.iter().take(path.len() - 1).skip(1) {
                            let entry = accumulator.entry(*middle).or_default();
                            *entry += 1. / group_size;
                        }
                    }
                }
                Ok(accumulator)
            })
            .collect::<Result<_>>()?;
        let mut centrality: Vec<f32> = vec![0.; node_count as usize];
        for segment in centrality_segments {
            for (node_id, score) in segment {
                centrality[node_id as usize] += score;
            }
        }

        for (idx, score) in centrality.into_iter().enumerate() {
            let node = indices[idx].clone();
            out.put(vec![node, f64::from(score).into()]);
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

/// Closeness centrality (Wasserman-Faust normalisation).
///
/// For each node, computes the ratio of reachable-node-count squared to the
/// product of total distance and (N-1).  Handles disconnected graphs by only
/// summing over finite distances.
///
/// **Complexity:** O(V * (E log V)) — runs Dijkstra from every node.
/// Parallelised across starting nodes.
///
/// **When to use:** Ranking nodes by how quickly information spreads from
/// them through the network.
pub(crate) struct ClosenessCentrality;

#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "graph Dijkstra indices are bounds-checked by the CSR adjacency structure"
)]
#[expect(
    clippy::cloned_instead_of_copied,
    reason = "OrderedFloat is Copy but clippy suggests .copied() inconsistently — .cloned() is clearer"
)]
impl FixedRule for ClosenessCentrality {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let undirected = payload.bool_option("undirected", Some(false))?;

        let (graph, indices, _inv_indices) = edges.as_directed_weighted_graph(undirected, false)?;

        let node_count = graph.node_count();
        if node_count == 0 {
            return Ok(());
        }
        let iterator = (0..node_count).into_par_iter();

        #[expect(
            clippy::result_large_err,
            reason = "InternalError carries structured context — boxing deferred to avoid API churn"
        )]
        let results: Vec<_> = iterator
            .map(|start| -> Result<f32> {
                let distances = dijkstra_cost_only(&graph, start, poison.clone())?;
                let total_distance: f32 = distances.iter().filter(|d| d.is_finite()).cloned().sum();
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "reachable node count acceptable as approximate float"
                )]
                let reachable_count: f32 =
                    distances.iter().filter(|d| d.is_finite()).count() as f32;
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "node count minus one acceptable as approximate float"
                )]
                let denominator = (node_count - 1) as f32;
                Ok(reachable_count * reachable_count / total_distance / denominator)
            })
            .collect::<Result<_>>()?;
        for (idx, centrality) in results.into_iter().enumerate() {
            out.put(vec![
                indices[idx].clone(),
                DataValue::from(f64::from(centrality)),
            ]);
            poison.check()?;
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

/// Single-source shortest-path distances via Dijkstra (cost-only, no paths).
///
/// Returns a distance vector where `distances[node]` is the shortest-path
/// cost from `start` to `node`, or `f32::INFINITY` if unreachable.
///
/// Reference: Dijkstra, E.W. (1959). "A Note on Two Problems in Connexion
/// with Graphs." *Numerische Mathematik*, 1, 269--271.
///
/// **Complexity:** O(E log V) using a binary-heap priority queue.
/// **Space:** O(V) for the distance array.
#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "graph Dijkstra indices are bounds-checked by the visited set and CSR adjacency structure"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Poison is lightweight and passed by value for ergonomic .check() calls"
)]
pub(crate) fn dijkstra_cost_only(
    edges: &DirectedCsrGraph<f32>,
    start: u32,
    poison: Poison,
) -> Result<Vec<f32>> {
    let mut distance = vec![f32::INFINITY; edges.node_count() as usize];
    let mut priority_queue = PriorityQueue::new();
    distance[start as usize] = 0.;
    priority_queue.push(start, Reverse(OrderedFloat(0.)));

    while let Some((node, Reverse(OrderedFloat(cost)))) = priority_queue.pop() {
        if cost > distance[node as usize] {
            continue;
        }

        for target in edges.out_neighbors_with_values(node) {
            let neighbor = target.target;
            let edge_weight = target.value;

            let new_cost = cost + edge_weight;
            if new_cost < distance[neighbor as usize] {
                priority_queue.push_increase(neighbor, Reverse(OrderedFloat(new_cost)));
                distance[neighbor as usize] = new_cost;
            }
        }
        poison.check()?;
    }

    Ok(distance)
}
