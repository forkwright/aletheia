//! Graph traversal algorithms: DFS, BFS, RandomWalk.

use std::collections::BTreeMap;
use std::collections::VecDeque;

use rand::Rng;
use rustc_hash::FxHashMap;

use crate::v2::algo::{
    build_adjacency, build_rows, i64_option, require_option,
};
use crate::v2::error::Result;
use crate::v2::rows::Rows;
use crate::v2::value::Value;

/// DFS traversal algorithm.
///
/// Depth-first search from a start node.
/// Output: `(step, node)` pairs in visit order.
#[expect(
    clippy::cast_possible_truncation,
    reason = "usize→i64: step count bounded by max_steps parameter which is < i64::MAX"
)]
pub struct DfsTraversal;

impl super::FixedRule for DfsTraversal {
    fn name(&self) -> &str {
        "dfs"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(2)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let start = require_option(options, "start", self.name())?;
        let max_steps = i64_option(options, "max_steps", 1000) as usize;

        let adj = build_adjacency(edges);

        let mut visited: Vec<Value> = Vec::new();
        let mut seen: FxHashMap<Value, bool> = FxHashMap::default();
        let mut stack: Vec<Value> = vec![start.clone()];

        while let Some(node) = stack.pop() {
            if seen.contains_key(&node) {
                continue;
            }
            if visited.len() >= max_steps {
                break;
            }

            seen.insert(node.clone(), true);
            visited.push(node.clone());

            // Push neighbors in reverse order for consistent traversal
            if let Some(neighbors) = adj.get(&node) {
                for (neighbor, _) in neighbors.iter().rev() {
                    if !seen.contains_key(neighbor) {
                        stack.push(neighbor.clone());
                    }
                }
            }
        }

        let rows: Vec<Vec<Value>> = visited
            .into_iter()
            .enumerate()
            .map(|(step, node)| vec![Value::from(step as i64), node])
            .collect();

        Ok(build_rows(vec!["step".to_owned(), "node".to_owned()], rows))
    }
}

/// BFS traversal algorithm.
///
/// Breadth-first search from a start node.
/// Output: `(step, node)` pairs in visit order (by depth).
#[expect(
    clippy::cast_possible_truncation,
    reason = "usize→i64: step count bounded by max_steps parameter which is < i64::MAX"
)]
pub struct BfsTraversal;

impl super::FixedRule for BfsTraversal {
    fn name(&self) -> &str {
        "bfs_traversal"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(2)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let start = require_option(options, "start", self.name())?;
        let max_steps = i64_option(options, "max_steps", 1000) as usize;

        let adj = build_adjacency(edges);

        let mut visited: Vec<Value> = Vec::new();
        let mut seen: FxHashMap<Value, bool> = FxHashMap::default();
        let mut queue: VecDeque<Value> = VecDeque::new();

        seen.insert(start.clone(), true);
        queue.push_back(start.clone());

        while let Some(node) = queue.pop_front() {
            if visited.len() >= max_steps {
                break;
            }
            visited.push(node.clone());

            if let Some(neighbors) = adj.get(&node) {
                for (neighbor, _) in neighbors {
                    if !seen.contains_key(neighbor) {
                        seen.insert(neighbor.clone(), true);
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }

        let rows: Vec<Vec<Value>> = visited
            .into_iter()
            .enumerate()
            .map(|(step, node)| vec![Value::from(step as i64), node])
            .collect();

        Ok(build_rows(vec!["step".to_owned(), "node".to_owned()], rows))
    }
}

/// Random walk algorithm.
///
/// Random neighbor selection for N steps.
/// Output: `(step, node)` pairs.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "usize→i64: step count bounded by steps parameter; i64→u64: seed is non-negative"
)]
pub struct RandomWalk;

impl super::FixedRule for RandomWalk {
    fn name(&self) -> &str {
        "random_walk"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(2)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let start = require_option(options, "start", self.name())?.clone();
        let steps = i64_option(options, "steps", 10) as usize;
        let seed = i64_option(options, "seed", 0);

        let adj = build_adjacency(edges);
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed as u64);

        let mut path = vec![start.clone()];
        let mut current = start;

