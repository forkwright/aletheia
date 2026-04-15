//! Weighted shortest path via Dijkstra's algorithm.
//!
//! Finds shortest paths from one or more source nodes to optional target
//! nodes in a weighted directed graph with non-negative edge weights.
//! Supports both single-path and tie-keeping (all equal-cost paths) modes.
//! Parallelised across starting nodes when count > 1.
//!
//! Reference: Dijkstra, E.W. (1959). "A Note on Two Problems in Connexion
//! with Graphs." *Numerische Mathematik*, 1, 269--271.
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::iter;

use compact_str::CompactString;
use itertools::Itertools;
use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;
use rayon::prelude::*;
use smallvec::{SmallVec, smallvec};

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::csr::DirectedCsrGraph;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Dijkstra shortest path with optional parallelisation and tie-keeping.
///
/// **Complexity:** O(S * (E log V)) where S is starting nodes, E is edges,
/// V is vertices.  Parallelised across starting nodes when count > 1.
///
/// **When to use:** Finding shortest weighted paths in graphs with
/// non-negative edge weights.  For unweighted graphs, prefer
/// `ShortestPathBFS`.
pub(crate) struct ShortestPathDijkstra;

#[expect(
    clippy::too_many_lines,
    reason = "Dijkstra setup, parallel dispatch, and path reconstruction kept together for algorithmic clarity"
)]
#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "graph Dijkstra indices are bounds-checked by the CSR adjacency structure and visited set"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
#[expect(
    clippy::type_complexity,
    reason = "Dijkstra returns parallel results with paths, costs, and node indices together"
)]
#[expect(
    clippy::semicolon_if_nothing_returned,
    reason = "trailing semicolons in match arms kept for consistency with surrounding style"
)]
impl FixedRule for ShortestPathDijkstra {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let starting = payload.get_input(1)?;
        let termination = payload.get_input(2);
        let undirected = payload.bool_option("undirected", Some(false))?;
        let keep_ties = payload.bool_option("keep_ties", Some(false))?;

        let (graph, indices, inv_indices) = edges.as_directed_weighted_graph(undirected, false)?;

        let mut starting_nodes = BTreeSet::new();
        for tuple in starting.iter()? {
            let tuple = tuple?;
            let node = &tuple[0];
            if let Some(idx) = inv_indices.get(node) {
                starting_nodes.insert(*idx);
            }
        }
        let termination_nodes = match termination {
            Err(_) => None,
            Ok(termination_rel) => {
                let mut targets = BTreeSet::new();
                for tuple in termination_rel.iter()? {
                    let tuple = tuple?;
                    let node = &tuple[0];
                    if let Some(idx) = inv_indices.get(node) {
                        targets.insert(*idx);
                    }
                }
                Some(targets)
            }
        };

