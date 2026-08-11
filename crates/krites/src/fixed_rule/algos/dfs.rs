//! Depth-first traversal that reports the first `limit` nodes matching a
//! predicate, reached by following edges out from one or more starting
//! nodes.
//!
//! Traversal commits to one path as deep as it goes before backtracking,
//! so among nodes satisfying the condition, discovery order reflects path
//! depth rather than distance from a start node — unlike breadth-first
//! search, a DFS match is not guaranteed to be the nearest one. A node
//! already visited from an earlier start is not re-explored from a later
//! one.
//!
//! Reference: Cormen, T.H. et al. (2009). *Introduction to Algorithms*,
//! 3rd ed., MIT Press, Section 22.3.
#![expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeSet key"
)]
use std::collections::{BTreeMap, BTreeSet};

use compact_str::CompactString;

use crate::data::expr::{Expr, eval_bytecode_pred};
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::{FixedRule, FixedRulePayload, NodeNotFoundError};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// A recorded match: the start node it was reached from, the matching
/// node itself, and the path between them in traversal order.
type PathHit = (DataValue, DataValue, Vec<DataValue>);

/// Depth-first search with predicate-based target discovery.
///
/// **Complexity:** O(V + E) over the union of nodes/edges reachable from
/// the starting set; stops as soon as `limit` matches are found.
///
/// **When to use:** Exhausting one branch of a graph before trying its
/// siblings — cycle probing, connectivity checks, or any search where
/// reaching *some* satisfying node matters more than reaching the
/// closest one.
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
        let starts = payload.get_input(2).unwrap_or(nodes);

        let limit = payload.pos_integer_option("limit", Some(1))?;
        let mut condition = payload.expr_option("condition", None)?;
        condition.fill_binding_indices(&nodes.get_binding_map(0))?;
        let program = condition.compile()?;
        let condition_span = condition.span();
        let condition_reads_id_only = condition.binding_indices()?.is_subset(&BTreeSet::from([0]));

        let mut visited: BTreeSet<DataValue> = BTreeSet::new();
        let mut hits: Vec<PathHit> = vec![];
        let mut scratch = vec![];

        'walk: for start_row in starts.iter()? {
            let start = start_row?.into_iter().next().unwrap_or(DataValue::Null);
            if !visited.insert(start.clone()) {
                continue;
            }

            // An explicit stack rather than recursion: DFS can commit to a
            // single path as deep as the graph goes before it backtracks,
            // and nothing here bounds that depth.
            //
            // INVARIANT: a node enters `visited` at the moment it is
            // pushed, not when it is later popped — so two branches that
            // both reach it before either is explored still queue it only
            // once, and `open` never carries more than one entry per node.
            //
            // Each stack frame already carries the path taken to reach it,
            // so a match is reported by extending that path in place —
            // there is no predecessor map to walk backward and reverse
            // afterward. The trade is a clone of the path on every push;
            // for a single unbranching chain of length n that is O(n^2)
            // bytes copied overall, accepted here for having the answer
            // ready the instant a match is found.
            let mut open: Vec<(DataValue, Vec<DataValue>)> =
                vec![(start.clone(), vec![start.clone()])];

            while let Some((here, path_so_far)) = open.pop() {
                poison.check()?;

                for edge in edges.prefix_iter(&here)? {
                    let edge = edge?;
                    let next = edge[1].clone();
                    if !visited.insert(next.clone()) {
                        continue;
                    }

                    let mut path_to_next = path_so_far.clone();
                    path_to_next.push(next.clone());

                    let next_tuple = if condition_reads_id_only {
                        vec![next.clone()]
                    } else {
                        nodes.prefix_iter(&next)?.next().ok_or_else(
                            || -> crate::error::InternalError {
                                NodeNotFoundError {
                                    missing: next.clone(),
                                    span: nodes.span(),
                                }
                                .into()
                            },
                        )??
                    };

                    if eval_bytecode_pred(&program, &next_tuple, &mut scratch, condition_span)? {
                        hits.push((start.clone(), next.clone(), path_to_next.clone()));
                        if hits.len() >= limit {
                            break 'walk;
                        }
                    }

                    open.push((next, path_to_next));
                }
            }
        }

        for (start, target, path) in hits {
            out.put(vec![start, target, DataValue::List(path)]);
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
