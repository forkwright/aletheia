//! Strongly (or weakly) connected components via Kosaraju's algorithm.
//!
//! Two iterative DFS passes: the first records a finishing order over the
//! forward graph, the second walks the reverse graph in reverse-finish
//! order, and each reverse-DFS tree is exactly one strongly connected
//! component. When `strong` is false the input is treated as undirected
//! (forward and reverse graphs coincide), which reduces this to ordinary
//! connected-component flood fill.
//!
//! Reference: Sharir, M. (1981). "A Strong-Connectivity Algorithm and its
//! Applications in Data Flow Analysis." *Computers & Mathematics with
//! Applications*, 7(1), 67--72. (Independently attributed to S.R. Kosaraju,
//! unpublished, 1978.)
#![expect(
    clippy::mutable_key_type,
    reason = "DataValue implements Hash via canonical byte representation — safe as BTreeMap/BTreeSet key"
)]
use std::collections::BTreeMap;

use compact_str::CompactString;

use crate::data::expr::Expr;
use crate::data::symb::Symbol;
use crate::data::value::DataValue;
use crate::error::InternalResult as Result;
use crate::fixed_rule::{FixedRule, FixedRulePayload};
use crate::parse::SourceSpan;
use crate::runtime::db::Poison;
use crate::runtime::temp_store::RegularTempStore;

/// Strongly (or weakly) connected components via Kosaraju's algorithm.
///
/// **Complexity:** O(V + E) — two linear DFS passes.
///
/// **When to use:** Cycle/mutual-reachability detection in directed
/// graphs (`strong: true`), or partitioning an undirected graph into
/// components (`strong: false`).
#[cfg(feature = "graph-algo")]
pub(crate) struct StronglyConnectedComponent {
    strong: bool,
}

#[cfg(feature = "graph-algo")]
impl StronglyConnectedComponent {
    pub(crate) fn new(strong: bool) -> Self {
        Self { strong }
    }
}

#[cfg(feature = "graph-algo")]
impl FixedRule for StronglyConnectedComponent {
    fn run(
        &self,
        payload: FixedRulePayload<'_, '_>,
        out: &mut RegularTempStore,
        poison: Poison,
    ) -> Result<()> {
        let edges = payload.get_input(0)?;

        let mut forward: BTreeMap<DataValue, Vec<DataValue>> = BTreeMap::new();
        let mut backward: BTreeMap<DataValue, Vec<DataValue>> = BTreeMap::new();
        let mut all_nodes: Vec<DataValue> = vec![];
        let mut seen = std::collections::BTreeSet::new();

        for row in edges.iter()? {
            let mut fields = row?.into_iter();
            let Some(source) = fields.next() else {
                continue;
            };
            let Some(target) = fields.next() else {
                continue;
            };
            for node in [&source, &target] {
                if seen.insert(node.clone()) {
                    all_nodes.push(node.clone());
                }
            }
            forward
                .entry(source.clone())
                .or_default()
                .push(target.clone());
            backward
                .entry(target.clone())
                .or_default()
                .push(source.clone());
            if !self.strong {
                forward
                    .entry(target.clone())
                    .or_default()
                    .push(source.clone());
                backward.entry(source).or_default().push(target);
            }
            poison.check()?;
        }

        let finish_order = dfs_finish_order(&all_nodes, &forward, &poison)?;

        let mut component_of: BTreeMap<DataValue, i64> = BTreeMap::new();
        let mut next_component: i64 = 0;
        for node in finish_order.into_iter().rev() {
            if component_of.contains_key(&node) {
                continue;
            }
            for reached in collect_reachable(&node, &backward, &component_of) {
                component_of.insert(reached, next_component);
            }
            next_component += 1;
            poison.check()?;
        }

        for (node, component) in &component_of {
            out.put(vec![node.clone(), DataValue::from(*component)]);
        }

        if let Ok(node_relation) = payload.get_input(1) {
            for row in node_relation.iter()? {
                let node = row?.into_iter().next().unwrap_or(DataValue::Null);
                if let std::collections::btree_map::Entry::Vacant(entry) = component_of.entry(node)
                {
                    out.put(vec![entry.key().clone(), DataValue::from(next_component)]);
                    entry.insert(next_component);
                    next_component += 1;
                }
                poison.check()?;
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
        Ok(2)
    }
}

/// Iterative post-order DFS over every node, forward graph. Returns nodes
/// in the order they finished (last-finished node is last in the vector).
#[expect(
    clippy::result_large_err,
    reason = "InternalError carries structured context — boxing deferred to avoid API churn"
)]
fn dfs_finish_order(
    all_nodes: &[DataValue],
    forward: &BTreeMap<DataValue, Vec<DataValue>>,
    poison: &Poison,
) -> Result<Vec<DataValue>> {
    let mut visited = std::collections::BTreeSet::new();
    let mut order = vec![];

    for root in all_nodes {
        if visited.contains(root) {
            continue;
        }
        // Explicit stack of (node, next-child-index-to-visit) frames.
        let mut frames: Vec<(DataValue, usize)> = vec![(root.clone(), 0)];
        visited.insert(root.clone());

        while let Some((node, child_idx)) = frames.pop() {
            let children = forward.get(&node);
            let next_unvisited = children
                .and_then(|kids| {
                    kids.iter()
                        .skip(child_idx)
                        .position(|c| !visited.contains(c))
                })
                .map(|offset| child_idx + offset);

            match next_unvisited {
                Some(idx) => {
                    // SAFETY: `idx` came from `position` over `children`, which is `Some`.
                    let child = children.and_then(|kids| kids.get(idx)).cloned();
                    frames.push((node.clone(), idx + 1));
                    if let Some(child) = child {
                        visited.insert(child.clone());
                        frames.push((child, 0));
                    }
                }
                None => order.push(node),
            }
            poison.check()?;
        }
    }
    Ok(order)
}

/// Flood-fill from `root` over `graph`, skipping nodes already assigned a
/// component. Returns every node reached, `root` included.
fn collect_reachable(
    root: &DataValue,
    graph: &BTreeMap<DataValue, Vec<DataValue>>,
    already_assigned: &BTreeMap<DataValue, i64>,
) -> Vec<DataValue> {
    let mut reached = vec![];
    let mut open = vec![root.clone()];
    let mut visited = std::collections::BTreeSet::from([root.clone()]);
    while let Some(node) = open.pop() {
        reached.push(node.clone());
        if let Some(neighbors) = graph.get(&node) {
            for neighbor in neighbors {
                if !already_assigned.contains_key(neighbor) && visited.insert(neighbor.clone()) {
                    open.push(neighbor.clone());
                }
            }
        }
    }
    reached
}
