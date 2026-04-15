//! Random walk over graphs.
//!
//! Performs one or more random walks from each starting node, optionally
//! using a user-provided weight expression to bias edge selection.  Each
//! walk records the sequence of visited nodes.
//!
//! Reference: Lovasz, L. (1993). "Random Walks on Graphs: A Survey."
//! *Combinatorics, Paul Erdos is Eighty*, Vol. 2, 1--46.
use std::collections::BTreeMap;

use compact_str::CompactString;
use itertools::Itertools;
use rand::distr::weighted::WeightedIndex;
use rand::prelude::*;

use crate::data::expr::{Expr, eval_bytecode};
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::error::GraphAlgorithmSnafu;
use crate::fixed_rule::{BadExprValueError, FixedRule, FixedRulePayload, NodeNotFoundError};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Random walk with optional weighted edge selection.
///
/// **Complexity:** O(S * I * (d + W)) where S is starting nodes, I is
/// iterations, d is out-degree, W is weight evaluation cost.  Each step
/// samples from outgoing edges.
///
/// **When to use:** Node embedding (DeepWalk/Node2Vec), graph sampling,
/// or simulating diffusion processes.
pub(crate) struct RandomWalk;

#[expect(
    clippy::too_many_lines,
    reason = "random walk setup, weighted/unweighted branching, and output generation kept together for clarity"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "graph random walk indices are bounds-checked by the CSR adjacency structure and node existence"
)]
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
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

        let mut maybe_weight = payload.expr_option("weight", None).ok(); // WHY: optional; absence means unweighted random walk
        let mut maybe_weight_bytecode = None;
        if let Some(weight) = &mut maybe_weight {
            let mut nodes_binding = nodes.get_binding_map(0);
            let nodes_arity = nodes.arity()?;
            let edges_binding = edges.get_binding_map(nodes_arity);
            nodes_binding.extend(edges_binding);
            weight.fill_binding_indices(&nodes_binding)?;
            maybe_weight_bytecode = Some((weight.compile()?, weight.span()));
        }
        let maybe_weight_bytecode = maybe_weight_bytecode;
        let mut stack = vec![];

        let mut walk_counter = 0i64;
        let mut rng = rand::rng();
        for start_node in starting.iter()? {
            let start_node = start_node?;
            let start_node_key = &start_node[0];
            let starting_tuple = nodes.prefix_iter(start_node_key)?.next().ok_or_else(
                || -> crate::error::InternalError {
                    NodeNotFoundError {
                        missing: start_node_key.clone(),
                        span: starting.span(),
                    }
                    .into()
                },
            )??;
            for _ in 0..iterations {
                walk_counter += 1;
                let mut current_tuple = starting_tuple.clone();
                let mut path = vec![start_node_key.clone()];
                for _ in 0..steps {
                    let current_node_key = &current_tuple[0];
                    let candidate_steps: Vec<_> =
                        edges.prefix_iter(current_node_key)?.try_collect()?;
                    if candidate_steps.is_empty() {
                        break;
                    }
                    let next_step = if let Some((weight_bytecode, _span)) = &maybe_weight_bytecode {
                        let weights: Vec<_> = candidate_steps
                                .iter()
                                .map(|tuple| -> Result<f64> {
                                    let mut combined = current_tuple.clone();
                                    combined.extend_from_slice(tuple);
                                    Ok(
                                        match eval_bytecode(weight_bytecode, &combined, &mut stack)?
                                        {
                                            DataValue::Num(n) => {
                                                let f = n.get_float();
                                                if f < 0. {
                                                    return Err(BadExprValueError(
                                                    DataValue::from(f),
                                                    "'weight' must evaluate to a non-negative number"
                                                        .to_string(),
                                                )
                                                    .into());
                                                }
                                                f
                                            }
                                            v => {
                                                return Err(BadExprValueError(
                                                v,
                                                "'weight' must evaluate to a non-negative number"
                                                    .to_string(),
                                            )
                                                .into());
                                            }
                                        },
                                    )
                                })
                                .try_collect()?;
                        let distribution = WeightedIndex::new(&weights).map_err(|err| {
                            GraphAlgorithmSnafu {
                                algorithm: "random_walk",
                                message: format!("invalid edge weights: {err}"),
                            }
                            .build()
                        })?;
                        &candidate_steps[distribution.sample(&mut rng)]
                    } else {
                        candidate_steps.choose(&mut rng).ok_or_else(|| {
                                GraphAlgorithmSnafu {
                                    algorithm: "random_walk",
                                    message:
                                        "candidate step set is unexpectedly empty after non-empty check",
                                }
                                .build()
                            })?
                    };
                    let next_node = &next_step[1];
                    path.push(next_node.clone());
                    current_tuple = nodes.prefix_iter(next_node)?.next().ok_or_else(
                        || -> crate::error::InternalError {
                            NodeNotFoundError {
                                missing: next_node.clone(),
                                span: nodes.span(),
                            }
                            .into()
                        },
                    )??;
                    poison.check()?;
                }
                out.put(vec![
                    DataValue::from(walk_counter),
                    start_node_key.clone(),
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
