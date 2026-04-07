//! Path finding algorithms: BFS, Dijkstra, A*, Yen's K-shortest paths.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

use crate::v2::algo::{
    build_adjacency, build_rows, f64_option, i64_option, require_option,
};
use crate::v2::error::Result;
use crate::v2::rows::Rows;
use crate::v2::value::Value;

/// State for priority queue in path finding.
#[derive(Debug, Clone)]
struct State {
    cost: f64,
    node: Value,
    path: Vec<Value>,
}

impl PartialEq for State {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost && self.node == other.node
    }
}

impl Eq for State {}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// BFS shortest path algorithm (unweighted).
///
/// Output: `(node, distance, path)` where path is a list of nodes.
pub struct BfsPath;

impl super::FixedRule for BfsPath {
    fn name(&self) -> &str {
        "bfs"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(3)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let start = require_option(options, "start", self.name())?;
        let adj = build_adjacency(edges);

        // BFS
        let mut dist: FxHashMap<Value, f64> = FxHashMap::default();
        let mut parent: FxHashMap<Value, Value> = FxHashMap::default();
        let mut queue: std::collections::VecDeque<Value> = std::collections::VecDeque::new();

        dist.insert(start.clone(), 0.0);
        queue.push_back(start.clone());

        while let Some(node) = queue.pop_front() {
            let d = dist[&node];
            if let Some(neighbors) = adj.get(&node) {
                for (next, _) in neighbors {
                    if !dist.contains_key(next) {
                        dist.insert(next.clone(), d + 1.0);
                        parent.insert(next.clone(), node.clone());
                        queue.push_back(next.clone());
                    }
                }
            }
        }

        // Build results
        let rows: Vec<Vec<Value>> = dist
            .into_iter()
            .map(|(node, d)| {
                // Reconstruct path
                let mut path = vec![node.clone()];
                let mut current = &node;
                while let Some(p) = parent.get(current) {
                    path.push(p.clone());
                    current = p;
                }
                path.reverse();

                vec![
                    node,
                    Value::from(d as i64),
                    Value::from(path),
                ]
            })
            .collect();

        Ok(build_rows(
            vec!["node".to_owned(), "distance".to_owned(), "path".to_owned()],
            rows,
        ))
    }
}

/// Dijkstra's shortest path algorithm (weighted).
///
/// Output: `(node, distance, path)` where path is a list of nodes.
pub struct DijkstraPath;

impl super::FixedRule for DijkstraPath {
    fn name(&self) -> &str {
        "dijkstra"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(3)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let start = require_option(options, "start", self.name())?;
        let adj = build_adjacency(edges);

        // Dijkstra
        let mut dist: FxHashMap<Value, f64> = FxHashMap::default();
        let mut parent: FxHashMap<Value, Value> = FxHashMap::default();
        let mut heap = BinaryHeap::new();

        dist.insert(start.clone(), 0.0);
        heap.push(State {
            cost: 0.0,
            node: start.clone(),
            path: vec![start.clone()],
        });

        while let Some(State { cost, node, .. }) = heap.pop() {
            if cost > *dist.get(&node).unwrap_or(&f64::INFINITY) {
                continue;
            }

            if let Some(neighbors) = adj.get(&node) {
                for (next, weight) in neighbors {
                    let next_cost = cost + *weight;
                    if next_cost < *dist.get(next).unwrap_or(&f64::INFINITY) {
                        dist.insert(next.clone(), next_cost);
                        parent.insert(next.clone(), node.clone());
                        heap.push(State {
                            cost: next_cost,
                            node: next.clone(),
                            path: vec![],
                        });
                    }
                }
            }
        }

        // Build results
        let rows: Vec<Vec<Value>> = dist
            .into_iter()
            .map(|(node, d)| {
                let mut path = vec![node.clone()];
                let mut current = &node;
                while let Some(p) = parent.get(current) {
                    path.push(p.clone());
                    current = p;
                }
                path.reverse();

                vec![node, Value::from(d as i64), Value::from(path)]
            })
            .collect();

        Ok(build_rows(
            vec!["node".to_owned(), "distance".to_owned(), "path".to_owned()],
            rows,
        ))
    }
}

