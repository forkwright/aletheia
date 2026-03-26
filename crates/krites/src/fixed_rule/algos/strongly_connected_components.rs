//! Tarjan's strongly connected components.
#![expect(
    unused_imports,
    reason = "algorithm may use additional imports depending on feature flags"
)]

use std::cmp::min;
use std::collections::BTreeMap;

use compact_str::CompactString;
use itertools::Itertools;

use crate::data::expr::Expr;
use crate::data::program::{MagicFixedRuleApply, MagicSymbol};
use crate::data::symb::Symbol;
use crate::data::tuple::Tuple;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::csr::DirectedCsrGraph;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::{EpochStore, RegularTempStore};
use crate::runtime::transact::SessionTx;

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
                let val = indices.get(*idx as usize).unwrap_or_else(|| unreachable!());
                #[expect(clippy::cast_possible_wrap, reason = "value fits i64")]
                let tuple = vec![val.clone(), DataValue::from(grp_id as i64)];
                out.put(tuple);
            }
        }

        #[expect(clippy::cast_possible_wrap, reason = "value fits i64")]
        let mut counter = tarjan.len() as i64;

        if let Ok(nodes) = payload.get_input(1) {
            for tuple in nodes.iter()? {
                let tuple = tuple?;
                let node = tuple.into_iter().next().unwrap_or(DataValue::Null);
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
            #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
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
        if self.ids[at as usize].unwrap_or(0) == self.low[at as usize] {
            while let Some(node) = self.stack.pop() {
                self.on_stack[node as usize] = false;
                self.low[node as usize] = self.ids[at as usize].unwrap_or(0);
                if node == at {
                    break;
                }
            }
        }
    }
}