        if starting_nodes.len() <= 1 {
            for start in starting_nodes {
                let results = if let Some(targets) = &termination_nodes {
                    if targets.len() == 1 {
                        let single_target = targets.iter().next().copied();
                        if keep_ties {
                            dijkstra_keep_ties(
                                &graph,
                                start,
                                &single_target,
                                &(),
                                &(),
                                poison.clone(),
                            )?
                        } else {
                            dijkstra(&graph, start, &single_target, &(), &())
                        }
                    } else if keep_ties {
                        dijkstra_keep_ties(&graph, start, targets, &(), &(), poison.clone())?
                    } else {
                        dijkstra(&graph, start, targets, &(), &())
                    }
                } else {
                    dijkstra(&graph, start, &(), &(), &())
                };
                for (target, cost, path) in results {
                    let tuple = vec![
                        indices[start as usize].clone(),
                        indices[target as usize].clone(),
                        DataValue::from(f64::from(cost)),
                        DataValue::List(
                            path.into_iter()
                                .map(|u| indices[u as usize].clone())
                                .collect_vec(),
                        ),
                    ];
                    out.put(tuple)
                }
            }
        } else {
            let parallel_iter = starting_nodes.into_par_iter();

            let all_results: Vec<_> = parallel_iter
                .map(|start| -> Result<(u32, Vec<(u32, f32, Vec<u32>)>)> {
                    Ok((
                        start,
                        if let Some(targets) = &termination_nodes {
                            if targets.len() == 1 {
                                let single_target = targets.iter().next().copied();
                                if keep_ties {
                                    dijkstra_keep_ties(
                                        &graph,
                                        start,
                                        &single_target,
                                        &(),
                                        &(),
                                        poison.clone(),
                                    )?
                                } else {
                                    dijkstra(&graph, start, &single_target, &(), &())
                                }
                            } else if keep_ties {
                                dijkstra_keep_ties(
                                    &graph,
                                    start,
                                    targets,
                                    &(),
                                    &(),
                                    poison.clone(),
                                )?
                            } else {
                                dijkstra(&graph, start, targets, &(), &())
                            }
                        } else {
                            dijkstra(&graph, start, &(), &(), &())
                        },
                    ))
                })
                .collect::<Result<_>>()?;
            for (start, results) in all_results {
                for (target, cost, path) in results {
                    let tuple = vec![
                        indices[start as usize].clone(),
                        indices[target as usize].clone(),
                        DataValue::from(f64::from(cost)),
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

/// Trait for edges that should be excluded from path search.
pub(crate) trait ForbiddenEdge {
    fn is_forbidden(&self, source: u32, destination: u32) -> bool;
}

impl ForbiddenEdge for () {
    fn is_forbidden(&self, _source: u32, _destination: u32) -> bool {
        false
    }
}

impl ForbiddenEdge for BTreeSet<(u32, u32)> {
    fn is_forbidden(&self, source: u32, destination: u32) -> bool {
        self.contains(&(source, destination))
    }
}

/// Trait for nodes that should be excluded from path search.
pub(crate) trait ForbiddenNode {
    fn is_forbidden(&self, node: u32) -> bool;
}

impl ForbiddenNode for () {
    fn is_forbidden(&self, _node: u32) -> bool {
        false
    }
}

impl ForbiddenNode for BTreeSet<u32> {
    fn is_forbidden(&self, node: u32) -> bool {
        self.contains(&node)
    }
}

/// Trait for tracking goal completion in shortest-path search.
pub(crate) trait Goal {
    fn is_exhausted(&self) -> bool;
    fn visit(&mut self, node: u32);
    fn iter(&self, total_nodes: u32) -> Box<dyn Iterator<Item = u32> + '_>;
}

impl Goal for () {
    fn is_exhausted(&self) -> bool {
        false
    }

    fn visit(&mut self, _node: u32) {}

    fn iter(&self, total_nodes: u32) -> Box<dyn Iterator<Item = u32> + '_> {
        Box::new(0..total_nodes)
    }
}

impl Goal for Option<u32> {
    fn is_exhausted(&self) -> bool {
        self.is_none()
    }

    fn visit(&mut self, node: u32) {
        if let Some(target) = &self
            && *target == node
        {
            self.take();
        }
    }

    fn iter(&self, _total_nodes: u32) -> Box<dyn Iterator<Item = u32> + '_> {
        if let Some(target) = self {
            Box::new(iter::once(*target))
        } else {
            Box::new(iter::empty())
        }
    }
}

impl Goal for BTreeSet<u32> {
    fn is_exhausted(&self) -> bool {
        self.is_empty()
    }

    fn visit(&mut self, node: u32) {
        self.remove(&node);
    }

    fn iter(&self, _total_nodes: u32) -> Box<dyn Iterator<Item = u32> + '_> {
        Box::new(self.iter().copied())
    }
}

/// Standard Dijkstra shortest path with optional edge/node exclusions and goal
/// tracking.
///
/// **Complexity:** O(E log V) using a binary-heap priority queue.
/// **Space:** O(V) for distances and backpointers.
#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "graph Dijkstra indices are bounds-checked by the CSR node count and distance arrays"
)]
#[expect(
    clippy::if_not_else,
    reason = "!cost.is_finite() is the short-circuit case (no path) — checking it first reads better"
)]
pub(crate) fn dijkstra<FE: ForbiddenEdge, FN: ForbiddenNode, G: Goal + Clone>(
    edges: &DirectedCsrGraph<f32>,
    start: u32,
    goals: &G,
    forbidden_edges: &FE,
    forbidden_nodes: &FN,
) -> Vec<(u32, f32, Vec<u32>)> {
    let graph_size = edges.node_count();
    let mut distance = vec![f32::INFINITY; graph_size as usize];
    let mut priority_queue = PriorityQueue::new();
    let mut back_pointers = vec![u32::MAX; graph_size as usize];
    distance[start as usize] = 0.;
    priority_queue.push(start, Reverse(OrderedFloat(0.)));
    let mut goals_remaining = goals.clone();

    while let Some((node, Reverse(OrderedFloat(cost)))) = priority_queue.pop() {
        if cost > distance[node as usize] {
            continue;
        }

        for target in edges.out_neighbors_with_values(node) {
            let neighbor = target.target;
            let edge_weight = target.value;

            if forbidden_nodes.is_forbidden(neighbor) {
                continue;
            }
            if forbidden_edges.is_forbidden(node, neighbor) {
                continue;
            }
            let new_cost = cost + edge_weight;
            if new_cost < distance[neighbor as usize] {
                priority_queue.push_increase(neighbor, Reverse(OrderedFloat(new_cost)));
                distance[neighbor as usize] = new_cost;
                back_pointers[neighbor as usize] = node;
            }
        }

        goals_remaining.visit(node);
        if goals_remaining.is_exhausted() {
            break;
        }
    }

    goals
        .iter(edges.node_count())
        .map(|target| {
            let cost = distance[target as usize];
            if !cost.is_finite() {
                (target, cost, vec![])
            } else {
                let mut path = vec![];
                let mut current = target;
                while current != start {
                    path.push(current);
                    current = back_pointers[current as usize];
                }
                path.push(start);
                path.reverse();
                (target, cost, path)
            }
        })
        .collect_vec()
}

