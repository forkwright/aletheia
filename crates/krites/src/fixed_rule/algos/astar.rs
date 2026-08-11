//! A* shortest-path search guided by a user-supplied heuristic.
//!
//! Runs one A* search per (start, goal) pair. The heuristic expression is
//! evaluated against each candidate node's full tuple (from the `nodes`
//! relation) concatenated with the goal's tuple, and must return a
//! non-negative numeric estimate of remaining cost. With an admissible
//! heuristic, A* explores no more nodes than Dijkstra and often far fewer.
//!
//! Reference: Hart, P.E., Nilsson, N.J., Raphael, B. (1968). "A Formal
//! Basis for the Heuristic Determination of Minimum Cost Paths." *IEEE
//! Transactions on Systems Science and Cybernetics*, 4(2), 100--107.
#![expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeMap/BTreeSet key"
)]
use std::cmp::Reverse;
use std::collections::BTreeMap;

use compact_str::CompactString;
use ordered_float::OrderedFloat;

use crate::data::expr::{Bytecode, Expr, eval_bytecode};
use crate::data::symb::Symbol;
use crate::data::tuple::Tuple;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::{BadExprValueError, FixedRule, FixedRulePayload, NodeNotFoundError};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// A* shortest-path search with a heuristic.
///
/// **Complexity:** O(E log V) worst case, less with an informative
/// heuristic.
///
/// **When to use:** Single-pair shortest path when a domain-specific
/// heuristic (e.g. spatial distance) is available.
pub(crate) struct ShortestPathAStar;

impl FixedRule for ShortestPathAStar {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?.ensure_min_len(2)?;
        let nodes = payload.get_input(1)?;
        let starting = payload.get_input(2)?;
        let goals = payload.get_input(3)?;
        let mut heuristic = payload.expr_option("heuristic", None)?;

        let mut binding = nodes.get_binding_map(0);
        binding.extend(goals.get_binding_map(nodes.arity()?));
        heuristic.fill_binding_indices(&binding)?;
        let program = heuristic.compile()?;
        let mut eval_stack = vec![];

        for start_row in starting.iter()? {
            let start_row = start_row?;
            let Some(start_key) = start_row.first().cloned() else {
                continue;
            };
            for goal_row in goals.iter()? {
                let goal_row = goal_row?;
                let Some(goal_key) = goal_row.first().cloned() else {
                    continue;
                };
                let (cost, path) = search(
                    edges,
                    nodes,
                    &start_key,
                    &goal_key,
                    &goal_row,
                    &program,
                    &mut eval_stack,
                    &poison,
                )?;
                out.put(vec![
                    start_key.clone(),
                    goal_key,
                    DataValue::from(cost),
                    DataValue::List(path),
                ]);
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
    clippy::too_many_arguments,
    reason = "search state threaded explicitly rather than bundled into an ad-hoc context struct"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
fn search(
    edges: crate::fixed_rule::FixedRuleInputRelation<'_, '_>,
    nodes: crate::fixed_rule::FixedRuleInputRelation<'_, '_>,
    start: &DataValue,
    goal: &DataValue,
    goal_tuple: &Tuple,
    heuristic: &[Bytecode],
    eval_stack: &mut Vec<DataValue>,
    poison: &Poison,
) -> Result<(f64, Vec<DataValue>)> {
    let mut g_score: BTreeMap<DataValue, f64> = BTreeMap::from([(start.clone(), 0.0)]);
    let mut predecessor: BTreeMap<DataValue, DataValue> = BTreeMap::new();
    let mut open: std::collections::BinaryHeap<Reverse<(OrderedFloat<f64>, u64, DataValue)>> =
        std::collections::BinaryHeap::new();
    let mut sequence: u64 = 0;
    open.push(Reverse((OrderedFloat(0.0), sequence, start.clone())));

    while let Some(Reverse((_, _, node))) = open.pop() {
        if node == *goal {
            let mut path = vec![node.clone()];
            let mut cursor = node;
            while cursor != *start {
                let Some(prev) = predecessor.get(&cursor) else {
                    break;
                };
                path.push(prev.clone());
                cursor = prev.clone();
            }
            path.reverse();
            let cost = g_score.get(goal).copied().unwrap_or(f64::INFINITY);
            return Ok((cost, path));
        }

        let node_cost = g_score.get(&node).copied().unwrap_or(f64::INFINITY);
        for edge in edges.prefix_iter(&node)? {
            let edge = edge?;
            let Some(neighbor) = edge.get(1).cloned() else {
                continue;
            };
            let edge_cost = match edge.get(2) {
                None => 1.0,
                Some(c) => c.get_float().ok_or_else(|| {
                    BadExprValueError(neighbor.clone(), "edge cost must be a number".to_string())
                })?,
            };
            if !edge_cost.is_finite() {
                return Err(BadExprValueError(
                    neighbor.clone(),
                    "edge cost must be a number".to_string(),
                )
                .into());
            }

            let tentative = node_cost + edge_cost;
            let currently_best = g_score.get(&neighbor).copied().unwrap_or(f64::INFINITY);
            if tentative < currently_best {
                g_score.insert(neighbor.clone(), tentative);
                predecessor.insert(neighbor.clone(), node.clone());

                let neighbor_tuple = nodes.prefix_iter(&neighbor)?.next().ok_or_else(
                    || -> crate::error::InternalError {
                        NodeNotFoundError {
                            missing: neighbor.clone(),
                            span: nodes.span(),
                        }
                        .into()
                    },
                )??;
                let mut combined = neighbor_tuple;
                combined.extend_from_slice(goal_tuple);
                let h_value = eval_bytecode(heuristic, &combined, eval_stack)?;
                let h = h_value.get_float().ok_or_else(|| {
                    BadExprValueError(h_value.clone(), "a number is required".to_string())
                })?;
                if h.is_nan() {
                    return Err(BadExprValueError(
                        DataValue::from(h),
                        "a number is required".to_string(),
                    )
                    .into());
                }

                sequence += 1;
                open.push(Reverse((OrderedFloat(tentative + h), sequence, neighbor)));
            }
            poison.check()?;
        }
    }
    Ok((f64::INFINITY, vec![]))
}
