//! Random walks over a graph, optionally biased by a weight expression.
//!
//! Runs `iterations` independent walks of up to `steps` hops from each
//! starting node. A walk stops early if it reaches a node with no outgoing
//! edges. With no `weight` option, the next hop is chosen uniformly at
//! random among outgoing edges; with `weight`, hops are sampled
//! proportionally to the (non-negative) value the expression evaluates to
//! for each candidate edge.
//!
//! Reference: Lovasz, L. (1993). "Random Walks on Graphs: A Survey."
//! *Combinatorics, Paul Erdos is Eighty*, Vol. 2, 1--46.
use std::collections::BTreeMap;

use compact_str::CompactString;
use rand::distr::weighted::WeightedIndex;
use rand::prelude::*;

use crate::data::expr::{Expr, eval_bytecode};
use crate::data::symb::Symbol;
use crate::data::tuple::Tuple;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::error::GraphAlgorithmSnafu;
use crate::fixed_rule::{BadExprValueError, FixedRule, FixedRulePayload, NodeNotFoundError};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Random walk with optional weighted edge selection.
///
/// **Complexity:** O(S * I * steps) walk-steps, each doing an edge lookup
/// (plus a weight-expression evaluation per candidate, if weighted).
///
/// **When to use:** Node embeddings, graph sampling, diffusion simulation.
pub(crate) struct RandomWalk;

impl FixedRule for RandomWalk {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?.ensure_min_len(2)?;
        let nodes = payload.get_input(1)?;
        let starting = payload.get_input(2)?;
        let iterations = payload.pos_integer_option("iterations", Some(1))?;
        let steps = payload.pos_integer_option("steps", None)?;

        let weight_program = match payload.expr_option("weight", None) {
            Err(_) => None,
            Ok(mut weight_expr) => {
                let mut binding = nodes.get_binding_map(0);
                binding.extend(edges.get_binding_map(nodes.arity()?));
                weight_expr.fill_binding_indices(&binding)?;
                Some(weight_expr.compile()?)
            }
        };

        let mut rng = rand::rng();
        let mut eval_stack = vec![];
        let mut walk_id: i64 = 0;

        for start_row in starting.iter()? {
            let start_row = start_row?;
            let Some(start_key) = start_row.into_iter().next() else {
                continue;
            };
            let start_tuple = fetch_node_tuple(nodes, &start_key)?;

            for _ in 0..iterations {
                walk_id += 1;
                let mut path = vec![start_key.clone()];
                let mut current = start_tuple.clone();

                for _ in 0..steps {
                    let current_key = current.first().cloned().unwrap_or(DataValue::Null);
                    let candidates: Vec<Tuple> = edges
                        .prefix_iter(&current_key)?
                        .collect::<Result<Vec<_>>>()?;
                    if candidates.is_empty() {
                        break;
                    }
                    let chosen = match &weight_program {
                        None => candidates.choose(&mut rng),
                        Some(program) => {
                            let mut weights = vec![];
                            for candidate in &candidates {
                                let mut combined = current.clone();
                                combined.extend_from_slice(candidate);
                                let value = eval_bytecode(program, &combined, &mut eval_stack)?;
                                let DataValue::Num(n) = value else {
                                    return Err(BadExprValueError(
                                        value,
                                        "'weight' must evaluate to a non-negative number"
                                            .to_string(),
                                    )
                                    .into());
                                };
                                let f = n.get_float();
                                if f < 0.0 {
                                    return Err(BadExprValueError(
                                        DataValue::from(f),
                                        "'weight' must evaluate to a non-negative number"
                                            .to_string(),
                                    )
                                    .into());
                                }
                                weights.push(f);
                            }
                            let distribution = WeightedIndex::new(&weights).map_err(|err| {
                                GraphAlgorithmSnafu {
                                    algorithm: "random_walk",
                                    message: format!("invalid edge weights: {err}"),
                                }
                                .build()
                            })?;
                            candidates.get(distribution.sample(&mut rng))
                        }
                    };
                    let Some(edge) = chosen else {
                        break;
                    };
                    let Some(next_key) = edge.get(1).cloned() else {
                        break;
                    };
                    path.push(next_key.clone());
                    current = fetch_node_tuple(nodes, &next_key)?;
                    poison.check()?;
                }

                out.put(vec![
                    DataValue::from(walk_id),
                    start_key.clone(),
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
        Ok(3)
    }
}

#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
fn fetch_node_tuple(
    nodes: crate::fixed_rule::FixedRuleInputRelation<'_, '_>,
    key: &DataValue,
) -> Result<Tuple> {
    nodes
        .prefix_iter(key)?
        .next()
        .ok_or_else(|| -> crate::error::InternalError {
            NodeNotFoundError {
                missing: key.clone(),
                span: nodes.span(),
            }
            .into()
        })?
}