/// Dijkstra variant that keeps all tied (equal-cost) shortest paths.
///
/// **Complexity:** O(E log V + P) where P is the number of paths found.
/// Can be exponential in worst case when many equal-cost paths exist.
#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "graph Dijkstra indices are bounds-checked by the CSR node count and distance arrays"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
#[expect(
    clippy::float_cmp,
    reason = "Dijkstra equal-cost tie detection compares distances set from same arithmetic path — exact equality is correct"
)]
#[expect(
    clippy::semicolon_if_nothing_returned,
    reason = "trailing semicolons in if-blocks kept for consistency with surrounding style"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Poison is lightweight and passed by value for ergonomic .check() calls"
)]
#[expect(
    clippy::if_not_else,
    reason = "!cost.is_finite() is the short-circuit case (no path) — checking it first reads better"
)]
pub(crate) fn dijkstra_keep_ties<FE: ForbiddenEdge, FN: ForbiddenNode, G: Goal + Clone>(
    edges: &DirectedCsrGraph<f32>,
    start: u32,
    goals: &G,
    forbidden_edges: &FE,
    forbidden_nodes: &FN,
    poison: Poison,
) -> Result<Vec<(u32, f32, Vec<u32>)>> {
    let mut distance = vec![f32::INFINITY; edges.node_count() as usize];
    let mut priority_queue = PriorityQueue::new();
    let mut back_pointers: Vec<SmallVec<[u32; 1]>> =
        vec![smallvec![]; edges.node_count() as usize];
    distance[start as usize] = 0.;
    priority_queue.push(start, Reverse(OrderedFloat(0.)));
    let mut goals_remaining = goals.clone();

    while let Some((node, Reverse(OrderedFloat(cost)))) = priority_queue.pop() {
        if cost > distance[node as usize] {
            continue;
        }

        for target in edges.out_neighbors_with_values(node) {
            let neighbor = target.target;
            let edge_weight = target.value;

            if forbidden_nodes.is_forbidden(neighbor) {
                continue;
            }
            if forbidden_edges.is_forbidden(node, neighbor) {
                continue;
            }
            let new_cost = cost + edge_weight;
            if new_cost < distance[neighbor as usize] {
                priority_queue.push_increase(neighbor, Reverse(OrderedFloat(new_cost)));
                distance[neighbor as usize] = new_cost;
                back_pointers[neighbor as usize].clear();
                back_pointers[neighbor as usize].push(node);
            } else if new_cost == distance[neighbor as usize] {
                priority_queue.push_increase(neighbor, Reverse(OrderedFloat(new_cost)));
                back_pointers[neighbor as usize].push(node);
            }
            poison.check()?;
        }

        goals_remaining.visit(node);
        if goals_remaining.is_exhausted() {
            break;
        }
    }

    let results = goals
        .iter(edges.node_count())
        .flat_map(|target| {
            let cost = distance[target as usize];
            if !cost.is_finite() {
                vec![(target, cost, vec![])]
            } else {
                struct PathCollector {
                    collected: Vec<(u32, f32, Vec<u32>)>,
                }

                impl PathCollector {
                    fn collect(
                        &mut self,
                        chain: &[u32],
                        start: u32,
                        target: u32,
                        cost: f32,
                        back_pointers: &[SmallVec<[u32; 1]>],
                    ) {
                        // SAFETY: `collect` is always called with non-empty `chain`
                        // (initial call passes `&[target]`).
                        let last = chain[chain.len() - 1];
                        let predecessors = &back_pointers[last as usize];
                        for &predecessor in predecessors {
                            let mut extended = chain.to_vec();
                            extended.push(predecessor);
                            if predecessor == start {
                                extended.reverse();
                                self.collected.push((target, cost, extended));
                            } else {
                                self.collect(
                                    &extended,
                                    start,
                                    target,
                                    cost,
                                    back_pointers,
                                )
                            }
                        }
                    }
                }
                let mut collector = PathCollector { collected: vec![] };
                collector.collect(&[target], start, target, cost, &back_pointers);
                collector.collected
            }
        })
        .collect_vec();

    Ok(results)
}
