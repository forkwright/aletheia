//! Topological sort.
use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::csr::DirectedCsrGraph;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

pub(crate) struct TopSort;

#[expect(
    clippy::as_conversions,
    reason = "graph topological sort indices are small values cast between u32/i64 — guarded by graph size"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "graph topological sort indices are bounds-checked by the CSR node count"
)]
impl FixedRule for TopSort {
    /// Run topological sort (Kahn's algorithm).
    ///
    /// # Complexity
    ///
    /// O(V + E) where V is vertices and E is edges. Processes each node and edge once.
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
            // INVARIANT: `val_id` comes from Kahn's algorithm over the graph, indices are valid
            let val = indices
                .get(*val_id as usize)
                .unwrap_or_else(|| panic!("topological sort index must be valid"));
            #[expect(clippy::cast_possible_wrap, reason = "value fits i64")]
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

/// Kahn's algorithm for topological sorting.
///
/// # Complexity
///
/// O(V + E) where V is vertices and E is edges.
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    clippy::indexing_slicing,
    reason = "Kahn's algorithm indices are bounds-checked by the CSR node count and in-degree arrays"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Poison is lightweight and passed by value for ergonomic .check() calls"
)]
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
