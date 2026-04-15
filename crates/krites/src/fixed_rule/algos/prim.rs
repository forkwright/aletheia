//! Minimum spanning tree via Prim's algorithm.
//!
//! Grows a spanning tree from a starting node by repeatedly adding the
//! cheapest edge that connects a visited node to an unvisited node.  Uses a
//! binary-heap priority queue for efficient edge selection.
//!
//! Reference: Prim, R.C. (1957). "Shortest Connection Networks and Some
//! Generalizations." *Bell System Technical Journal*, 36(6), 1389--1401.
use std::cmp::Reverse;
use std::collections::BTreeMap;

use compact_str::CompactString;
use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::csr::DirectedCsrGraph;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Prim's minimum spanning tree.
///
/// **Complexity:** O(E log V) using a binary heap where E is edges and V
/// is vertices.
///
/// **When to use:** Finding a minimum-cost spanning tree from a specific
/// starting node.  Preferred over Kruskal for dense graphs.
pub(crate) struct MinimumSpanningTreePrim;

#[expect(
    clippy::as_conversions,
    clippy::cast_lossless,
    clippy::indexing_slicing,
    reason = "graph Prim MST indices are bounds-checked by the CSR adjacency structure and visited set"
)]
impl FixedRule for MinimumSpanningTreePrim {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let (graph, indices, inv_indices) = edges.as_directed_weighted_graph(true, true)?;
        if graph.node_count() == 0 {
            return Ok(());
        }
        let starting_node = match payload.get_input(1) {
            Err(_) => 0,
            Ok(rel) => {
                let tuple = rel.iter()?.next().ok_or_else(|| {
                    crate::fixed_rule::error::InvalidInputSnafu {
                        rule: "MinimumSpanningTreePrim",
                        message: "The provided starting nodes relation is empty".to_string(),
                    }
                    .build()
                })??;
                let node_value = &tuple[0];
                *inv_indices.get(node_value).ok_or_else(|| {
                    crate::fixed_rule::error::InvalidInputSnafu {
                        rule: "MinimumSpanningTreePrim",
                        message: format!("The requested starting node {node_value:?} is not found"),
                    }
                    .build()
                })?
            }
        };
        let mst_edges = prim(&graph, starting_node, poison)?;
        for (source, destination, cost) in mst_edges {
            out.put(vec![
                indices[source as usize].clone(),
                indices[destination as usize].clone(),
                DataValue::from(cost as f64),
            ]);
        }
        Ok(())
    }

    fn arity(
        &self,
        _options: &BTreeMap<CompactString, Expr>,
        _rule_head: &[Symbol],
        _span: SourceSpan,
    ) -> Result<usize> {
        Ok(3)
    }
}

/// Core Prim MST construction using a binary heap.
///
/// **Complexity:** O(E log V) where E is edges and V is vertices.  Each
/// edge may be pushed to the heap once, and each vertex is extracted once.
#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "Prim MST indices are bounds-checked by the visited array and CSR node count"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Poison is lightweight and passed by value for ergonomic .check() calls"
)]
fn prim(
    graph: &DirectedCsrGraph<f32>,
    starting_node: u32,
    poison: Poison,
) -> Result<Vec<(u32, u32, f32)>> {
    let mut visited = vec![false; graph.node_count() as usize];
    let mut mst_edges = Vec::with_capacity((graph.node_count() - 1) as usize);
    let mut priority_queue = PriorityQueue::new();

    let mut relax_edges_at_node = |node: u32, pq: &mut PriorityQueue<_, _>| {
        visited[node as usize] = true;
        for target in graph.out_neighbors_with_values(node) {
            let neighbor = target.target;
            let cost = target.value;
            if visited[neighbor as usize] {
                continue;
            }
            pq.push_increase(neighbor, (Reverse(OrderedFloat(cost)), node));
        }
    };

    relax_edges_at_node(starting_node, &mut priority_queue);

    while let Some((to_node, (Reverse(OrderedFloat(cost)), from_node))) = priority_queue.pop() {
        if mst_edges.len() == (graph.node_count() - 1) as usize {
            break;
        }
        mst_edges.push((from_node, to_node, cost));
        relax_edges_at_node(to_node, &mut priority_queue);
        poison.check()?;
    }

    Ok(mst_edges)
}
