//! Breadth-first search traversal.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::engine::error::DbResult as Result;
use compact_str::CompactString;

use crate::engine::data::expr::{Expr, eval_bytecode_pred};
use crate::engine::data::symb::Symbol;
use crate::engine::data::value::DataValue;
use crate::engine::fixed_rule::{FixedRule, FixedRulePayload, NodeNotFoundError};
use crate::engine::parse::SourceSpan;
use crate::engine::runtime::db::Poison;
use crate::engine::runtime::temp_store::RegularTempStore;

pub(crate) struct Bfs;

impl FixedRule for Bfs {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?.ensure_min_len(2)?;
        let nodes = payload.get_input(1)?;
        let starting_nodes = payload.get_input(2).unwrap_or(nodes);
        let limit = payload.pos_integer_option("limit", Some(1))?;
        let mut condition = payload.expr_option("condition", None)?;
        let binding_map = nodes.get_binding_map(0);
        condition.fill_binding_indices(&binding_map)?;
        let condition_bytecode = condition.compile()?;
        let condition_span = condition.span();
        let binding_indices = condition.binding_indices()?;
        let skip_query_nodes = binding_indices.is_subset(&BTreeSet::from([0]));

        let mut visited: BTreeSet<DataValue> = Default::default();
        let mut backtrace: BTreeMap<DataValue, DataValue> = Default::default();
        let mut found: Vec<(DataValue, DataValue)> = vec![];
        let mut stack = vec![];

        'outer: for node_tuple in starting_nodes.iter()? {
            let node_tuple = node_tuple?;
            let starting_node = &node_tuple[0];
            if visited.contains(starting_node) {
                continue;
            }
            visited.insert(starting_node.clone());

            let mut queue: VecDeque<DataValue> = VecDeque::default();
            queue.push_front(starting_node.clone());

            while let Some(candidate) = queue.pop_back() {
                for edge in edges.prefix_iter(&candidate)? {
                    let edge = edge?;
                    let to_node = &edge[1];
                    if visited.contains(to_node) {
                        continue;
                    }

                    visited.insert(to_node.clone());
                    backtrace.insert(to_node.clone(), candidate.clone());

                    let cand_tuple = if skip_query_nodes {
                        vec![to_node.clone()]
                    } else {
                        nodes
                            .prefix_iter(to_node)?
                            .next()
                            .ok_or_else(|| NodeNotFoundError {
                                missing: candidate.clone(),
                                span: nodes.span(),
                            })??
                    };

                    if eval_bytecode_pred(
                        &condition_bytecode,
                        &cand_tuple,
                        &mut stack,
                        condition_span,
                    )? {
                        found.push((starting_node.clone(), to_node.clone()));
                        if found.len() >= limit {
                            break 'outer;
                        }
                    }

                    queue.push_front(to_node.clone());
                    poison.check()?;
                }
            }
        }

        for (starting, ending) in found {
            let mut route = vec![];
            let mut current = ending.clone();
            while current != starting {
                route.push(current.clone());
                current = backtrace.get(&current).unwrap().clone();
            }
            route.push(starting.clone());
            route.reverse();
            let tuple = vec![starting, ending, DataValue::List(route)];
            out.put(tuple);
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
