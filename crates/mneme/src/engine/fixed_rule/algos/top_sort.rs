//! Topological sort.
#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::engine::data::expr::Expr;
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::DataValue;
use crate::engine::error::InternalResult as Result;
use crate::engine::fixed_rule::csr::DirectedCsrGraph;
use crate::engine::fixed_rule::{FixedRule, FixedRulePayload};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::db::Poison;
use crate::engine::runtime::temp_store::RegularTempStore;

pub(crate) struct TopSort;

impl FixedRule for TopSort {
    #[expect(
        clippy::expect_used,
        reason = "val_id produced by graph traversal, always in bounds"
    )]
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;

        let (graph, indices, _) = edges.as_directed_graph(false)?;

        let sorted = kahn_g(&graph, poison)?;

        for (idx, val_id) in sorted.iter().enumerate() {
            let val = indices
                .get(*val_id as usize)
                .expect("val_id within graph index bounds");
            let tuple = vec![DataValue::from(idx as i64), val.clone()];
            out.put(tuple);
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

pub(crate) fn kahn_g(graph: &DirectedCsrGraph, poison: Poison) -> Result<Vec<u32>> {
    let graph_size = graph.node_count();
    let mut in_degree = vec![0; graph_size as usize];
    for tos in 0..graph_size {
        for to in graph.out_neighbors(tos) {
            in_degree[to as usize] += 1;
        }
    }
    let mut sorted = Vec::with_capacity(graph_size as usize);
    let mut pending = vec![];

    for (node, degree) in in_degree.iter().enumerate() {
        if *degree == 0 {
            pending.push(node as u32);
        }
    }

    while let Some(removed) = pending.pop() {
        sorted.push(removed);
        for nxt in graph.out_neighbors(removed) {
            in_degree[nxt as usize] -= 1;
            if in_degree[nxt as usize] == 0 {
                pending.push(nxt);
            }
        }
        poison.check()?;
    }

    Ok(sorted)
}
