//! Yen's k-shortest loopless paths algorithm.
//!
//! Finds the k shortest simple (loopless) paths between a source and target
//! node.  Works by iteratively finding spur paths from each node on the
//! previous shortest path, with appropriate edge and node exclusions to
//! so new paths are discovered.
//!
//! Reference: Yen, J.Y. (1971). "Finding the K Shortest Loopless Paths in
//! a Network." *Management Science*, 17(11), 712--716.
use std::collections::{BTreeMap, BTreeSet};

use compact_str::CompactString;
use itertools::Itertools;
use rayon::prelude::*;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::algos::shortest_path_dijkstra::dijkstra;
use crate::fixed_rule::csr::DirectedCsrGraph;
use crate::fixed_rule::error::GraphAlgorithmSnafu;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Yen's k-shortest loopless paths.
///
/// **Complexity:** O(K * S * T * (E log V)) where K is paths, S is sources,
/// T is targets.  Each candidate path requires a Dijkstra run with edge
/// exclusions.
///
/// **When to use:** Finding alternative routes in transportation or
/// communication networks, or when path diversity is needed beyond the
/// single shortest path.
pub(crate) struct KShortestPathYen;

#[expect(
    clippy::as_conversions,
    clippy::cast_lossless,
    clippy::indexing_slicing,
    reason = "graph Yen's k-shortest path indices are bounds-checked by the CSR adjacency structure"
)]
#[expect(
    clippy::type_complexity,
    reason = "Yen's parallel results contain source, target, and path list together"
)]
#[expect(
    clippy::semicolon_if_nothing_returned,
    reason = "trailing semicolons in output blocks kept for consistency with surrounding style"
)]
impl FixedRule for KShortestPathYen {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let starting = payload.get_input(1)?;
        let termination = payload.get_input(2)?;
        let undirected = payload.bool_option("undirected", Some(false))?;
        let path_count = payload.pos_integer_option("k", None)?;

        let (graph, indices, inv_indices) = edges.as_directed_weighted_graph(undirected, false)?;

        let mut starting_nodes = BTreeSet::new();
        for tuple in starting.iter()? {
            let tuple = tuple?;
            let node = &tuple[0];
            if let Some(idx) = inv_indices.get(node) {
                starting_nodes.insert(*idx);
            }
        }
        let mut termination_nodes = BTreeSet::new();
        for tuple in termination.iter()? {
            let tuple = tuple?;
            let node = &tuple[0];
            if let Some(idx) = inv_indices.get(node) {
                termination_nodes.insert(*idx);
            }
        }
        if starting_nodes.len() <= 1 && termination_nodes.len() <= 1 {
            for start in starting_nodes {
                for goal in &termination_nodes {
                    for (cost, path) in
                        k_shortest_path_yen(path_count, &graph, start, *goal, poison.clone())?
                    {
                        let tuple = vec![
                            indices[start as usize].clone(),
                            indices[*goal as usize].clone(),
                            DataValue::from(cost as f64),
                            DataValue::List(
                                path.into_iter()
                                    .map(|u| indices[u as usize].clone())
                                    .collect_vec(),
                            ),
                        ];
                        out.put(tuple)
                    }
                }
            }
        } else {
            let pair_iter = starting_nodes
                .iter()
                .flat_map(|start| termination_nodes.iter().map(|goal| (*start, *goal)));
            let parallel_iter = pair_iter.par_bridge();

            #[expect(
                clippy::result_large_err,
                reason = "InternalError carries structured context — boxing deferred to avoid API churn"
            )]
            let all_results: Vec<_> = parallel_iter
                .map(
                    |(start, goal)| -> Result<(u32, u32, Vec<(f32, Vec<u32>)>)> {
                        Ok((
                            start,
                            goal,
                            k_shortest_path_yen(path_count, &graph, start, goal, poison.clone())?,
                        ))
                    },
                )
                .collect::<Result<_>>()?;

            for (start, goal, paths) in all_results {
                for (cost, path) in paths {
                    let tuple = vec![
                        indices[start as usize].clone(),
                        indices[goal as usize].clone(),
                        DataValue::from(cost as f64),
                        DataValue::List(
                            path.into_iter()
                                .map(|u| indices[u as usize].clone())
                                .collect_vec(),
                        ),
                    ];
                    out.put(tuple)
                }
            }
        }
        Ok(())
    }

    fn arity(
        &self,
        _options: &BTreeMap<CompactString, Expr>,
        _rule_head: &[Symbol],
        _span: SourceSpan,
    ) -> Result<usize> {
        Ok(4)
    }
}

