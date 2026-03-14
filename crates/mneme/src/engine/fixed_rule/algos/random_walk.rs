//! Random walk over graphs.
use std::collections::BTreeMap;

use crate::engine::error::InternalResult as Result;
use compact_str::CompactString;
use itertools::Itertools;
use rand::distr::weighted::WeightedIndex;
use rand::prelude::*;

use crate::engine::data::expr::{Expr, eval_bytecode};
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::DataValue;
use crate::engine::fixed_rule::error::GraphAlgorithmSnafu;
use crate::engine::fixed_rule::{
    BadExprValueError, FixedRule, FixedRulePayload, NodeNotFoundError,
};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::db::Poison;
use crate::engine::runtime::temp_store::RegularTempStore;

pub(crate) struct RandomWalk;

impl FixedRule for RandomWalk {
    #[expect(
        clippy::expect_used,
        reason = "candidate_steps checked non-empty before choose"
    )]
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

        let mut maybe_weight = payload.expr_option("weight", None).ok();
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

        let mut counter = 0i64;
        let mut rng = rand::rng();
        for start_node in starting.iter()? {
            let start_node = start_node?;
            let start_node_key = &start_node[0];
            let starting_tuple = nodes.prefix_iter(start_node_key)?.next().ok_or_else(
                || -> crate::engine::error::InternalError {
                    NodeNotFoundError {
                        missing: start_node_key.clone(),
                        span: starting.span(),
                    }
                    .into()
                },
            )??;
            for _ in 0..iterations {
                counter += 1;
                let mut current_tuple = starting_tuple.clone();
                let mut path = vec![start_node_key.clone()];
                for _ in 0..steps {
                    let cur_node_key = &current_tuple[0];
                    let candidate_steps: Vec<_> = edges.prefix_iter(cur_node_key)?.try_collect()?;
                    if candidate_steps.is_empty() {
                        break;
                    }
                    let next_step = if let Some((weight_expr, _span)) = &maybe_weight_bytecode {
                        let weights: Vec<_> = candidate_steps
                            .iter()
                            .map(|t| -> Result<f64> {
                                let mut cand = current_tuple.clone();
                                cand.extend_from_slice(t);
                                Ok(match eval_bytecode(weight_expr, &cand, &mut stack)? {
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
                                })
                            })
                            .try_collect()?;
                        let dist = WeightedIndex::new(&weights).map_err(|e| {
                            GraphAlgorithmSnafu {
                                algorithm: "random_walk",
                                message: format!("invalid edge weights: {e}"),
                            }
                            .build()
                        })?;
                        &candidate_steps[dist.sample(&mut rng)]
                    } else {
                        candidate_steps
                            .choose(&mut rng)
                            .expect("candidate_steps checked non-empty above")
                    };
                    let next_node = &next_step[1];
                    path.push(next_node.clone());
                    current_tuple = nodes.prefix_iter(next_node)?.next().ok_or_else(
                        || -> crate::engine::error::InternalError {
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
                    DataValue::from(counter),
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
