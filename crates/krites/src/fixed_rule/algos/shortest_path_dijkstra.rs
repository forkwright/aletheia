//! Weighted single-source shortest paths via Dijkstra's algorithm.
//!
//! Runs one Dijkstra search per starting node over a non-negative-weighted
//! graph. When a termination relation is supplied, only those destinations
//! are reported; otherwise every node reachable in the edge relation is a
//! destination. `keep_ties` additionally reports every path tied for
//! minimum cost, not just one.
//!
//! Reference: Dijkstra, E.W. (1959). "A Note on Two Problems in Connexion
//! with Graphs." *Numerische Mathematik*, 1, 269--271.
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

/// Dijkstra shortest path, one run per starting node.
///
/// **Complexity:** O(S * E log V) — S independent Dijkstra runs.
///
/// **When to use:** Shortest weighted paths with non-negative weights. For
/// unweighted graphs prefer `ShortestPathBFS`.
pub(crate) struct ShortestPathDijkstra;

impl FixedRule for ShortestPathDijkstra {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;
        let starting = payload.get_input(1)?;
        let termination = payload.get_input(2);
        let undirected = payload.bool_option("undirected", Some(false))?;
        let keep_ties = payload.bool_option("keep_ties", Some(false))?;

        let (adjacency, all_nodes) = build_adjacency(edges, undirected, "ShortestPathDijkstra")?;

        let mut start_nodes: Vec<DataValue> = vec![];
        let mut seen_starts: BTreeSet<DataValue> = BTreeSet::new();
        for row in starting.iter()? {
            let node = row?.into_iter().next().unwrap_or(DataValue::Null);
            if seen_starts.insert(node.clone()) {
                start_nodes.push(node);
            }
        }

        let targets: Option<BTreeSet<DataValue>> = match termination {
            Err(_) => None,
            Ok(rel) => {
                let mut set = BTreeSet::new();
                for row in rel.iter()? {
                    set.insert(row?.into_iter().next().unwrap_or(DataValue::Null));
                }
                Some(set)
            }
        };

        for start in &start_nodes {
            if !all_nodes.contains(start) {
                continue;
            }
            let requested: Vec<DataValue> = match &targets {
                Some(set) => set
                    .iter()
                    .filter(|t| all_nodes.contains(*t))
                    .cloned()
                    .collect(),
                None => all_nodes.iter().cloned().collect(),
            };
            if requested.is_empty() {
                continue;
            }

            if keep_ties {
                let (distance, predecessors) = dijkstra_multi_predecessor(&adjacency, start);
                for target in requested {
                    let cost = distance.get(&target).copied().unwrap_or(f64::INFINITY);
                    if !cost.is_finite() {
                        out.put(vec![
                            start.clone(),
                            target,
                            DataValue::from(cost),
                            DataValue::List(vec![]),
                        ]);
                        continue;
                    }
                    for path in enumerate_paths(&predecessors, start, &target) {
                        out.put(vec![
                            start.clone(),
                            target.clone(),
                            DataValue::from(cost),
                            DataValue::List(path),
                        ]);
                    }
                    poison.check()?;
                }
            } else {
                let (distance, predecessor) = dijkstra_single_predecessor(&adjacency, start);
                for target in requested {
                    let cost = distance.get(&target).copied().unwrap_or(f64::INFINITY);
                    let path = if cost.is_finite() {
                        single_path(&predecessor, start, &target)
                    } else {
                        vec![]
                    };
                    out.put(vec![
                        start.clone(),
                        target,
                        DataValue::from(cost),
                        DataValue::List(path),
                    ]);
                    poison.check()?;
                }
            }
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
        Ok(4)
    }
}

/// Build a `DataValue`-keyed weighted adjacency list from an edge relation,
/// plus the set of every node mentioned by any edge. Rejects non-finite or
/// negative weights (Dijkstra requires non-negative edge costs).
#[expect(
    clippy::indexing_slicing,
    reason = "edge tuple has at least 2 elements by construction from the weighted edge scan"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
fn build_adjacency(
    edges: crate::fixed_rule::FixedRuleInputRelation<'_, '_>,
    undirected: bool,
    rule_name: &'static str,
) -> Result<(Adjacency, BTreeSet<DataValue>)> {
    let edges = edges.ensure_min_len(2)?;
    let mut adjacency: Adjacency = BTreeMap::new();
    let mut all_nodes: BTreeSet<DataValue> = BTreeSet::new();

    for row in edges.iter()? {
        let row = row?;
        let source = row[0].clone();
        let target = row[1].clone();
        let weight = match row.get(2) {
            None => 1.0,
            Some(w) => {
                let f = w.get_float().ok_or_else(|| {
                    InvalidInputSnafu {
                        rule: rule_name,
                        message: format!("edge weight {w:?} is not a number"),
                    }
                    .build()
                })?;
                if !f.is_finite() || f < 0.0 {
                    return Err(InvalidInputSnafu {
                        rule: rule_name,
                        message: format!("edge weight {w:?} must be finite and non-negative"),
                    }
                    .build()
                    .into());
                }
                f
            }
        };
        all_nodes.insert(source.clone());
        all_nodes.insert(target.clone());
        adjacency
            .entry(source.clone())
            .or_default()
            .push((target.clone(), weight));
        if undirected {
            adjacency.entry(target).or_default().push((source, weight));
        } else {
            adjacency.entry(target).or_default();
        }
    }
    Ok((adjacency, all_nodes))
}

