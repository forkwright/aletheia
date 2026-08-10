//! Unweighted shortest path between explicit source/target sets, via BFS.
//!
//! For every (start, end) pair, reports the minimum-hop path, or a `Null`
//! path when `end` is unreached from `start`.
//!
//! Reference: Cormen, T.H. et al. (2009). *Introduction to Algorithms*,
//! 3rd ed., MIT Press, Section 22.2.
#![expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeMap/BTreeSet key"
)]
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use compact_str::CompactString;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::error::GraphAlgorithmSnafu;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Unweighted shortest path via BFS between explicit start/end sets.
///
/// **Complexity:** O(S * (V + E)) — one BFS per starting node.
///
/// **When to use:** Minimum hop-count paths in unweighted graphs. For
/// weighted graphs, use `ShortestPathDijkstra`.
pub(crate) struct ShortestPathBFS;

impl FixedRule for ShortestPathBFS {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?.ensure_min_len(2)?;
        let starts_rel = payload.get_input(1)?.ensure_min_len(1)?;
        let ends_rel = payload.get_input(2)?.ensure_min_len(1)?;

        let mut starts: Vec<DataValue> = vec![];
        for row in starts_rel.iter()? {
            starts.push(row?.into_iter().next().unwrap_or(DataValue::Null));
        }
        let mut ends: BTreeSet<DataValue> = BTreeSet::new();
        for row in ends_rel.iter()? {
            ends.insert(row?.into_iter().next().unwrap_or(DataValue::Null));
        }

        for start in &starts {
            let came_from = bfs_predecessors(edges, start, &ends, &poison)?;

            for end in &ends {
                match reconstruct(&came_from, start, end)? {
                    Some(path) => out.put(vec![start.clone(), end.clone(), DataValue::List(path)]),
                    None => out.put(vec![start.clone(), end.clone(), DataValue::Null]),
                }
            }
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

/// Run BFS from `start`, stopping once every node in `wanted` has been
/// discovered (or the frontier is exhausted). Returns the predecessor map.
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
fn bfs_predecessors(
    edges: crate::fixed_rule::FixedRuleInputRelation<'_, '_>,
    start: &DataValue,
    wanted: &BTreeSet<DataValue>,
    poison: &Poison,
) -> Result<BTreeMap<DataValue, DataValue>> {
    let mut remaining = wanted.clone();
    remaining.remove(start);
    let mut visited: BTreeSet<DataValue> = BTreeSet::from([start.clone()]);
    let mut came_from: BTreeMap<DataValue, DataValue> = BTreeMap::new();

    let mut frontier: VecDeque<DataValue> = VecDeque::from([start.clone()]);
    while let Some(current) = frontier.pop_front() {
        if remaining.is_empty() {
            break;
        }
        for edge in edges.prefix_iter(&current)? {
            let edge = edge?;
            let Some(neighbor) = edge.get(1) else {
                continue;
            };
            if visited.contains(neighbor) {
                continue;
            }
            visited.insert(neighbor.clone());
            came_from.insert(neighbor.clone(), current.clone());
            remaining.remove(neighbor);
            frontier.push_back(neighbor.clone());
        }
        poison.check()?;
    }
    Ok(came_from)
}

/// Reconstruct the path from `start` to `end` out of a predecessor map.
/// Returns `None` when `end` has no predecessor entry (unreached, or the
/// degenerate `start == end` case — mirroring the traversal, which never
/// records the root as its own predecessor).
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
fn reconstruct(
    came_from: &BTreeMap<DataValue, DataValue>,
    start: &DataValue,
    end: &DataValue,
) -> Result<Option<Vec<DataValue>>> {
    if !came_from.contains_key(end) {
        return Ok(None);
    }
    let mut path = vec![];
    let mut cursor = end.clone();
    while cursor != *start {
        path.push(cursor.clone());
        cursor = came_from
            .get(&cursor)
            .ok_or_else(|| {
                GraphAlgorithmSnafu {
                    algorithm: "shortest_path_bfs",
                    message: "path reconstruction lost the predecessor chain",
                }
                .build()
            })?
            .clone();
    }
    path.push(cursor);
    path.reverse();
    Ok(Some(path))
}
