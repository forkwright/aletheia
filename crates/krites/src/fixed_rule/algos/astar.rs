//! A* shortest-path search with heuristic guidance.
//!
//! Finds the shortest path between a single source-target pair using a
//! user-provided heuristic function to guide exploration.  With an admissible
//! (non-overestimating) heuristic, A* is optimal and typically explores far
//! fewer nodes than Dijkstra.
//!
//! Reference: Hart, P.E., Nilsson, N.J., Raphael, B. (1968). "A Formal
//! Basis for the Heuristic Determination of Minimum Cost Paths." *IEEE
//! Transactions on Systems Science and Cybernetics*, 4(2), 100--107.
use std::cmp::Reverse;
use std::collections::BTreeMap;

use compact_str::CompactString;
use ordered_float::OrderedFloat;
use priority_queue::PriorityQueue;

use crate::data::expr::{Expr, eval_bytecode};
use crate::data::symb::Symbol;
use crate::data::tuple::Tuple;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::error::GraphAlgorithmSnafu;
use crate::fixed_rule::{
    BadExprValueError, FixedRule, FixedRuleInputRelation, FixedRulePayload, NodeNotFoundError,
};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// A* shortest-path search.
///
/// Requires four input relations (edges, nodes, starting, goals) and a
/// `heuristic` expression option.  The heuristic is evaluated per candidate
/// node and must return a non-negative numeric estimate of the remaining
/// cost to the goal.
///
/// **Complexity:** O(E log V) worst case; with a good heuristic, explores
/// significantly fewer nodes than Dijkstra.
///
/// **When to use:** Single-pair shortest path when a domain-specific
/// heuristic is available (e.g., Euclidean distance for spatial graphs).
pub(crate) struct ShortestPathAStar;

#[expect(
    clippy::indexing_slicing,
    reason = "input tuple indices are bounds-checked by ensure_min_len and iter arity guarantees"
)]
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

        let mut binding_map = nodes.get_binding_map(0);
        let goal_binding_map = goals.get_binding_map(nodes.arity()?);
        binding_map.extend(goal_binding_map);
        heuristic.fill_binding_indices(&binding_map)?;
        for start in starting.iter()? {
            let start = start?;
            for goal in goals.iter()? {
                let goal = goal?;
                let (cost, path) = astar(&start, &goal, edges, nodes, &heuristic, poison.clone())?;
                out.put(vec![
                    start[0].clone(),
                    goal[0].clone(),
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

/// Core A* pathfinding loop.
///
/// **Complexity:** O(E log V) worst case.  The priority queue operations
/// dominate.
#[expect(
    clippy::indexing_slicing,
    reason = "graph A* indices are bounds-checked by the visited set and input arity validation"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Poison is lightweight and passed by value for ergonomic .check() calls"
)]
#[expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeMap key"
)]
#[expect(
    clippy::default_trait_access,
    reason = "Default::default() is idiomatic for type-inferred collection initialization"
)]
#[expect(
    clippy::cloned_instead_of_copied,
    reason = "OrderedFloat<f64> is Copy but .cloned() matches surrounding pattern"
)]
fn astar(
    starting: &Tuple,
    goal: &Tuple,
    edges: FixedRuleInputRelation<'_, '_>,
    nodes: FixedRuleInputRelation<'_, '_>,
    heuristic: &Expr,
    poison: Poison,
) -> Result<(f64, Vec<DataValue>)> {
    let start_node = &starting[0];
    let goal_node = &goal[0];
    let heuristic_bytecode = heuristic.compile()?;
    let mut stack = vec![];
    let mut eval_heuristic = |node: &Tuple| -> Result<f64> {
        let mut combined = node.clone();
        combined.extend_from_slice(goal);
        let cost_val = eval_bytecode(&heuristic_bytecode, &combined, &mut stack)?;
        let cost = cost_val
            .get_float()
            .ok_or_else(|| BadExprValueError(cost_val, "a number is required".to_string()))?;
        if cost.is_nan() {
            return Err(BadExprValueError(
                DataValue::from(cost),
                "a number is required".to_string(),
            )
            .into());
        }
        Ok(cost)
    };
    let mut back_trace: BTreeMap<DataValue, DataValue> = Default::default();
    let mut g_score: BTreeMap<DataValue, f64> = BTreeMap::from([(start_node.clone(), 0.)]);
    let mut open_set: PriorityQueue<DataValue, (Reverse<OrderedFloat<f64>>, usize)> =
        PriorityQueue::new();
    open_set.push(start_node.clone(), (Reverse(OrderedFloat(0.)), 0));
    let mut sub_priority: usize = 0;
    while let Some((node, (Reverse(OrderedFloat(cost)), _))) = open_set.pop() {
        if node == *goal_node {
            let mut current = node;
            let mut route = vec![];
            while current != *start_node {
                let prev = back_trace
                    .get(&current)
                    .ok_or_else(|| {
                        GraphAlgorithmSnafu {
                            algorithm: "astar",
                            message: "back_trace missing entry during path reconstruction",
                        }
                        .build()
                    })?
                    .clone();
                route.push(current);
                current = prev;
            }
            route.push(current);
            route.reverse();
            return Ok((cost, route));
        }

        for edge in edges.prefix_iter(&node)? {
            let edge = edge?;
            let edge_destination = &edge[1];
            let edge_cost = match edge.get(2) {
                None => 1.,
                Some(cost) => cost.get_float().ok_or_else(|| {
                    BadExprValueError(
                        edge_destination.clone(),
                        "edge cost must be a number".to_string(),
                    )
                })?,
            };
            if edge_cost.is_nan() {
                return Err(BadExprValueError(
                    edge_destination.clone(),
                    "edge cost must be a number".to_string(),
                )
                .into());
            }

            let cost_to_source = g_score.get(&node).cloned().unwrap_or(f64::INFINITY);
            let tentative_cost = cost_to_source + edge_cost;
            let previous_cost = g_score
                .get(edge_destination)
                .cloned()
                .unwrap_or(f64::INFINITY);
            if tentative_cost < previous_cost {
                back_trace.insert(edge_destination.clone(), node.clone());
                g_score.insert(edge_destination.clone(), tentative_cost);

                let destination_tuple = nodes.prefix_iter(edge_destination)?.next().ok_or_else(
                    || -> crate::error::InternalError {
                        NodeNotFoundError {
                            missing: edge_destination.clone(),
                            span: nodes.span(),
                        }
                        .into()
                    },
                )??;

                let heuristic_cost = eval_heuristic(&destination_tuple)?;
                sub_priority += 1;
                open_set.push_increase(
                    edge_destination.clone(),
                    (
                        Reverse(OrderedFloat(tentative_cost + heuristic_cost)),
                        sub_priority,
                    ),
                );
            }
            poison.check()?;
        }
    }
    Ok((f64::INFINITY, vec![]))
}
