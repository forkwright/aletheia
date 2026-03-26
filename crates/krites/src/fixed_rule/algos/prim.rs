//! Minimum spanning tree (Prim).
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

pub(crate) struct MinimumSpanningTreePrim;

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
        let starting = match payload.get_input(1) {
            Err(_) => 0,
            Ok(rel) => {
                let tuple = rel.iter()?.next().ok_or_else(|| {
                    crate::fixed_rule::error::InvalidInputSnafu {
                        rule: "MinimumSpanningTreePrim",
                        message: "The provided starting nodes relation is empty".to_string(),
                    }
                    .build()
                })??;
                #[expect(clippy::indexing_slicing, reason = "index bounds validated")]
                let dv = &tuple[0];
                *inv_indices.get(dv).ok_or_else(|| {
                    crate::fixed_rule::error::InvalidInputSnafu {
                        rule: "MinimumSpanningTreePrim",
                        message: format!("The requested starting node {dv:?} is not found"),
                    }
                    .build()
                })?
            }
        };
        let msp = prim(&graph, starting, poison)?;
        for (src, dst, cost) in msp {
            out.put(vec![
                indices[src as usize].clone(),
                indices[dst as usize].clone(),
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

fn prim(
    graph: &DirectedCsrGraph<f32>,
    starting: u32,
    poison: Poison,
) -> Result<Vec<(u32, u32, f32)>> {
    #[expect(clippy::cast_sign_loss, reason = "graph node u32 fits usize")]
    let mut visited = vec![false; graph.node_count() as usize];
    #[expect(clippy::cast_sign_loss, reason = "graph node u32 fits usize")]
    let mut mst_edges = Vec::with_capacity((graph.node_count() - 1) as usize);
    let mut pq = PriorityQueue::new();

    let mut relax_edges_at_node = |node: u32, pq: &mut PriorityQueue<_, _>| {
        visited[node as usize] = true;
        for target in graph.out_neighbors_with_values(node) {
            let to_node = target.target;
            let cost = target.value;
            if visited[to_node as usize] {
                continue;
            }
            pq.push_increase(to_node, (Reverse(OrderedFloat(cost)), node));
        }
    };

    relax_edges_at_node(starting, &mut pq);

    while let Some((to_node, (Reverse(OrderedFloat(cost)), from_node))) = pq.pop() {
        if mst_edges.len() == (graph.node_count() - 1) as usize {
            break;
        }
        mst_edges.push((from_node, to_node, cost));
        relax_edges_at_node(to_node, &mut pq);
        poison.check()?;
    }

    Ok(mst_edges)
}