/// Core Yen's k-shortest loopless paths computation.
///
/// **Complexity:** O(K * N * (E log V)) where K is requested paths, N is
/// path length, E is edges, V is vertices.  For each of K paths, explores
/// up to N spur nodes with Dijkstra.
#[expect(
    clippy::indexing_slicing,
    reason = "Yen's algorithm indices are bounds-checked by path length and candidate list"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Poison is lightweight and passed by value for ergonomic .check() calls"
)]
#[expect(
    clippy::range_plus_one,
    reason = "0..i + 1 matches the standard Yen's algorithm notation for root path prefix"
)]
fn k_shortest_path_yen(
    k: usize,
    edges: &DirectedCsrGraph<f32>,
    start: u32,
    goal: u32,
    poison: Poison,
) -> Result<Vec<(f32, Vec<u32>)>> {
    let mut k_shortest: Vec<(f32, Vec<u32>)> = Vec::with_capacity(k);
    let mut candidates: Vec<(f32, Vec<u32>)> = vec![];

    match dijkstra(edges, start, &Some(goal), &(), &())
        .into_iter()
        .next()
    {
        None => return Ok(k_shortest),
        Some((_, cost, path)) => k_shortest.push((cost, path)),
    }

    for _ in 1..k {
        // SAFETY: `k_shortest` has at least one element (pushed before the loop).
        let (_, previous_path) = k_shortest.last().ok_or_else(|| {
            GraphAlgorithmSnafu {
                algorithm: "yen",
                message: "k_shortest is unexpectedly empty inside loop",
            }
            .build()
        })?;
        for i in 0..previous_path.len() - 1 {
            let spur_node = match previous_path.get(i) {
                None => return Ok(vec![]),
                Some(node) => *node,
            };
            let root_path = &previous_path[0..i + 1];
            let mut forbidden_edges = BTreeSet::new();
            for (_, existing_path) in &k_shortest {
                if existing_path.len() < root_path.len() + 1 {
                    continue;
                }
                let path_prefix = &existing_path[0..i + 1];
                if path_prefix == root_path {
                    forbidden_edges.insert((existing_path[i], existing_path[i + 1]));
                }
            }
            let mut forbidden_nodes = BTreeSet::new();
            for node in &previous_path[0..i] {
                forbidden_nodes.insert(*node);
            }
            if let Some((_, spur_cost, spur_path)) = dijkstra(
                edges,
                spur_node,
                &Some(goal),
                &forbidden_edges,
                &forbidden_nodes,
            )
            .into_iter()
            .next()
            {
                let mut total_cost = spur_cost;
                for i in 0..root_path.len() - 1 {
                    let source = root_path[i];
                    let destination = root_path[i + 1];
                    for target in edges.out_neighbors_with_values(source) {
                        let edge_target = target.target;
                        let edge_cost = target.value;
                        if edge_target == destination {
                            total_cost += edge_cost;
                            break;
                        }
                    }
                }
                let mut total_path = root_path.to_vec();
                total_path.pop();
                total_path.extend(spur_path);
                if candidates
                    .iter()
                    .all(|(_, existing)| *existing != total_path)
                {
                    candidates.push((total_cost, total_path));
                }
                poison.check()?;
            }
        }
        if candidates.is_empty() {
            break;
        }
        candidates.sort_by(|(cost_a, _), (cost_b, _)| cost_b.total_cmp(cost_a));
        // SAFETY: `candidates.is_empty()` was checked (and would break) above.
        let shortest_candidate = candidates.pop().ok_or_else(|| {
            GraphAlgorithmSnafu {
                algorithm: "yen",
                message: "candidates list is unexpectedly empty after non-empty check",
            }
            .build()
        })?;
        let shortest_cost = shortest_candidate.0;
        if shortest_cost.is_finite() {
            k_shortest.push(shortest_candidate);
        }
    }
    Ok(k_shortest)
}
