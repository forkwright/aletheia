//! Yen's k shortest loopless (simple) paths between a source and target.
//!
//! Finds the single shortest path, then repeatedly looks for the next-best
//! path by temporarily forbidding the edges/nodes that would reproduce an
//! already-found path's prefix, and re-running Dijkstra from each "spur"
//! node along the most recently accepted path.
//!
//! Reference: Yen, J.Y. (1971). "Finding the K Shortest Loopless Paths in
//! a Network." *Management Science*, 17(11), 712--716.
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

/// Yen's k-shortest loopless paths.
///
/// **Complexity:** O(K * V * (E log V)) — up to K rounds, each probing up
/// to V spur nodes with a Dijkstra run.
///
/// **When to use:** Alternative routes beyond the single shortest path,
/// e.g. for route diversity or backup-path planning.
pub(crate) struct KShortestPathYen;

impl FixedRule for KShortestPathYen {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?.ensure_min_len(2)?;
        let starting = payload.get_input(1)?;
        let termination = payload.get_input(2)?;
        let undirected = payload.bool_option("undirected", Some(false))?;
        let k = payload.pos_integer_option("k", None)?;

        let adjacency = build_weighted_adjacency(edges, undirected)?;

        let mut starts: BTreeSet<DataValue> = BTreeSet::new();
        for row in starting.iter()? {
            starts.insert(row?.into_iter().next().unwrap_or(DataValue::Null));
        }
        let mut goals: BTreeSet<DataValue> = BTreeSet::new();
        for row in termination.iter()? {
            goals.insert(row?.into_iter().next().unwrap_or(DataValue::Null));
        }

        for start in &starts {
            for goal in &goals {
                for (cost, path) in yen_k_shortest(&adjacency, start, goal, k, &poison)? {
                    out.put(vec![
                        start.clone(),
                        goal.clone(),
                        DataValue::from(cost),
                        DataValue::List(path),
                    ]);
                }
                poison.check()?;
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
        Ok(4)
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
fn build_weighted_adjacency(
    edges: crate::fixed_rule::FixedRuleInputRelation<'_, '_>,
    undirected: bool,
) -> Result<Adjacency> {
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
                        rule: "KShortestPathYen",
                        message: format!("edge weight {w:?} is not a number"),
                    }
                    .build()
                })?;
                if !f.is_finite() || f < 0.0 {
                    return Err(InvalidInputSnafu {
                        rule: "KShortestPathYen",
                        message: format!("edge weight {w:?} must be finite and non-negative"),
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
        if undirected {
            adjacency.entry(target).or_default().push((source, weight));
        } else {
            adjacency.entry(target).or_default();
        }
    }
    Ok(adjacency)
}

/// Dijkstra from `start` to a single `goal`, honoring forbidden edges and
/// forbidden intermediate nodes. Returns `None` when unreachable.
fn constrained_dijkstra(
    adjacency: &Adjacency,
    start: &DataValue,
    goal: &DataValue,
    forbidden_edges: &BTreeSet<(DataValue, DataValue)>,
    forbidden_nodes: &BTreeSet<DataValue>,
) -> Option<(f64, Vec<DataValue>)> {
    let mut distance: BTreeMap<DataValue, f64> = BTreeMap::from([(start.clone(), 0.0)]);
    let mut predecessor: BTreeMap<DataValue, DataValue> = BTreeMap::new();
    let mut queue: BinaryHeap<Reverse<(OrderedFloat<f64>, DataValue)>> = BinaryHeap::new();
    queue.push(Reverse((OrderedFloat(0.0), start.clone())));

    while let Some(Reverse((OrderedFloat(cost), node))) = queue.pop() {
        if distance.get(&node).is_some_and(|&best| cost > best) {
            continue;
        }
        if node == *goal {
            break;
        }
        for (neighbor, weight) in adjacency.get(&node).into_iter().flatten() {
            if forbidden_nodes.contains(neighbor) {
                continue;
            }
            if forbidden_edges.contains(&(node.clone(), neighbor.clone())) {
                continue;
            }
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

    let cost = *distance.get(goal)?;
    let mut path = vec![goal.clone()];
    let mut cursor = goal.clone();
    while cursor != *start {
        let prev = predecessor.get(&cursor)?;
        path.push(prev.clone());
        cursor = prev.clone();
    }
    path.reverse();
    Some((cost, path))
}

/// Total weight of a known-valid path, by summing its constituent edges.
#[expect(
    clippy::indexing_slicing,
    reason = "windows(2) guarantees each pair has exactly 2 elements"
)]
fn path_cost(adjacency: &Adjacency, path: &[DataValue]) -> f64 {
    path.windows(2)
        .map(|pair| {
            let (from, to) = (&pair[0], &pair[1]);
            adjacency
                .get(from)
                .into_iter()
                .flatten()
                .find(|(neighbor, _)| neighbor == to)
                .map_or(0.0, |(_, weight)| *weight)
        })
        .sum()
}

/// Yen's algorithm: the `k` shortest loopless paths from `start` to `goal`.
#[expect(
    clippy::indexing_slicing,
    reason = "spur_index is always < previous.len() - 1, guarding every range/index used below"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
fn yen_k_shortest(
    adjacency: &Adjacency,
    start: &DataValue,
    goal: &DataValue,
    k: usize,
    poison: &Poison,
) -> Result<Vec<(f64, Vec<DataValue>)>> {
    let mut accepted: Vec<(f64, Vec<DataValue>)> = vec![];
    let Some(first) =
        constrained_dijkstra(adjacency, start, goal, &BTreeSet::new(), &BTreeSet::new())
    else {
        return Ok(accepted);
    };
    accepted.push(first);

    let mut candidates: Vec<(f64, Vec<DataValue>)> = vec![];
    while accepted.len() < k {
        // SAFETY: `accepted` always has at least one element by this point.
        let previous = accepted.last().map(|(_, p)| p.clone()).unwrap_or_default();
        if previous.len() < 2 {
            break;
        }

        for spur_index in 0..previous.len() - 1 {
            let root_path = &previous[0..=spur_index];
            let Some(spur_node) = root_path.last() else {
                continue;
            };

            let mut forbidden_edges: BTreeSet<(DataValue, DataValue)> = BTreeSet::new();
            for (_, existing) in &accepted {
                if existing.len() > spur_index
                    && &existing[0..=spur_index] == root_path
                    && let Some(next_hop) = existing.get(spur_index + 1)
                {
                    forbidden_edges.insert((spur_node.clone(), next_hop.clone()));
                }
            }
            let forbidden_nodes: BTreeSet<DataValue> =
                root_path[..spur_index].iter().cloned().collect();

            if let Some((spur_cost, spur_path)) = constrained_dijkstra(
                adjacency,
                spur_node,
                goal,
                &forbidden_edges,
                &forbidden_nodes,
            ) {
                let mut total_path = root_path[..spur_index].to_vec();
                total_path.extend(spur_path);
                let total_cost = path_cost(adjacency, root_path) + spur_cost;
                if !candidates
                    .iter()
                    .any(|(_, existing)| *existing == total_path)
                    && !accepted.iter().any(|(_, existing)| *existing == total_path)
                {
                    candidates.push((total_cost, total_path));
                }
            }
            poison.check()?;
        }

        if candidates.is_empty() {
            break;
        }
        candidates.sort_by(|(cost_a, _), (cost_b, _)| cost_a.total_cmp(cost_b));
        accepted.push(candidates.remove(0));
    }

    Ok(accepted)
}
