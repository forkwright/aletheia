//! Betweenness and closeness centrality, both built on single-source
//! shortest-path trees over the full node set.
//!
//! **Betweenness centrality** (Brandes' algorithm) measures how often a
//! node sits on a shortest path between two *other* nodes. For each
//! source, a weighted Dijkstra pass records shortest-path counts (`sigma`)
//! and predecessor sets, then a second pass over nodes in decreasing
//! distance order accumulates each node's "dependency" back onto its
//! predecessors.
//!
//! Reference: Brandes, U. (2001). "A Faster Algorithm for Betweenness
//! Centrality." *Journal of Mathematical Sociology*, 25(2), 163--177.
//!
//! **Closeness centrality** (Wasserman-Faust normalisation) is the squared
//! count of reachable nodes divided by the product of total distance and
//! (N-1), so partially-reachable graphs are still comparable.
//!
//! Reference: Freeman, L.C. (1978). "Centrality in Social Networks:
//! Conceptual Clarification." *Social Networks*, 1(3), 215--239.
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

/// Betweenness centrality via Brandes' algorithm.
///
/// **Complexity:** O(V * E log V) — one Dijkstra pass per node.
///
/// **When to use:** Identifying bridge/bottleneck nodes in a weighted
/// directed (or undirected) graph.
pub(crate) struct BetweennessCentrality;

impl FixedRule for BetweennessCentrality {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?.ensure_min_len(2)?;
        let undirected = payload.bool_option("undirected", Some(false))?;
        let (adjacency, all_nodes) = build_adjacency(edges, undirected, "BetweennessCentrality")?;

        let mut betweenness: BTreeMap<DataValue, f64> =
            all_nodes.iter().map(|node| (node.clone(), 0.0)).collect();

        for source in &all_nodes {
            accumulate_brandes(&adjacency, source, &mut betweenness);
            poison.check()?;
        }

        for (node, score) in betweenness {
            out.put(vec![node, DataValue::from(score)]);
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

/// Closeness centrality (Wasserman-Faust normalisation).
///
/// **Complexity:** O(V * E log V) — one Dijkstra pass per node.
///
/// **When to use:** Ranking nodes by how quickly they can reach the rest
/// of the network.
pub(crate) struct ClosenessCentrality;

impl FixedRule for ClosenessCentrality {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?.ensure_min_len(2)?;
        let undirected = payload.bool_option("undirected", Some(false))?;
        let (adjacency, all_nodes) = build_adjacency(edges, undirected, "ClosenessCentrality")?;
        let node_count = all_nodes.len();
        if node_count == 0 {
            return Ok(());
        }

        for source in &all_nodes {
            let distances = single_source_distances(&adjacency, source);
            // WHY: matches the reference Wasserman-Faust normalisation, which
            // sums over every *finite* entry including the source's own
            // zero-distance to itself — an isolated sink therefore scores
            // +inf (1*1/0/denominator) rather than 0, and both are treated
            // as valid non-negative closeness values downstream.
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "reachable-node count fits comfortably in f64 mantissa for graph sizes this crate handles"
            )]
            let reachable: f64 = distances.values().filter(|d| d.is_finite()).count() as f64;
            let total_distance: f64 = distances.values().filter(|d| d.is_finite()).sum();
            #[expect(
                clippy::as_conversions,
                clippy::cast_precision_loss,
                reason = "node count fits comfortably in f64 mantissa for graph sizes this crate handles"
            )]
            let denominator = (node_count - 1) as f64;
            let score = (reachable * reachable) / total_distance / denominator;
            out.put(vec![source.clone(), DataValue::from(score)]);
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

