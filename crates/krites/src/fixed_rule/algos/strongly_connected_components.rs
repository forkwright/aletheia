//! Strongly connected components via Tarjan's algorithm.
//!
//! Finds all strongly connected components (SCCs) of a directed graph in a
//! single DFS pass.  Also supports weakly connected components when the
//! graph is treated as undirected.
//!
//! Reference: Tarjan, R.E. (1972). "Depth-First Search and Linear Graph
//! Algorithms." *SIAM Journal on Computing*, 1(2), 146--160.

use std::cmp::min;
use std::collections::BTreeMap;

use compact_str::CompactString;
use itertools::Itertools;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::csr::DirectedCsrGraph;
use crate::fixed_rule::error::GraphAlgorithmSnafu;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Strongly (or weakly) connected components.
///
/// When `strong` is true, finds SCCs using Tarjan's algorithm on the
/// directed graph.  When `strong` is false, finds weakly connected
/// components by treating the graph as undirected.
///
/// **Complexity:** O(V + E) -- single DFS traversal.
///
/// **When to use:** Identifying cycles and mutual reachability in directed
/// graphs, or partitioning undirected graphs into connected components.
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
#[expect(
    clippy::as_conversions,
    reason = "graph SCC group indices are small values cast between u32/i64 — guarded by graph size"
)]
impl FixedRule for StronglyConnectedComponent {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;

        let (graph, indices, mut inv_indices) = edges.as_directed_graph(!self.strong)?;

        let tarjan = TarjanScc::new(graph).run(poison)?;
        for (group_id, component) in tarjan.iter().enumerate() {
            for idx in component {
                let node_value = indices.get(*idx as usize).ok_or_else(|| {
                    GraphAlgorithmSnafu {
                        algorithm: "strongly_connected_components",
                        message: format!(
                            "graph traversal produced index {idx} beyond vertex array bounds"
                        ),
                    }
                    .build()
                })?;
                #[expect(clippy::cast_possible_wrap, reason = "value fits i64")]
                let tuple = vec![node_value.clone(), DataValue::from(group_id as i64)];
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

/// Tarjan's SCC algorithm state.
///
/// Maintains DFS discovery ids, low-link values, and the explicit stack
/// needed for SCC identification.
///
/// **Note:** The `dfs()` method uses recursion.  For extremely deep graphs
/// this may hit the system stack limit.  Production use on graphs with
/// depth > ~10K should consider an iterative variant.
pub(crate) struct TarjanScc {
    graph: DirectedCsrGraph,
    next_id: u32,
    discovery_ids: Vec<Option<u32>>,
    low_links: Vec<u32>,
    on_stack: Vec<bool>,
    stack: Vec<u32>,
}

#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "Tarjan SCC DFS indices are bounds-checked by the CSR node count and ids/low/on_stack arrays"
)]
impl TarjanScc {
    pub(crate) fn new(graph: DirectedCsrGraph) -> Self {
        let graph_size = graph.node_count();
        Self {
            graph,
            next_id: 0,
            discovery_ids: vec![None; graph_size as usize],
            low_links: vec![0; graph_size as usize],
            on_stack: vec![false; graph_size as usize],
            stack: vec![],
        }
    }

    /// Execute Tarjan's SCC algorithm.
    ///
    /// **Complexity:** O(V + E) -- linear time DFS-based algorithm.
    #[expect(
        clippy::result_large_err,
        reason = "InternalError carries structured context — boxing deferred to avoid API churn"
    )]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "Poison is lightweight and passed by value for ergonomic .check() calls"
    )]
    pub(crate) fn run(mut self, poison: Poison) -> Result<Vec<Vec<u32>>> {
        for node in 0..self.graph.node_count() {
            if self.discovery_ids[node as usize].is_none() {
                self.dfs(node);
                poison.check()?;
            }
        }

        let mut low_link_map: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (idx, group) in self.low_links.into_iter().enumerate() {
            #[expect(clippy::cast_possible_truncation, reason = "value fits u32")]
            let idx_u32 = idx as u32;
            low_link_map.entry(group).or_default().push(idx_u32);
        }

        Ok(low_link_map.into_values().collect_vec())
    }

    /// Recursive DFS for SCC discovery.
    ///
    /// **Complexity:** O(1) per node plus O(E) total edge traversals across
    /// all DFS calls.
    fn dfs(&mut self, at: u32) {
        self.stack.push(at);
        self.on_stack[at as usize] = true;
        self.next_id += 1;
        self.discovery_ids[at as usize] = Some(self.next_id);
        self.low_links[at as usize] = self.next_id;
        for neighbor in self.graph.out_neighbors(at).collect_vec() {
            if self.discovery_ids[neighbor as usize].is_none() {
                self.dfs(neighbor);
            }
            if self.on_stack[neighbor as usize] {
                self.low_links[at as usize] = min(
                    self.low_links[at as usize],
                    self.low_links[neighbor as usize],
                );
            }
        }
        if self.discovery_ids[at as usize].unwrap_or(0) == self.low_links[at as usize] {
            while let Some(node) = self.stack.pop() {
                self.on_stack[node as usize] = false;
                self.low_links[node as usize] = self.discovery_ids[at as usize].unwrap_or(0);
                if node == at {
                    break;
                }
            }
        }
    }
}
