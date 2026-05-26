//! Parallel-execution frontier computation for prompt dependency DAGs.
//!
//! [`compute_frontier`] derives ordered parallel groups from a [`PromptDag`]
//! so that prompts within each group may execute concurrently and groups
//! must execute sequentially.

use std::collections::{HashMap, HashSet};

use crate::dag::condition::{ConditionContext, evaluate_condition};
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
    let mut outputs: HashMap<u32, serde_json::Value> = dag
        .nodes
        .iter()
        .filter_map(|(&num, node)| node.output.clone().map(|output| (num, output)))
        .collect();

    // NOTE: Only non-Done prompts are dispatchable.
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
            .filter(|&&num| node_condition_allows(dag, num, &outputs))
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
        for num in &group {
            if let Some(output) = dag.nodes.get(num).and_then(|node| node.output.clone()) {
                outputs.insert(*num, output);
            }
        }
        groups.push(group);
    }

    groups
}

/// Compute the next currently eligible frontier group from real DAG state.
///
/// Unlike [`compute_frontier`], this does not simulate future completion. It
/// returns only non-terminal nodes whose dependencies are already `Done` and
/// whose `when` condition is satisfied by recorded structured outputs.
#[must_use]
pub fn compute_ready_frontier(dag: &PromptDag) -> Vec<u32> {
    let completed: HashSet<u32> = dag
        .nodes
        .iter()
        .filter(|(_, n)| n.status == PromptStatus::Done)
        .map(|(&num, _)| num)
        .collect();
    let outputs: HashMap<u32, serde_json::Value> = dag
        .nodes
        .iter()
        .filter_map(|(&num, node)| node.output.clone().map(|output| (num, output)))
        .collect();
    let all_numbers: HashSet<u32> = dag.nodes.keys().copied().collect();

    let mut group: Vec<u32> = dag
        .nodes
        .iter()
        .filter(|(_, node)| {
            matches!(
                node.status,
                PromptStatus::Pending | PromptStatus::Ready | PromptStatus::Blocked
            )
        })
        .filter(|(_, node)| {
            node.depends_on
                .iter()
                .filter(|dep| all_numbers.contains(dep))
                .all(|dep| completed.contains(dep))
        })
        .filter(|&(&num, _)| node_condition_allows(dag, num, &outputs))
        .map(|(&num, _)| num)
        .collect();

    group.sort_unstable();
    group
}

fn node_condition_allows(
    dag: &PromptDag,
    number: u32,
    outputs: &HashMap<u32, serde_json::Value>,
) -> bool {
    let Some(when) = dag.nodes.get(&number).and_then(|node| node.when.as_deref()) else {
        return true;
    };

    let context = ConditionContext::from_prompt_outputs(outputs);
    evaluate_condition(when, &context).unwrap_or(false)
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

    #[test]
    fn frontier_gates_branch_by_structured_output_condition() {
        let mut dag = PromptDag::new();
        dag.add_node_with_contract(
            1,
            vec![],
            crate::dag::ContextPolicy::Fresh,
            Some(crate::dag::NodeOutputFormat {
                schema: serde_json::json!({
                    "type": "object",
                    "required": ["severity"],
                    "properties": {
                        "severity": { "enum": ["high", "low"] }
                    }
                }),
            }),
            None,
        )
        .unwrap();
        dag.add_node_with_contract(
            2,
            vec![1],
            crate::dag::ContextPolicy::Fresh,
            None,
            Some("$1.output.severity == 'high'".to_owned()),
        )
        .unwrap();
        dag.add_node_with_contract(
            3,
            vec![1],
            crate::dag::ContextPolicy::Fresh,
            None,
            Some("$1.output.severity == 'low'".to_owned()),
        )
        .unwrap();

        dag.complete_node(1, Some(serde_json::json!({ "severity": "high" })))
            .unwrap();

        assert_eq!(compute_ready_frontier(&dag), vec![2]);
        assert_eq!(compute_frontier(&dag), vec![vec![2]]);
    }
}
