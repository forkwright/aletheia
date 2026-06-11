//! Parallel-execution frontier computation for prompt dependency DAGs.
//!
//! [`compute_frontier`] derives ordered parallel groups from a [`PromptDag`]
//! so that prompts within each group may execute concurrently and groups
//! must execute sequentially.

use std::collections::{HashMap, HashSet};

use crate::dag::{PromptDag, PromptStatus};

/// Compute the parallel dispatch frontier from a DAG.
///
/// Returns a sequence of parallel groups. Each group contains prompt numbers
/// that can execute simultaneously. Groups must execute in order: all prompts
/// in group N must complete before group N+1 can start.
///
/// [`PromptStatus::Done`] prompts are pre-seeded as completed so they satisfy
/// dependency checks but never appear in the dispatch plan.
///
/// # Algorithm
///
/// 1. Pre-seed `Done` prompts as completed.
/// 2. Find all non-Done prompts whose in-graph dependencies are all completed.
/// 3. Those form group 0. Mark them completed.
/// 4. Repeat until all prompts are assigned.
///
/// If the graph has a cycle, the algorithm terminates early with whatever
/// groups it managed to compute. Callers should [`PromptDag::validate`] before
/// calling this function.
#[must_use]
pub fn compute_frontier(dag: &PromptDag) -> Vec<Vec<u32>> {
    let mut groups: Vec<Vec<u32>> = Vec::new();

    // WHY: Done prompts satisfy dependency resolution but must not appear in
    // the dispatch plan. Pre-seed them as completed.
    let mut completed: HashSet<u32> = dag
        .nodes
        .iter()
        .filter(|(_, n)| n.status == PromptStatus::Done)
        .map(|(&num, _)| num)
        .collect();

    let dispatchable: HashSet<u32> = dag
        .nodes
        .iter()
        .filter(|(_, n)| n.status != PromptStatus::Done)
        .map(|(&num, _)| num)
        .collect();

    let all_numbers: HashSet<u32> = dag.nodes.keys().copied().collect();

    // WHY: Build a dependency map from nodes, filtering to in-graph deps only.
    // Deps outside the graph are treated as already satisfied.
    let deps: HashMap<u32, Vec<u32>> = dag
        .nodes
        .iter()
        .map(|(&num, node)| {
            let in_graph: Vec<u32> = node
                .depends_on
                .iter()
                .filter(|d| all_numbers.contains(d))
                .copied()
                .collect();
            (num, in_graph)
        })
        .collect();

    let total = dispatchable.len();
    let mut dispatched = 0;

    while dispatched < total {
        let mut group: Vec<u32> = dispatchable
            .iter()
            .filter(|&&num| !completed.contains(&num))
            .filter(|&&num| {
                deps.get(&num)
                    .is_none_or(|d| d.iter().all(|dep| completed.contains(dep)))
            })
            .copied()
            .collect();

        if group.is_empty() {
            // INVARIANT: No progress means a cycle exists. Break rather than loop
            // forever — validate() should have caught this beforehand.
            break;
        }

        group.sort_unstable();
        dispatched += group.len();
        completed.extend(&group);
        groups.push(group);
    }

    groups
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test assertions over fixture data")]
mod tests {
    use super::*;

    #[test]
    fn frontier_empty_dag_returns_empty() {
        let dag = PromptDag::new();
        assert!(compute_frontier(&dag).is_empty());
    }

    #[test]
    fn frontier_isolated_nodes_in_single_group() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        dag.add_node(2, vec![]).unwrap();
        dag.add_node(3, vec![]).unwrap();

        let frontier = compute_frontier(&dag);
        assert_eq!(frontier.len(), 1);
        assert_eq!(frontier[0], vec![1, 2, 3]);
    }

    #[test]
    fn frontier_linear_chain_one_per_group() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        dag.add_node(2, vec![1]).unwrap();
        dag.add_node(3, vec![2]).unwrap();

        let frontier = compute_frontier(&dag);
        assert_eq!(frontier.len(), 3);
        assert_eq!(frontier[0], vec![1]);
        assert_eq!(frontier[1], vec![2]);
        assert_eq!(frontier[2], vec![3]);
    }

    #[test]
    fn frontier_diamond_three_groups() {
        // WHY: Diamond A->B,C->D should produce three groups: [A], [B,C], [D]
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        dag.add_node(2, vec![1]).unwrap();
        dag.add_node(3, vec![1]).unwrap();
        dag.add_node(4, vec![2, 3]).unwrap();

        let frontier = compute_frontier(&dag);
        assert_eq!(frontier.len(), 3);
        assert_eq!(frontier[0], vec![1]);
        assert_eq!(frontier[1], vec![2, 3]);
        assert_eq!(frontier[2], vec![4]);
    }

    #[test]
    fn frontier_done_prompts_excluded_but_satisfy_deps() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        dag.add_node(2, vec![1]).unwrap();
        dag.add_node(3, vec![1]).unwrap();
        dag.add_node(4, vec![2, 3]).unwrap();
        dag.set_status(1, PromptStatus::Done).unwrap();

        let frontier = compute_frontier(&dag);
        // Node 1 is Done: excluded. Nodes 2,3 have their dep (1) satisfied.
        assert_eq!(frontier.len(), 2);
        assert_eq!(frontier[0], vec![2, 3]);
        assert_eq!(frontier[1], vec![4]);
    }

    #[test]
    fn frontier_all_done_returns_empty() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        dag.add_node(2, vec![1]).unwrap();
        dag.set_status(1, PromptStatus::Done).unwrap();
        dag.set_status(2, PromptStatus::Done).unwrap();

        assert!(compute_frontier(&dag).is_empty());
    }

    #[test]
    fn frontier_wide_parallel_two_groups() {
        // NOTE: Two roots each with a child, producing two groups.
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        dag.add_node(2, vec![]).unwrap();
        dag.add_node(3, vec![1]).unwrap();
        dag.add_node(4, vec![2]).unwrap();

        let frontier = compute_frontier(&dag);
        assert_eq!(frontier.len(), 2);
        assert_eq!(frontier[0], vec![1, 2]);
        assert_eq!(frontier[1], vec![3, 4]);
    }

    #[test]
    fn frontier_partially_done_subset() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        dag.add_node(2, vec![]).unwrap();
        dag.add_node(3, vec![2]).unwrap();
        dag.add_node(4, vec![2]).unwrap();
        dag.set_status(1, PromptStatus::Done).unwrap();
        dag.set_status(2, PromptStatus::Done).unwrap();

        let frontier = compute_frontier(&dag);
        assert_eq!(frontier.len(), 1);
        assert_eq!(frontier[0], vec![3, 4]);
    }

    #[test]
    fn frontier_cycle_terminates_without_panic() {
        // WHY: Cyclic graphs should not cause an infinite loop or panic.
        // compute_frontier breaks early when no progress can be made.
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![3]).unwrap();
        dag.add_node(2, vec![1]).unwrap();
        dag.add_node(3, vec![2]).unwrap();

        let frontier = compute_frontier(&dag);
        // No node can be dispatched because every node depends on another
        // node in the cycle that is not yet completed.
        assert!(frontier.is_empty());
    }

    #[test]
    fn frontier_cycle_after_roots_terminates_cleanly() {
        // WHY: A cycle that appears after some valid nodes should still
        // terminate cleanly, returning only the dispatchable groups.
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        dag.add_node(2, vec![1]).unwrap();
        dag.add_node(3, vec![2]).unwrap();
        // Introduce a cycle: 2 now also depends on 3, making 2<->3 a cycle.
        dag.nodes
            .get_mut(&2)
            .expect("node 2 exists")
            .depends_on
            .push(3);

        let frontier = compute_frontier(&dag);
        // Group 0: [1] — root with no deps.
        // Group 1: nothing — 2 depends on 3 (not done), 3 depends on 2 (not done).
        // Algorithm breaks early.
        assert_eq!(frontier.len(), 1);
        assert_eq!(frontier[0], vec![1]);
    }
}
