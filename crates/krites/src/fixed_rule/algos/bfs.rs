//! Breadth-first traversal that reports the first `limit` nodes matching a
//! predicate, reached from one or more starting nodes.
//!
//! Discovery order is level-by-level (BFS), so among nodes satisfying the
//! condition, closer nodes are reported first. A node already discovered
//! from an earlier starting node is not re-explored from a later one.
//!
//! Reference: Cormen, T.H. et al. (2009). *Introduction to Algorithms*,
//! 3rd ed., MIT Press, Section 22.2.
#![expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeMap/BTreeSet key"
)]
use std::collections::{BTreeMap, BTreeSet, VecDeque};

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

/// Breadth-first search with predicate-based target discovery.
///
/// **Complexity:** O(V + E) over the union of nodes/edges reachable from
/// the starting set; stops early once `limit` matches are found.
///
/// **When to use:** Finding the closest predicate-satisfying nodes in an
/// unweighted graph, or a full reachable-node sweep layer by layer.
pub(crate) struct Bfs;

#[expect(
    clippy::indexing_slicing,
    reason = "edge relation arity is checked via ensure_min_len(2) before traversal begins"
)]
impl FixedRule for Bfs {
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

        let mut discovered: BTreeSet<DataValue> = BTreeSet::new();
        let mut came_from: BTreeMap<DataValue, DataValue> = BTreeMap::new();
        let mut matches: Vec<(DataValue, DataValue)> = vec![];
        let mut pred_stack = vec![];

        'roots: for root_row in roots.iter()? {
            let root = root_row?.into_iter().next().unwrap_or(DataValue::Null);
            if !discovered.insert(root.clone()) {
                continue;
            }

            let mut frontier: VecDeque<DataValue> = VecDeque::new();
            frontier.push_back(root.clone());

            while let Some(current) = frontier.pop_front() {
                for edge in edges.prefix_iter(&current)? {
                    let edge = edge?;
                    let neighbor = &edge[1];
                    if discovered.contains(neighbor) {
                        continue;
                    }
                    discovered.insert(neighbor.clone());
                    came_from.insert(neighbor.clone(), current.clone());

                    let neighbor_tuple = if node_only_predicate {
                        vec![neighbor.clone()]
                    } else {
                        nodes.prefix_iter(neighbor)?.next().ok_or_else(
                            || -> crate::error::InternalError {
                                NodeNotFoundError {
                                    missing: neighbor.clone(),
                                    span: nodes.span(),
                                }
                                .into()
                            },
                        )??
                    };

                    if eval_bytecode_pred(
                        &program,
                        &neighbor_tuple,
                        &mut pred_stack,
                        condition_span,
                    )? {
                        matches.push((root.clone(), neighbor.clone()));
                        if matches.len() >= limit {
                            break 'roots;
                        }
                    }

                    frontier.push_back(neighbor.clone());
                    poison.check()?;
                }
            }
        }

        for (root, target) in matches {
            out.put(vec![
                root.clone(),
                target.clone(),
                DataValue::List(reconstruct_path(&came_from, &root, &target)?),
            ]);
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

/// Walk `came_from` backwards from `target` to `root`, then reverse.
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
fn reconstruct_path(
    came_from: &BTreeMap<DataValue, DataValue>,
    root: &DataValue,
    target: &DataValue,
) -> Result<Vec<DataValue>> {
    let mut path = vec![];
    let mut cursor = target.clone();
    while cursor != *root {
        path.push(cursor.clone());
        cursor = came_from
            .get(&cursor)
            .ok_or_else(|| {
                GraphAlgorithmSnafu {
                    algorithm: "bfs",
                    message: "path reconstruction lost the predecessor chain",
                }
                .build()
            })?
            .clone();
    }
    path.push(cursor);
    path.reverse();
    Ok(path)
}
