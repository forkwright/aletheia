// WHY: Prompt dependency graph for dispatch planning. Tracks which prompts
// are ready to dispatch based on dependency satisfaction, and computes
// parallel execution waves for maximum throughput.

use std::collections::{HashMap, HashSet};
use std::fmt;

/// Status of a prompt node within the dependency graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PromptStatus {
    /// Node has been added to the graph but not yet evaluated.
    Pending,
    /// All dependencies satisfied — ready to dispatch.
    Ready,
    /// Currently being executed by an agent session.
    InProgress,
    /// Completed successfully.
    Done,
    /// Execution failed.
    Failed,
    /// Waiting on unsatisfied dependencies.
    Blocked,
}

impl fmt::Display for PromptStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pending => write!(f, "PENDING"),
            Self::Ready => write!(f, "READY"),
            Self::InProgress => write!(f, "IN_PROGRESS"),
            Self::Done => write!(f, "DONE"),
            Self::Failed => write!(f, "FAILED"),
            Self::Blocked => write!(f, "BLOCKED"),
        }
    }
}

/// A node in the prompt dependency graph.
#[derive(Debug, Clone)]
pub struct DagNode {
    /// Unique prompt number.
    pub number: u32,
    /// Prompt numbers this prompt depends on (forward edges).
    pub depends_on: Vec<u32>,
    /// Current execution status.
    pub status: PromptStatus,
}

/// Errors that can occur during DAG construction or validation.
#[derive(Debug, snafu::Snafu)]
#[non_exhaustive]
pub enum DagError {
    /// A cycle was detected in the dependency graph.
    #[snafu(display("cycle detected in dependency graph: {}", format_cycle(cycle)))]
    Cycle {
        /// Prompt numbers forming the cycle.
        cycle: Vec<u32>,
    },

    /// One or more prompts reference dependencies not present in the graph.
    #[snafu(display("{}", format_missing_deps(broken)))]
    MissingDependencies {
        /// All broken `(prompt, missing_dep)` pairs.
        broken: Vec<(u32, u32)>,
    },

    /// A prompt number was referenced but not found in the graph.
    #[snafu(display("prompt {number} not found in the graph"))]
    InvalidPrompt {
        /// The prompt number that was not found.
        number: u32,
    },

    /// Duplicate prompt number detected during construction.
    #[snafu(display("duplicate prompt number {number} in graph"))]
    DuplicateNode {
        /// The duplicate prompt number.
        number: u32,
    },
}

fn format_cycle(cycle: &[u32]) -> String {
    cycle
        .iter()
        .map(|n| format!("#{n}"))
        .collect::<Vec<_>>()
        .join(" -> ")
}

fn format_missing_deps(broken: &[(u32, u32)]) -> String {
    let mut lines: Vec<String> = broken
        .iter()
        .map(|(prompt, missing)| {
            format!("  prompt {prompt} depends on {missing}, which is not in the graph")
        })
        .collect();
    lines.sort();
    format!(
        "{} broken dependency reference(s):\n{}",
        broken.len(),
        lines.join("\n")
    )
}

/// The prompt dependency directed acyclic graph.
///
/// Tracks all known prompts and their dependency relationships. Provides
/// topological ordering for dispatch planning via [`compute_frontier`] and
/// runtime status tracking via [`PromptDag::set_status`].
#[derive(Debug, Default)]
pub struct PromptDag {
    /// Prompt number to node mapping.
    pub(crate) nodes: HashMap<u32, DagNode>,
}

impl PromptDag {
    /// Create an empty DAG.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a prompt node to the graph.
    ///
    /// The initial status is `Pending`. Returns [`DagError::DuplicateNode`] if
    /// `number` is already present.
    pub fn add_node(&mut self, number: u32, depends_on: Vec<u32>) -> Result<(), DagError> {
        if self.nodes.contains_key(&number) {
            return Err(DagError::DuplicateNode { number });
        }
        self.nodes.insert(
            number,
            DagNode {
                number,
                depends_on,
                status: PromptStatus::Pending,
            },
        );
        Ok(())
    }

    /// Update the status of a prompt node.
    ///
    /// Returns [`DagError::InvalidPrompt`] if `number` is not in the graph.
    pub fn set_status(&mut self, number: u32, status: PromptStatus) -> Result<(), DagError> {
        self.nodes
            .get_mut(&number)
            .ok_or(DagError::InvalidPrompt { number })?
            .status = status;
        Ok(())
    }

