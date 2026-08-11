//! Minimum spanning tree via Prim's algorithm.
//!
//! Grows a tree outward from a starting node, at each step adding the
//! cheapest edge that crosses the visited/unvisited boundary. Uses a
//! lazy-deletion binary heap: stale (superseded) heap entries are simply
//! skipped when popped rather than removed in place.
//!
//! Reference: Prim, R.C. (1957). "Shortest Connection Networks and Some
//! Generalizations." *Bell System Technical Journal*, 36(6), 1389--1401.
#![expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeMap/BTreeSet key"
)]
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use compact_str::CompactString;
use ordered_float::OrderedFloat;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::error::InvalidInputSnafu;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Node-to-weighted-neighbor adjacency, keyed by `DataValue`.
type Adjacency = BTreeMap<DataValue, Vec<(DataValue, f64)>>;
/// Min-heap frontier entry: (edge weight, edge source, edge target).
type Frontier = BinaryHeap<Reverse<(OrderedFloat<f64>, DataValue, DataValue)>>;

/// Push every not-yet-visited neighbor of `node` onto `frontier`.
fn admit_neighbors(
    node: &DataValue,
    adjacency: &Adjacency,
    visited: &BTreeSet<DataValue>,
    frontier: &mut Frontier,
) {
    for (neighbor, weight) in adjacency.get(node).into_iter().flatten() {
        if !visited.contains(neighbor) {
            frontier.push(Reverse((
                OrderedFloat(*weight),
                node.clone(),
                neighbor.clone(),
            )));
        }
    }
}

/// Prim's minimum spanning tree (undirected, weighted).
///
/// **Complexity:** O(E log V) with a binary heap.
///
/// **When to use:** Minimum-cost spanning tree from a specific starting
/// node; competitive with Kruskal on dense graphs.
pub(crate) struct MinimumSpanningTreePrim;

#[expect(
    clippy::indexing_slicing,
    reason = "edge tuple has at least 2 elements by construction from the weighted edge scan"
)]
impl FixedRule for MinimumSpanningTreePrim {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?.ensure_min_len(2)?;

        let mut adjacency: Adjacency = BTreeMap::new();
        for row in edges.iter()? {
            let row = row?;
            let source = row[0].clone();
            let target = row[1].clone();
            let weight = match row.get(2) {
                None => 1.0,
                Some(w) => {
                    let f = w.get_float().ok_or_else(|| {
                        InvalidInputSnafu {
                            rule: "MinimumSpanningTreePrim",
                            message: format!("edge weight {w:?} is not a number"),
                        }
                        .build()
                    })?;
                    if !f.is_finite() {
                        return Err(InvalidInputSnafu {
                            rule: "MinimumSpanningTreePrim",
                            message: format!("edge weight {w:?} must be finite"),
                        }
                        .build()
                        .into());
                    }
                    f
                }
            };
            adjacency
                .entry(source.clone())
                .or_default()
                .push((target.clone(), weight));
            adjacency.entry(target).or_default().push((source, weight));
            poison.check()?;
        }

        if adjacency.is_empty() {
            return Ok(());
        }

        let start = match payload.get_input(1) {
            Err(_) => adjacency.keys().next().cloned().unwrap_or(DataValue::Null),
            Ok(rel) => {
                let tuple = rel.iter()?.next().ok_or_else(|| {
                    InvalidInputSnafu {
                        rule: "MinimumSpanningTreePrim",
                        message: "the provided starting-node relation is empty".to_string(),
                    }
                    .build()
                })??;
                let node = tuple.into_iter().next().unwrap_or(DataValue::Null);
                if !adjacency.contains_key(&node) {
                    return Err(InvalidInputSnafu {
                        rule: "MinimumSpanningTreePrim",
                        message: format!("requested starting node {node:?} is not in the graph"),
                    }
                    .build()
                    .into());
                }
                node
            }
        };

        let mut visited: BTreeSet<DataValue> = BTreeSet::new();
        let mut frontier: Frontier = BinaryHeap::new();

        visited.insert(start.clone());
        admit_neighbors(&start, &adjacency, &visited, &mut frontier);

        while let Some(Reverse((OrderedFloat(cost), from, to))) = frontier.pop() {
            if visited.contains(&to) {
                continue;
            }
            visited.insert(to.clone());
            out.put(vec![from, to.clone(), DataValue::from(cost)]);
            admit_neighbors(&to, &adjacency, &visited, &mut frontier);
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
        Ok(3)
    }
}
