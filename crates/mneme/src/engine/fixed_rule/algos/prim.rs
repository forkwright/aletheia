/*
 * Copyright 2022, The Cozo Project Authors.
 *
 * This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.
 * If a copy of the MPL was not distributed with this file,
 * You can obtain one at https://mozilla.org/MPL/2.0/.
 */

use crate::engine::error::DbResult as Result;
use graph::prelude::{DirectedCsrGraph, DirectedNeighborsWithValues, Graph};
use std::cmp::Reverse;
use std::collections::BTreeMap;

use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;
use smartstring::{LazyCompact, SmartString};

use crate::engine::data::expr::Expr;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::DataValue;
use crate::engine::fixed_rule::{FixedRule, FixedRulePayload};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::db::Poison;
use crate::engine::runtime::temp_store::RegularTempStore;

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
                    crate::engine::error::AdhocError(
                        "The provided starting nodes relation is empty".to_string(),
                    )
                })??;
                let dv = &tuple[0];
                *inv_indices.get(dv).ok_or_else(|| {
                    crate::engine::error::AdhocError(format!(
                        "The requested starting node {dv:?} is not found"
                    ))
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
        _options: &BTreeMap<SmartString<LazyCompact>, Expr>,
        _rule_head: &[Symbol],
        _span: SourceSpan,
    ) -> Result<usize> {
        Ok(3)
    }
}

fn prim(
    graph: &DirectedCsrGraph<u32, (), f32>,
    starting: u32,
    poison: Poison,
) -> Result<Vec<(u32, u32, f32)>> {
    let mut visited = vec![false; graph.node_count() as usize];
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
