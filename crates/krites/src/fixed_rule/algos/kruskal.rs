//! Minimum spanning forest via Kruskal's algorithm.
//!
//! Builds a minimum spanning forest by greedily adding the cheapest edge
//! that does not form a cycle.  Uses a union-find (disjoint-set) data
//! structure with path compression and union by size for near-constant-time
//! connectivity queries.
//!
//! Reference: Kruskal, J.B. (1956). "On the Shortest Spanning Subtree of a
//! Graph and the Traveling Salesman Problem." *Proceedings of the American
//! Mathematical Society*, 7(1), 48--50.
use std::cmp::Reverse;
use std::collections::BTreeMap;

use compact_str::CompactString;
use itertools::Itertools;
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

/// Kruskal's minimum spanning forest.
///
/// **Complexity:** O(E log E) for sorting edges, plus O(E * alpha(V)) for
/// union-find operations where alpha is the inverse Ackermann function
/// (effectively constant).
///
/// **When to use:** Finding a minimum-cost spanning tree or forest.
/// Preferred over Prim for sparse graphs.
pub(crate) struct MinimumSpanningForestKruskal;

#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "graph Kruskal indices are bounds-checked by the CSR node count"
)]
impl FixedRule for MinimumSpanningForestKruskal {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let (graph, indices, _) = edges.as_directed_weighted_graph(true, true)?;
        if graph.node_count() == 0 {
            return Ok(());
        }
        let mst_edges = kruskal(&graph, poison)?;
        for (source, destination, cost) in mst_edges {
            out.put(vec![
                indices[source as usize].clone(),
                indices[destination as usize].clone(),
                DataValue::from(f64::from(cost)),
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

/// Core Kruskal MST construction using union-find with path compression.
///
/// **Complexity:** O(E log E) dominated by edge sorting via priority queue.
/// Union-find operations are O(alpha(V)), effectively constant.
#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "Kruskal union-find indices are bounds-checked by the node count and parent arrays"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Poison is lightweight and passed by value for ergonomic .check() calls"
)]
fn kruskal(edges: &DirectedCsrGraph<f32>, poison: Poison) -> Result<Vec<(u32, u32, f32)>> {
    let mut priority_queue = PriorityQueue::new();
    let mut union_find = UnionFind::new(edges.node_count());
    let mut mst = Vec::with_capacity((edges.node_count() - 1) as usize);
    for from in 0..edges.node_count() {
        for target in edges.out_neighbors_with_values(from) {
            let to = target.target;
            let cost = target.value;
            priority_queue.push((from, to), Reverse(OrderedFloat(cost)));
        }
    }
    while let Some(((from, to), Reverse(OrderedFloat(cost)))) = priority_queue.pop() {
        if union_find.connected(from, to) {
            continue;
        }
        union_find.union(from, to);

        mst.push((from, to, cost));
        if union_find.sizes[0] == edges.node_count() {
            break;
        }
        poison.check()?;
    }
    Ok(mst)
}

/// Disjoint-set (union-find) with path compression and union by size.
///
/// **Complexity per operation:** O(alpha(V)) amortised where alpha is the
/// inverse Ackermann function.
struct UnionFind {
    parents: Vec<u32>,
    sizes: Vec<u32>,
}

#[expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "union-find parent/size arrays are indexed by node id which is always < node_count"
)]
impl UnionFind {
    fn new(node_count: u32) -> Self {
        Self {
            parents: (0..node_count).collect_vec(),
            sizes: vec![1; node_count as usize],
        }
    }
    fn union(&mut self, p: u32, q: u32) {
        let root_p = self.find(p);
        let root_q = self.find(q);
        if root_p != root_q {
            if self.sizes[root_p as usize] < self.sizes[root_q as usize] {
                self.sizes[root_q as usize] += self.sizes[root_p as usize];
                self.parents[root_p as usize] = root_q;
            } else {
                self.sizes[root_p as usize] += self.sizes[root_q as usize];
                self.parents[root_q as usize] = root_p;
            }
        }
    }
    fn find(&mut self, mut node: u32) -> u32 {
        let mut root = node;
        while root != self.parents[root as usize] {
            root = self.parents[root as usize];
        }
        // Path compression
        while node != root {
            let next = self.parents[node as usize];
            self.parents[node as usize] = root;
            node = next;
        }
        root
    }
    fn connected(&mut self, p: u32, q: u32) -> bool {
        self.find(p) == self.find(q)
    }
}
