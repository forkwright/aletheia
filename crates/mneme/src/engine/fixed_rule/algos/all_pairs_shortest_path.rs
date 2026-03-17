//! All-pairs shortest path (Floyd-Warshall).
use crate::engine::error::InternalResult as Result;
use crate::engine::fixed_rule::csr::DirectedCsrGraph;
use std::cmp::Reverse;
use std::collections::BTreeMap;

use compact_str::CompactString;
use itertools::Itertools;
use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;
use rayon::prelude::*;

use crate::engine::data::expr::Expr;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::DataValue;
use crate::engine::fixed_rule::algos::shortest_path_dijkstra::dijkstra_keep_ties;
use crate::engine::fixed_rule::{FixedRule, FixedRulePayload};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::db::Poison;
use crate::engine::runtime::temp_store::RegularTempStore;

pub(crate) struct BetweennessCentrality;

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

        let n = graph.node_count();
        if n == 0 {
            return Ok(());
        }

        let it = (0..n).into_par_iter();

        let centrality_segs: Vec<_> = it
            .map(|start| -> Result<BTreeMap<u32, f32>> {
                let res_for_start =
                    dijkstra_keep_ties(&graph, start, &(), &(), &(), poison.clone())?;
                let mut ret: BTreeMap<u32, f32> = Default::default();
                let grouped = res_for_start.into_iter().chunk_by(|(n, _, _)| *n);
                for (_, grp) in grouped.into_iter() {
                    let grp = grp.collect_vec();
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "path group count acceptable as approximate float"
                    )]
                    let l = grp.len() as f32;
                    for (_, _, path) in grp {
                        if path.len() < 3 {
                            continue;
                        }
                        for middle in path.iter().take(path.len() - 1).skip(1) {
                            let entry = ret.entry(*middle).or_default();
                            *entry += 1. / l;
                        }
                    }
                }
                Ok(ret)
            })
            .collect::<Result<_>>()?;
        let mut centrality: Vec<f32> = vec![0.; n as usize];
        for m in centrality_segs {
            for (k, v) in m {
                centrality[k as usize] += v;
            }
        }

        for (i, s) in centrality.into_iter().enumerate() {
            let node = indices[i].clone();
            out.put(vec![node, f64::from(s).into()]);
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

pub(crate) struct ClosenessCentrality;

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

        let n = graph.node_count();
        if n == 0 {
            return Ok(());
        }
        let it = (0..n).into_par_iter();

        let res: Vec<_> = it
            .map(|start| -> Result<f32> {
                let distances = dijkstra_cost_only(&graph, start, poison.clone())?;
                let total_dist: f32 = distances.iter().filter(|d| d.is_finite()).cloned().sum();
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "reachable node count acceptable as approximate float"
                )]
                let nc: f32 = distances.iter().filter(|d| d.is_finite()).count() as f32;
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "node count minus one acceptable as approximate float"
                )]
                let denom = (n - 1) as f32;
                Ok(nc * nc / total_dist / denom)
            })
            .collect::<Result<_>>()?;
        for (idx, centrality) in res.into_iter().enumerate() {
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

pub(crate) fn dijkstra_cost_only(
    edges: &DirectedCsrGraph<f32>,
    start: u32,
    poison: Poison,
) -> Result<Vec<f32>> {
    let mut distance = vec![f32::INFINITY; edges.node_count() as usize];
    let mut pq = PriorityQueue::new();
    let mut back_pointers = vec![u32::MAX; edges.node_count() as usize];
    distance[start as usize] = 0.;
    pq.push(start, Reverse(OrderedFloat(0.)));

    while let Some((node, Reverse(OrderedFloat(cost)))) = pq.pop() {
        if cost > distance[node as usize] {
            continue;
        }

        for target in edges.out_neighbors_with_values(node) {
            let nxt_node = target.target;
            let path_weight = target.value;

            let nxt_cost = cost + path_weight;
            if nxt_cost < distance[nxt_node as usize] {
                pq.push_increase(nxt_node, Reverse(OrderedFloat(nxt_cost)));
                distance[nxt_node as usize] = nxt_cost;
                back_pointers[nxt_node as usize] = node;
            }
        }
        poison.check()?;
    }

    Ok(distance)
}