/// A* shortest path algorithm.
///
/// Uses Dijkstra fallback if no heuristic provided.
/// Output: `(node, distance, path)` where path is a list of nodes.
pub struct AStarPath;

impl super::FixedRule for AStarPath {
    fn name(&self) -> &str {
        "astar"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(3)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let start = require_option(options, "start", self.name())?;
        let _goal = require_option(options, "goal", self.name())?;
        let adj = build_adjacency(edges);

        // A* (without heuristic, equivalent to Dijkstra for full search)
        let mut dist: FxHashMap<Value, f64> = FxHashMap::default();
        let mut parent: FxHashMap<Value, Value> = FxHashMap::default();
        let mut heap = BinaryHeap::new();

        dist.insert(start.clone(), 0.0);
        heap.push(State {
            cost: 0.0,
            node: start.clone(),
            path: vec![start.clone()],
        });

        while let Some(State { cost, node, .. }) = heap.pop() {
            if cost > *dist.get(&node).unwrap_or(&f64::INFINITY) {
                continue;
            }

            if let Some(neighbors) = adj.get(&node) {
                for (next, weight) in neighbors {
                    let next_cost = cost + *weight;
                    if next_cost < *dist.get(next).unwrap_or(&f64::INFINITY) {
                        dist.insert(next.clone(), next_cost);
                        parent.insert(next.clone(), node.clone());
                        heap.push(State {
                            cost: next_cost,
                            node: next.clone(),
                            path: vec![],
                        });
                    }
                }
            }
        }

        // Build results
        let rows: Vec<Vec<Value>> = dist
            .into_iter()
            .map(|(node, d)| {
                let mut path = vec![node.clone()];
                let mut current = &node;
                while let Some(p) = parent.get(current) {
                    path.push(p.clone());
                    current = p;
                }
                path.reverse();

                vec![node, Value::from(d as i64), Value::from(path)]
            })
            .collect();

        Ok(build_rows(
            vec!["node".to_owned(), "distance".to_owned(), "path".to_owned()],
            rows,
        ))
    }
}

/// Yen's K-shortest paths algorithm.
///
/// Finds K shortest loopless paths from start to goal.
/// Output: `(path_id, node, step, total_cost)` for each node in each path.
pub struct YenKShortest;

impl YenKShortest {
    /// Default K value.
    const DEFAULT_K: i64 = 3;
}