/// Cost-only single-source Dijkstra: distance from `source` to every node
/// it can reach (`f64::INFINITY` for the rest).
fn single_source_distances(adjacency: &Adjacency, source: &DataValue) -> BTreeMap<DataValue, f64> {
    let mut distance: BTreeMap<DataValue, f64> = BTreeMap::from([(source.clone(), 0.0)]);
    let mut queue: BinaryHeap<Reverse<(OrderedFloat<f64>, DataValue)>> = BinaryHeap::new();
    queue.push(Reverse((OrderedFloat(0.0), source.clone())));
    while let Some(Reverse((OrderedFloat(cost), node))) = queue.pop() {
        if distance.get(&node).is_some_and(|&best| cost > best) {
            continue;
        }
        for (neighbor, weight) in adjacency.get(&node).into_iter().flatten() {
            let candidate = cost + weight;
            if distance
                .get(neighbor)
                .is_none_or(|&existing| candidate < existing)
            {
                distance.insert(neighbor.clone(), candidate);
                queue.push(Reverse((OrderedFloat(candidate), neighbor.clone())));
            }
        }
    }
    distance
}

/// One source's contribution to every node's betweenness score, added
/// directly into `betweenness`.
#[expect(
    clippy::float_cmp,
    reason = "tie detection compares costs computed via the same accumulation path — exact equality is intended"
)]
fn accumulate_brandes(
    adjacency: &Adjacency,
    source: &DataValue,
    betweenness: &mut BTreeMap<DataValue, f64>,
) {
    let mut distance: BTreeMap<DataValue, f64> = BTreeMap::from([(source.clone(), 0.0)]);
    let mut path_count: BTreeMap<DataValue, f64> = BTreeMap::from([(source.clone(), 1.0)]);
    let mut predecessors: BTreeMap<DataValue, Vec<DataValue>> = BTreeMap::new();

    let mut queue: BinaryHeap<Reverse<(OrderedFloat<f64>, DataValue)>> = BinaryHeap::new();
    queue.push(Reverse((OrderedFloat(0.0), source.clone())));
    while let Some(Reverse((OrderedFloat(cost), node))) = queue.pop() {
        if distance.get(&node).is_some_and(|&best| cost > best) {
            continue;
        }
        let node_paths = path_count.get(&node).copied().unwrap_or(0.0);
        for (neighbor, weight) in adjacency.get(&node).into_iter().flatten() {
            let candidate = cost + weight;
            match distance.get(neighbor).copied() {
                Some(existing) if candidate < existing => {
                    distance.insert(neighbor.clone(), candidate);
                    path_count.insert(neighbor.clone(), node_paths);
                    predecessors.insert(neighbor.clone(), vec![node.clone()]);
                    queue.push(Reverse((OrderedFloat(candidate), neighbor.clone())));
                }
                Some(existing) if candidate == existing => {
                    *path_count.entry(neighbor.clone()).or_insert(0.0) += node_paths;
                    predecessors
                        .entry(neighbor.clone())
                        .or_default()
                        .push(node.clone());
                }
                None => {
                    distance.insert(neighbor.clone(), candidate);
                    path_count.insert(neighbor.clone(), node_paths);
                    predecessors.insert(neighbor.clone(), vec![node.clone()]);
                    queue.push(Reverse((OrderedFloat(candidate), neighbor.clone())));
                }
                _ => {}
            }
        }
    }

    // Process reached nodes farthest-first so every node's dependency is
    // fully accumulated before it propagates back to its predecessors.
    let mut by_distance: Vec<(f64, DataValue)> = distance
        .iter()
        .map(|(node, &d)| (d, node.clone()))
        .collect();
    by_distance.sort_by(|a, b| b.0.total_cmp(&a.0));

    let mut dependency: BTreeMap<DataValue, f64> = BTreeMap::new();
    for (_, node) in by_distance {
        let node_dependency = dependency.get(&node).copied().unwrap_or(0.0);
        let node_paths = path_count.get(&node).copied().unwrap_or(1.0);
        if let Some(preds) = predecessors.get(&node) {
            for pred in preds {
                let pred_paths = path_count.get(pred).copied().unwrap_or(1.0);
                if node_paths > 0.0 {
                    let contribution = (pred_paths / node_paths) * (1.0 + node_dependency);
                    *dependency.entry(pred.clone()).or_insert(0.0) += contribution;
                }
            }
        }
        if node != *source {
            *betweenness.entry(node).or_insert(0.0) += node_dependency;
        }
    }
}