/// Single-source Dijkstra keeping one predecessor per node.
fn dijkstra_single_predecessor(
    adjacency: &Adjacency,
    start: &DataValue,
) -> (BTreeMap<DataValue, f64>, BTreeMap<DataValue, DataValue>) {
    let mut distance: BTreeMap<DataValue, f64> = BTreeMap::from([(start.clone(), 0.0)]);
    let mut predecessor: BTreeMap<DataValue, DataValue> = BTreeMap::new();
    let mut queue: BinaryHeap<Reverse<(OrderedFloat<f64>, DataValue)>> = BinaryHeap::new();
    queue.push(Reverse((OrderedFloat(0.0), start.clone())));

    while let Some(Reverse((OrderedFloat(cost), node))) = queue.pop() {
        if distance.get(&node).is_some_and(|&best| cost > best) {
            continue;
        }
        for (neighbor, weight) in adjacency.get(&node).into_iter().flatten() {
            let candidate = cost + weight;
            let better = distance
                .get(neighbor)
                .is_none_or(|&existing| candidate < existing);
            if better {
                distance.insert(neighbor.clone(), candidate);
                predecessor.insert(neighbor.clone(), node.clone());
                queue.push(Reverse((OrderedFloat(candidate), neighbor.clone())));
            }
        }
    }
    (distance, predecessor)
}

/// Single-source Dijkstra keeping every tied-minimum predecessor per node.
#[expect(
    clippy::float_cmp,
    reason = "tie detection compares costs computed via the same accumulation path — exact equality is intended"
)]
fn dijkstra_multi_predecessor(
    adjacency: &Adjacency,
    start: &DataValue,
) -> (
    BTreeMap<DataValue, f64>,
    BTreeMap<DataValue, Vec<DataValue>>,
) {
    let mut distance: BTreeMap<DataValue, f64> = BTreeMap::from([(start.clone(), 0.0)]);
    let mut predecessors: BTreeMap<DataValue, Vec<DataValue>> = BTreeMap::new();
    let mut queue: BinaryHeap<Reverse<(OrderedFloat<f64>, DataValue)>> = BinaryHeap::new();
    queue.push(Reverse((OrderedFloat(0.0), start.clone())));

    while let Some(Reverse((OrderedFloat(cost), node))) = queue.pop() {
        if distance.get(&node).is_some_and(|&best| cost > best) {
            continue;
        }
        for (neighbor, weight) in adjacency.get(&node).into_iter().flatten() {
            let candidate = cost + weight;
            match distance.get(neighbor).copied() {
                Some(existing) if candidate < existing => {
                    distance.insert(neighbor.clone(), candidate);
                    predecessors.insert(neighbor.clone(), vec![node.clone()]);
                    queue.push(Reverse((OrderedFloat(candidate), neighbor.clone())));
                }
                Some(existing) if candidate == existing => {
                    predecessors
                        .entry(neighbor.clone())
                        .or_default()
                        .push(node.clone());
                }
                None => {
                    distance.insert(neighbor.clone(), candidate);
                    predecessors.insert(neighbor.clone(), vec![node.clone()]);
                    queue.push(Reverse((OrderedFloat(candidate), neighbor.clone())));
                }
                _ => {}
            }
        }
    }
    (distance, predecessors)
}

fn single_path(
    predecessor: &BTreeMap<DataValue, DataValue>,
    start: &DataValue,
    target: &DataValue,
) -> Vec<DataValue> {
    let mut path = vec![target.clone()];
    let mut cursor = target.clone();
    while cursor != *start {
        let Some(prev) = predecessor.get(&cursor) else {
            break;
        };
        path.push(prev.clone());
        cursor = prev.clone();
    }
    path.reverse();
    path
}

/// Enumerate every tied shortest path from `start` to `target` by walking
/// the multi-predecessor map backwards. May be exponential in the number
/// of tied paths, matching the documented worst case of this variant.
fn enumerate_paths(
    predecessors: &BTreeMap<DataValue, Vec<DataValue>>,
    start: &DataValue,
    target: &DataValue,
) -> Vec<Vec<DataValue>> {
    if target == start {
        return vec![vec![start.clone()]];
    }
    let Some(preds) = predecessors.get(target) else {
        return vec![];
    };
    let mut results = vec![];
    for pred in preds {
        for mut prefix in enumerate_paths(predecessors, start, pred) {
            prefix.push(target.clone());
            results.push(prefix);
        }
    }
    results
}