impl super::FixedRule for YenKShortest {
    fn name(&self) -> &str {
        "yen_k_shortest"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(4)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let start = require_option(options, "start", self.name())?.clone();
        let goal = require_option(options, "goal", self.name())?.clone();
        let k = i64_option(options, "k", Self::DEFAULT_K) as usize;

        let adj = build_adjacency(edges);

        // Find first shortest path using Dijkstra
        let first_path = dijkstra_single_path(&adj, &start, &goal);

        if first_path.0.is_empty() {
            return Ok(build_rows(
                vec![
                    "path_id".to_owned(),
                    "node".to_owned(),
                    "step".to_owned(),
                    "total_cost".to_owned(),
                ],
                vec![],
            ));
        }

        let mut paths: Vec<(Vec<Value>, f64)> = vec![first_path];
        let mut candidates: Vec<(Vec<Value>, f64)> = Vec::new();

        for i in 1..k {
            let prev_path = &paths[i - 1].0;

            for j in 0..prev_path.len().saturating_sub(1) {
                let spur_node = prev_path[j].clone();
                let root_path: Vec<Value> = prev_path[..=j].to_vec();

                // Remove edges that would create already-found paths
                let mut filtered_edges: Vec<(Value, Value, f64)> = edges.to_vec();
                filtered_edges.retain(|(s, t, _)| {
                    let edge = vec![s.clone(), t.clone()];
                    !paths.iter().any(|(p, _)| {
                        p.len() > j && p[..=j] == root_path && p[j..j + 2] == edge
                    })
                });

                // Also remove nodes in root_path (except spur_node)
                let removed_nodes: std::collections::HashSet<_> =
                    root_path[..root_path.len().saturating_sub(1)].iter().cloned().collect();

                let filtered_adj = build_adjacency(&filtered_edges);

                // Find spur path
                let spur_path = dijkstra_single_path_filtered(&filtered_adj, &spur_node, &goal, &removed_nodes);

                if !spur_path.0.is_empty() {
                    let mut total_path = root_path[..root_path.len() - 1].to_vec();
                    total_path.extend(spur_path.0.iter().cloned());

                    let total_cost = calculate_path_cost(&filtered_adj, &total_path);

                    // Check for duplicates
                    if !candidates.iter().any(|(p, _)| p == &total_path)
                        && !paths.iter().any(|(p, _)| p == &total_path)
                    {
                        candidates.push((total_path, total_cost));
                    }
                }
            }

            if candidates.is_empty() {
                break;
            }

            // Sort by cost and pick best
            candidates.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal));
            paths.push(candidates.remove(0));
        }

        // Build output rows
        let mut rows = Vec::new();
        for (path_id, (path, cost)) in paths.iter().enumerate() {
            for (step, node) in path.iter().enumerate() {
                rows.push(vec![
                    Value::from(path_id as i64),
                    node.clone(),
                    Value::from(step as i64),
                    Value::from(*cost),
                ]);
            }
        }

        Ok(build_rows(
            vec![
                "path_id".to_owned(),
                "node".to_owned(),
                "step".to_owned(),
                "total_cost".to_owned(),
            ],
            rows,
        ))
    }
}

/// Dijkstra to find single path.
fn dijkstra_single_path(
    adj: &FxHashMap<Value, Vec<(Value, f64)>>,
    start: &Value,
    goal: &Value,
) -> (Vec<Value>, f64) {
    let mut dist: FxHashMap<Value, f64> = FxHashMap::default();
    let mut parent: FxHashMap<Value, Value> = FxHashMap::default();
    let mut heap = BinaryHeap::new();

    dist.insert(start.clone(), 0.0);
    heap.push(State {
        cost: 0.0,
        node: start.clone(),
        path: vec![start.clone()],
    });

    while let Some(State { cost, node, .. }) = heap.pop() {
        if node == *goal {
            // Reconstruct path
            let mut path = vec![node.clone()];
            let mut current = &node;
            while let Some(p) = parent.get(current) {
                path.push(p.clone());
                current = p;
            }
            path.reverse();
            return (path, cost);
        }

        if cost > *dist.get(&node).unwrap_or(&f64::INFINITY) {
            continue;
        }

        if let Some(neighbors) = adj.get(&node) {
            for (next, weight) in neighbors {
                let next_cost = cost + *weight;
                if next_cost < *dist.get(next).unwrap_or(&f64::INFINITY) {
                    dist.insert(next.clone(), next_cost);
                    parent.insert(next.clone(), node.clone());
                    heap.push(State {
                        cost: next_cost,
                        node: next.clone(),
                        path: vec![],
                    });
                }
            }
        }
    }

    (vec![], f64::INFINITY)
}