    /// Return prompt numbers currently in [`PromptStatus::Ready`] state.
    #[must_use]
    pub fn get_ready(&self) -> Vec<u32> {
        let mut ready: Vec<u32> = self
            .nodes
            .values()
            .filter(|n| n.status == PromptStatus::Ready)
            .map(|n| n.number)
            .collect();
        ready.sort_unstable();
        ready
    }

    /// Validate the graph: check for missing dependencies and cycles.
    ///
    /// Collects ALL broken `depends_on` references before returning, so callers
    /// can fix every problem in one pass. Cycle detection uses DFS with
    /// three-color marking (white/gray/black).
    pub fn validate(&self) -> Result<(), DagError> {
        // WHY: Check missing deps first — simpler to diagnose, collect all at once.
        let all_numbers: HashSet<u32> = self.nodes.keys().copied().collect();
        let mut broken: Vec<(u32, u32)> = Vec::new();

        for node in self.nodes.values() {
            for dep in &node.depends_on {
                if !all_numbers.contains(dep) {
                    broken.push((node.number, *dep));
                }
            }
        }

        if !broken.is_empty() {
            broken.sort_unstable();
            return Err(DagError::MissingDependencies { broken });
        }

        self.detect_cycle()
    }

    /// Detect cycles using DFS with three-color marking.
    fn detect_cycle(&self) -> Result<(), DagError> {
        #[derive(Clone, Copy, PartialEq, Eq)]
        enum Color {
            White,
            Gray,
            Black,
        }

        fn dfs(
            node: u32,
            nodes: &HashMap<u32, DagNode>,
            colors: &mut HashMap<u32, Color>,
            path: &mut Vec<u32>,
        ) -> Result<(), DagError> {
            colors.insert(node, Color::Gray);
            path.push(node);

            let deps = nodes
                .get(&node)
                .map(|n| n.depends_on.as_slice())
                .unwrap_or_default();

            for &dep in deps {
                let Some(&color) = colors.get(&dep) else {
                    continue;
                };
                match color {
                    Color::Gray => {
                        // WHY: Back-edge found — extract the cycle from the DFS stack.
                        // `start` is the index of `dep` within `path`, which is always
                        // a valid slice start since `position` returns an index into `path`.
                        let start = path.iter().position(|&n| n == dep).unwrap_or(0);
                        #[expect(
                            clippy::indexing_slicing,
                            reason = "start is the result of position() on path, so it is a valid index"
                        )]
                        let mut cycle: Vec<u32> = path[start..].to_vec();
                        cycle.push(dep);
                        return Err(DagError::Cycle { cycle });
                    }
                    Color::White => dfs(dep, nodes, colors, path)?,
                    Color::Black => {}
                }
            }

