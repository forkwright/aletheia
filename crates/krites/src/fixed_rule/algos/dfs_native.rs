//! Depth-first traversal (explicit stack, not recursive) that reports the
//! first `limit` nodes matching a predicate, reached from one or more
//! starting nodes.
//!
//! Reference: Cormen, T.H. et al. (2009). *Introduction to Algorithms*,
//! 3rd ed., MIT Press, Section 22.3.
#![expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeMap/BTreeSet key"
)]
use std::collections::{BTreeMap, BTreeSet};

use compact_str::CompactString;

use crate::data::expr::{Expr, eval_bytecode_pred};
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::error::GraphAlgorithmSnafu;
use crate::fixed_rule::{FixedRule, FixedRulePayload, NodeNotFoundError};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Depth-first search (iterative) with predicate-based target discovery.
///
/// **Complexity:** O(V + E) over the union of nodes/edges reachable from
/// the starting set.
///
/// **When to use:** Finding *any* path (not necessarily shortest) to a
/// predicate-satisfying node, when depth-first exploration order suffices.
pub(crate) struct Dfs;

#[expect(
    clippy::indexing_slicing,
    reason = "edge relation arity is checked via ensure_min_len(2) before traversal begins"
)]
impl FixedRule for Dfs {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?.ensure_min_len(2)?;
        let nodes = payload.get_input(1)?;
        let roots = payload.get_input(2).unwrap_or(nodes);

        let limit = payload.pos_integer_option("limit", Some(1))?;
        let mut condition = payload.expr_option("condition", None)?;
        condition.fill_binding_indices(&nodes.get_binding_map(0))?;
        let program = condition.compile()?;
        let condition_span = condition.span();
        let node_only_predicate = condition.binding_indices()?.is_subset(&BTreeSet::from([0]));

        let mut visited: BTreeSet<DataValue> = BTreeSet::new();
        let mut came_from: BTreeMap<DataValue, DataValue> = BTreeMap::new();
        let mut matches: Vec<(DataValue, DataValue)> = vec![];
        let mut pred_stack = vec![];

        'roots: for root_row in roots.iter()? {
            let root = root_row?.into_iter().next().unwrap_or(DataValue::Null);
            if visited.contains(&root) {
                continue;
            }

            let mut open: Vec<DataValue> = vec![root.clone()];
            while let Some(candidate) = open.pop() {
                if visited.contains(&candidate) {
                    continue;
                }

                let candidate_tuple = if node_only_predicate {
                    vec![candidate.clone()]
                } else {
                    nodes.prefix_iter(&candidate)?.next().ok_or_else(
                        || -> crate::error::InternalError {
                            NodeNotFoundError {
                                missing: candidate.clone(),
                                span: nodes.span(),
                            }
                            .into()
                        },
                    )??
                };

                if eval_bytecode_pred(&program, &candidate_tuple, &mut pred_stack, condition_span)?
                {
                    matches.push((root.clone(), candidate.clone()));
                    if matches.len() >= limit {
                        break 'roots;
                    }
                }
                visited.insert(candidate.clone());

                for edge in edges.prefix_iter(&candidate)? {
                    let edge = edge?;
                    let neighbor = &edge[1];
                    if visited.contains(neighbor) {
                        continue;
                    }
                    came_from.insert(neighbor.clone(), candidate.clone());
                    open.push(neighbor.clone());
                    poison.check()?;
                }
            }
        }

        for (root, target) in matches {
            let mut path = vec![];
            let mut cursor = target.clone();
            while cursor != root {
                path.push(cursor.clone());
                cursor = came_from
                    .get(&cursor)
                    .ok_or_else(|| {
                        GraphAlgorithmSnafu {
                            algorithm: "dfs",
                            message: "path reconstruction lost the predecessor chain",
                        }
                        .build()
                    })?
                    .clone();
            }
            path.push(cursor);
            path.reverse();
            out.put(vec![root, target, DataValue::List(path)]);
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