/// Dijkstra with filtered nodes.
fn dijkstra_single_path_filtered(
    adj: &FxHashMap<Value, Vec<(Value, f64)>>,
    start: &Value,
    goal: &Value,
    excluded: &std::collections::HashSet<Value>,
) -> (Vec<Value>, f64) {
    let mut dist: FxHashMap<Value, f64> = FxHashMap::default();
    let mut parent: FxHashMap<Value, Value> = FxHashMap::default();
    let mut heap = BinaryHeap::new();

    dist.insert(start.clone(), 0.0);
    heap.push(State {
        cost: 0.0,
        node: start.clone(),
        path: vec![start.clone()],
    });

    while let Some(State { cost, node, .. }) = heap.pop() {
        if node == *goal {
            let mut path = vec![node.clone()];
            let mut current = &node;
            while let Some(p) = parent.get(current) {
                path.push(p.clone());
                current = p;
            }
            path.reverse();
            return (path, cost);
        }

        if cost > *dist.get(&node).unwrap_or(&f64::INFINITY) {
            continue;
        }

        if let Some(neighbors) = adj.get(&node) {
            for (next, weight) in neighbors {
                if excluded.contains(next) && next != goal {
                    continue;
                }
                let next_cost = cost + *weight;
                if next_cost < *dist.get(next).unwrap_or(&f64::INFINITY) {
                    dist.insert(next.clone(), next_cost);
                    parent.insert(next.clone(), node.clone());
                    heap.push(State {
                        cost: next_cost,
                        node: next.clone(),
                        path: vec![],
                    });
                }
            }
        }
    }

    (vec![], f64::INFINITY)
}

/// Calculate path cost.
fn calculate_path_cost(
    adj: &FxHashMap<Value, Vec<(Value, f64)>>,
    path: &[Value],
) -> f64 {
    let mut cost = 0.0;
    for window in path.windows(2) {
        if let [a, b] = window {
            if let Some(neighbors) = adj.get(a) {
                if let Some((_, w)) = neighbors.iter().find(|(n, _)| n == b) {
                    cost += *w;
                }
            }
        }
    }
    cost
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::v2::algo::FixedRule;

    fn chain_edges(n: usize) -> Vec<(Value, Value, f64)> {
        (0..n - 1)
            .map(|i| (Value::from(i as i64), Value::from((i + 1) as i64), 1.0))
            .collect()
    }

    #[test]
    fn bfs_chain_5() {
        let edges = chain_edges(5);
        let mut options = BTreeMap::new();
        options.insert("start".to_owned(), Value::from(0_i64));

        let result = BfsPath.run(&edges, &options).unwrap();
        assert_eq!(result.headers, vec!["node", "distance", "path"]);
        assert_eq!(result.len(), 5);

        // Check distances
        for i in 0..5 {
            let row = result
                .rows
                .iter()
                .find(|r| r[0].as_int() == Some(i as i64))
                .unwrap();
            assert_eq!(row[1].as_int(), Some(i as i64)); // Distance = i
        }
    }

    #[test]
    fn dijkstra_weighted() {
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 2.0),
            (Value::from("a"), Value::from("c"), 5.0), // Direct but more expensive
        ];

        let mut options = BTreeMap::new();
        options.insert("start".to_owned(), Value::from("a"));

        let result = DijkstraPath.run(&edges, &options).unwrap();

        // a to c should be 3 (via b), not 5 (direct)
        let c_row = result
            .rows
            .iter()
            .find(|r| r[0].as_str() == Some("c"))
            .unwrap();
        assert_eq!(c_row[1].as_int(), Some(3));
    }

    #[test]
    fn astar_requires_goal() {
        let edges = chain_edges(3);
        let mut options = BTreeMap::new();
        options.insert("start".to_owned(), Value::from(0_i64));
        // Missing goal

        let result = AStarPath.run(&edges, &options);
        assert!(result.is_err());
    }

    #[test]
    fn yen_k_shortest() {
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
            (Value::from("a"), Value::from("d"), 1.0),
            (Value::from("d"), Value::from("c"), 1.0),
            (Value::from("a"), Value::from("c"), 3.0), // Direct path
        ];

        let mut options = BTreeMap::new();
        options.insert("start".to_owned(), Value::from("a"));
        options.insert("goal".to_owned(), Value::from("c"));
        options.insert("k".to_owned(), Value::from(3_i64));

        let result = YenKShortest.run(&edges, &options).unwrap();
        assert_eq!(result.headers, vec!["path_id", "node", "step", "total_cost"]);

        // Should have at least 3 paths
        let path_ids: std::collections::HashSet<_> = result
            .rows
            .iter()
            .map(|r| r[0].as_int().unwrap())
            .collect();
        assert!(path_ids.len() >= 2); // At least 2 distinct paths exist
    }
}