            colors.insert(node, Color::Black);
            path.pop();
            Ok(())
        }

        let mut colors: HashMap<u32, Color> =
            self.nodes.keys().map(|&k| (k, Color::White)).collect();
        let mut path: Vec<u32> = Vec::new();

        // NOTE: Process in sorted order for deterministic cycle reporting.
        let mut keys: Vec<u32> = self.nodes.keys().copied().collect();
        keys.sort_unstable();

        for node in keys {
            // `colors` is built from `self.nodes.keys()` and `keys` is the same set,
            // so every `node` is guaranteed to be present.
            #[expect(
                clippy::indexing_slicing,
                reason = "keys are derived from the same node set used to build colors"
            )]
            if colors[&node] == Color::White {
                dfs(node, &self.nodes, &mut colors, &mut path)?;
            }
        }

        Ok(())
    }
}

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
            .copied()
            .collect();

        if group.is_empty() {
            // SAFETY: No progress means a cycle exists. Break rather than loop
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
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // PromptDag API tests
    // -------------------------------------------------------------------------

    #[test]
    fn add_node_and_get_status() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        assert_eq!(dag.nodes[&1].status, PromptStatus::Pending);
    }

    #[test]
    fn add_node_duplicate_returns_error() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        let err = dag.add_node(1, vec![]).unwrap_err();
        assert!(matches!(err, DagError::DuplicateNode { number: 1 }));
    }

    #[test]
    fn set_status_updates_node() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        dag.set_status(1, PromptStatus::Ready).unwrap();
        assert_eq!(dag.nodes[&1].status, PromptStatus::Ready);
    }

    #[test]
    fn set_status_missing_node_returns_error() {
        let mut dag = PromptDag::new();
        let err = dag.set_status(99, PromptStatus::Done).unwrap_err();
        assert!(matches!(err, DagError::InvalidPrompt { number: 99 }));
    }

    #[test]
    fn get_ready_returns_only_ready_nodes() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        dag.add_node(2, vec![]).unwrap();
        dag.add_node(3, vec![]).unwrap();
        dag.set_status(1, PromptStatus::Ready).unwrap();
        dag.set_status(3, PromptStatus::Done).unwrap();

        let ready = dag.get_ready();
        assert_eq!(ready, vec![1], "only node 1 should be Ready");
    }

    #[test]
    fn get_ready_returns_sorted() {
        let mut dag = PromptDag::new();
        dag.add_node(5, vec![]).unwrap();
        dag.add_node(2, vec![]).unwrap();
        dag.add_node(8, vec![]).unwrap();
        dag.set_status(5, PromptStatus::Ready).unwrap();
        dag.set_status(2, PromptStatus::Ready).unwrap();
        dag.set_status(8, PromptStatus::Ready).unwrap();

        assert_eq!(dag.get_ready(), vec![2, 5, 8]);
    }

    // -------------------------------------------------------------------------
    // Validation tests
    // -------------------------------------------------------------------------

    #[test]
    fn validate_passes_for_valid_dag() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![]).unwrap();
        dag.add_node(2, vec![1]).unwrap();
        dag.add_node(3, vec![1]).unwrap();
        dag.add_node(4, vec![2, 3]).unwrap();
        dag.validate().expect("valid diamond DAG should pass");
    }

    #[test]
    fn validate_detects_cycle_three_nodes() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![3]).unwrap();
        dag.add_node(2, vec![1]).unwrap();
        dag.add_node(3, vec![2]).unwrap();

        let err = dag.validate().expect_err("cycle should fail validation");
        match err {
            DagError::Cycle { cycle } => {
                assert!(
                    cycle.len() >= 3,
                    "cycle path should contain all three nodes"
                );
            }
            other => panic!("expected Cycle, got: {other}"),
        }
    }

    #[test]
    fn validate_detects_two_node_cycle() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![2]).unwrap();
        dag.add_node(2, vec![1]).unwrap();

        assert!(matches!(dag.validate(), Err(DagError::Cycle { .. })));
    }

    #[test]
    fn validate_detects_missing_dependency() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![99]).unwrap();

        let err = dag.validate().expect_err("missing dep should fail");
        match err {
            DagError::MissingDependencies { broken } => {
                assert_eq!(broken.len(), 1);
                assert_eq!(broken[0], (1, 99));
            }
            other => panic!("expected MissingDependencies, got: {other}"),
        }
    }

    #[test]
    fn validate_collects_all_missing_deps() {
        let mut dag = PromptDag::new();
        dag.add_node(1, vec![88]).unwrap();
        dag.add_node(2, vec![99]).unwrap();
        dag.add_node(3, vec![88, 77]).unwrap();

        let err = dag.validate().expect_err("missing deps should fail");
        match err {
            DagError::MissingDependencies { broken } => {
                assert_eq!(broken.len(), 4, "should report all 4 broken refs");
                assert!(broken.contains(&(1, 88)));
                assert!(broken.contains(&(2, 99)));
                assert!(broken.contains(&(3, 77)));
                assert!(broken.contains(&(3, 88)));
            }
            other => panic!("expected MissingDependencies, got: {other}"),
        }
    }

    // -------------------------------------------------------------------------
    // compute_frontier tests
    // -------------------------------------------------------------------------

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
        // A->B,C->D: three waves [A], [B,C], [D]
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
        // Two roots, each with a child: [1,2], [3,4]
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

    // -------------------------------------------------------------------------
    // Display tests
    // -------------------------------------------------------------------------

    #[test]
    fn prompt_status_display() {
        assert_eq!(PromptStatus::Pending.to_string(), "PENDING");
        assert_eq!(PromptStatus::Ready.to_string(), "READY");
        assert_eq!(PromptStatus::InProgress.to_string(), "IN_PROGRESS");
        assert_eq!(PromptStatus::Done.to_string(), "DONE");
        assert_eq!(PromptStatus::Failed.to_string(), "FAILED");
        assert_eq!(PromptStatus::Blocked.to_string(), "BLOCKED");
    }
}