        for _ in 0..steps {
            if let Some(neighbors) = adj.get(&current) {
                if neighbors.is_empty() {
                    break;
                }
                // Weighted random selection
                let total_weight: f64 = neighbors.iter().map(|(_, w)| w).sum();
                let mut choice = rng.random::<f64>() * total_weight;

                let mut selected = &neighbors[0].0;
                for (node, weight) in neighbors {
                    choice -= *weight;
                    if choice <= 0.0 {
                        selected = node;
                        break;
                    }
                }

                current = selected.clone();
                path.push(current.clone());
            } else {
                break;
            }
        }

        let rows: Vec<Vec<Value>> = path
            .into_iter()
            .enumerate()
            .map(|(step, node)| vec![Value::from(step as i64), node])
            .collect();

        Ok(build_rows(vec!["step".to_owned(), "node".to_owned()], rows))
    }
}

use rand::SeedableRng;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::v2::algo::FixedRule;

    fn tree_edges() -> Vec<(Value, Value, f64)> {
        // Tree structure:
        //       a
        //     / | \
        //    b  c  d
        //   / \
        //  e   f
        vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("a"), Value::from("c"), 1.0),
            (Value::from("a"), Value::from("d"), 1.0),
            (Value::from("b"), Value::from("e"), 1.0),
            (Value::from("b"), Value::from("f"), 1.0),
        ]
    }

    #[test]
    fn dfs_traversal() {
        let edges = tree_edges();
        let mut options = BTreeMap::new();
        options.insert("start".to_owned(), Value::from("a"));

        let result = DfsTraversal.run(&edges, &options).unwrap();
        assert_eq!(result.headers, vec!["step", "node"]);
        assert_eq!(result.len(), 6);

        // First step should be start node
        assert_eq!(result.rows[0][1].as_str(), Some("a"));

        // All nodes visited exactly once
        let nodes: std::collections::HashSet<_> = result
            .rows
            .iter()
            .map(|r| r[1].clone())
            .collect();
        assert_eq!(nodes.len(), 6);
    }

    #[test]
    fn bfs_traversal() {
        let edges = tree_edges();
        let mut options = BTreeMap::new();
        options.insert("start".to_owned(), Value::from("a"));

        let result = BfsTraversal.run(&edges, &options).unwrap();
        assert_eq!(result.len(), 6);

        // BFS should visit all direct children of 'a' before grandchildren
        let get_step = |n: &str| {
            result
                .rows
                .iter()
                .find(|r| r[1].as_str() == Some(n))
                .map(|r| r[0].as_int().unwrap())
                .unwrap()
        };

        // b, c, d are depth 1, e and f are depth 2
        let step_b = get_step("b");
        let step_c = get_step("c");
        let step_d = get_step("d");
        let step_e = get_step("e");
        let step_f = get_step("f");

        // e and f should be after b, c, d (they're deeper)
        assert!(step_e > step_b);
        assert!(step_e > step_c);
        assert!(step_e > step_d);
        assert!(step_f > step_b);
        assert!(step_f > step_c);
        assert!(step_f > step_d);
    }

    #[test]
    fn random_walk_deterministic() {
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
            (Value::from("c"), Value::from("a"), 1.0),
        ];

        let mut options = BTreeMap::new();
        options.insert("start".to_owned(), Value::from("a"));
        options.insert("steps".to_owned(), Value::from(5_i64));
        options.insert("seed".to_owned(), Value::from(42_i64));

        let result1 = RandomWalk.run(&edges, &options).unwrap();
        let result2 = RandomWalk.run(&edges, &options).unwrap();

        // Same seed should produce same walk
        assert_eq!(result1.rows, result2.rows);
        assert_eq!(result1.len(), 6); // start + 5 steps
    }

    #[test]
    fn traversal_max_steps() {
        let edges = tree_edges();
        let mut options = BTreeMap::new();
        options.insert("start".to_owned(), Value::from("a"));
        options.insert("max_steps".to_owned(), Value::from(3_i64));

        let result = DfsTraversal.run(&edges, &options).unwrap();
        assert_eq!(result.len(), 3);

        let result = BfsTraversal.run(&edges, &options).unwrap();
        assert_eq!(result.len(), 3);
    }
}
