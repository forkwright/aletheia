//! Tarjan's strongly connected components.
#![expect(
    unused_imports,
    reason = "algorithm may use additional imports depending on feature flags"
)]
#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use std::cmp::min;
use std::collections::BTreeMap;

use compact_str::CompactString;
use itertools::Itertools;

use crate::engine::data::expr::Expr;
use crate::engine::data::program::{MagicFixedRuleApply, MagicSymbol};
use crate::engine::data::symb::Symbol;
use crate::engine::data::tuple::Tuple;
use crate::engine::data::value::DataValue;
use crate::engine::error::InternalResult as Result;
use crate::engine::fixed_rule::csr::DirectedCsrGraph;
use crate::engine::fixed_rule::{FixedRule, FixedRulePayload};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::db::Poison;
use crate::engine::runtime::temp_store::{EpochStore, RegularTempStore};
use crate::engine::runtime::transact::SessionTx;

#[cfg(feature = "graph-algo")]
pub(crate) struct StronglyConnectedComponent {
    strong: bool,
}
#[cfg(feature = "graph-algo")]
impl StronglyConnectedComponent {
    pub(crate) fn new(strong: bool) -> Self {
        Self { strong }
    }
}

#[cfg(feature = "graph-algo")]
impl FixedRule for StronglyConnectedComponent {
    #[expect(
        clippy::expect_used,
        reason = "indices bounded by graph size; tuples guaranteed non-empty"
    )]
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;

        let (graph, indices, mut inv_indices) = edges.as_directed_graph(!self.strong)?;

        let tarjan = TarjanSccG::new(graph).run(poison)?;
        for (grp_id, cc) in tarjan.iter().enumerate() {
            for idx in cc {
                let val = indices
                    .get(*idx as usize)
                    .expect("idx within graph index bounds");
                let tuple = vec![val.clone(), DataValue::from(grp_id as i64)];
                out.put(tuple);
            }
        }

        let mut counter = tarjan.len() as i64;

        if let Ok(nodes) = payload.get_input(1) {
            for tuple in nodes.iter()? {
                let tuple = tuple?;
                let node = tuple.into_iter().next().expect("tuple is non-empty");
                if !inv_indices.contains_key(&node) {
                    inv_indices.insert(node.clone(), u32::MAX);
                    let tuple = vec![node, DataValue::from(counter)];
                    out.put(tuple);
                    counter += 1;
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
        Ok(2)
    }
}

pub(crate) struct TarjanSccG {
    graph: DirectedCsrGraph,
    id: u32,
    ids: Vec<Option<u32>>,
    low: Vec<u32>,
    on_stack: Vec<bool>,
    stack: Vec<u32>,
}

impl TarjanSccG {
    pub(crate) fn new(graph: DirectedCsrGraph) -> Self {
        let graph_size = graph.node_count();
        Self {
            graph,
            id: 0,
            ids: vec![None; graph_size as usize],
            low: vec![0; graph_size as usize],
            on_stack: vec![false; graph_size as usize],
            stack: vec![],
        }
    }
    pub(crate) fn run(mut self, poison: Poison) -> Result<Vec<Vec<u32>>> {
        for i in 0..self.graph.node_count() {
            if self.ids[i as usize].is_none() {
                self.dfs(i);
                poison.check()?;
            }
        }

        let mut low_map: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (idx, grp) in self.low.into_iter().enumerate() {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "graph node count bounded by u32"
            )]
            let idx_u32 = idx as u32;
            low_map.entry(grp).or_default().push(idx_u32);
        }

        Ok(low_map.into_values().collect_vec())
    }
    #[expect(
        clippy::expect_used,
        reason = "ids[at] set on entry before recursive use"
    )]
    fn dfs(&mut self, at: u32) {
        self.stack.push(at);
        self.on_stack[at as usize] = true;
        self.id += 1;
        self.ids[at as usize] = Some(self.id);
        self.low[at as usize] = self.id;
        for to in self.graph.out_neighbors(at).collect_vec() {
            if self.ids[to as usize].is_none() {
                self.dfs(to);
            }
            if self.on_stack[to as usize] {
                self.low[at as usize] = min(self.low[at as usize], self.low[to as usize]);
            }
        }
        if self.ids[at as usize].expect("id set during dfs traversal") == self.low[at as usize] {
            while let Some(node) = self.stack.pop() {
                self.on_stack[node as usize] = false;
                self.low[node as usize] =
                    self.ids[at as usize].expect("id set during dfs traversal");
                if node == at {
                    break;
                }
            }
        }
    }
}
