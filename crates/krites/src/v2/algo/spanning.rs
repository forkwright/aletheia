//! Spanning tree/forest algorithms: Prim MST, Kruskal MSF.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

use crate::v2::algo::{build_rows, collect_nodes};
use crate::v2::error::Result;
use crate::v2::rows::Rows;
use crate::v2::value::Value;

/// Edge with nodes and weight for MST algorithms.
#[derive(Debug, Clone)]
struct Edge {
    source: Value,
    target: Value,
    weight: f64,
}

impl PartialEq for Edge {
    fn eq(&self, other: &Self) -> bool {
        self.weight == other.weight
    }
}

impl Eq for Edge {}

impl Ord for Edge {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap (BinaryHeap is max-heap by default)
        other.weight.partial_cmp(&self.weight).unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for Edge {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Union-Find data structure for Kruskal's algorithm.
struct UnionFind {
    parent: FxHashMap<Value, Value>,
    rank: FxHashMap<Value, usize>,
}

impl UnionFind {
    fn new(nodes: &[Value]) -> Self {
        let mut parent = FxHashMap::default();
        let mut rank = FxHashMap::default();
        for node in nodes {
            parent.insert(node.clone(), node.clone());
            rank.insert(node.clone(), 0);
        }
        Self { parent, rank }
    }

    fn find(&mut self, x: &Value) -> Value {
        let parent = self.parent.get(x).cloned().unwrap_or_else(|| x.clone());
        if parent != *x {
            let root = self.find(&parent);
            self.parent.insert(x.clone(), root.clone());
            root
        } else {
            x.clone()
        }
    }

    fn union(&mut self, x: &Value, y: &Value) {
        let root_x = self.find(x);
        let root_y = self.find(y);

        if root_x == root_y {
            return;
        }

        let rank_x = *self.rank.get(&root_x).unwrap_or(&0);
        let rank_y = *self.rank.get(&root_y).unwrap_or(&0);

        if rank_x < rank_y {
            self.parent.insert(root_x, root_y);
        } else if rank_x > rank_y {
            self.parent.insert(root_y, root_x);
        } else {
            self.parent.insert(root_y, root_x.clone());
            self.rank.insert(root_x, rank_x + 1);
        }
    }

    fn connected(&mut self, x: &Value, y: &Value) -> bool {
        self.find(x) == self.find(y)
    }
}

/// Prim's Minimum Spanning Tree algorithm.
///
/// Finds the MST using Prim's algorithm starting from an arbitrary node.
/// Output: `(source, target, weight)` edges in the MST.
pub struct PrimMst;

impl super::FixedRule for PrimMst {
    fn name(&self) -> &str {
        "prim"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(3)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        _options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        // Filter out self-loops
        let edges: Vec<_> = edges.iter().filter(|(s, t, _)| s != t).cloned().collect();
        let nodes = collect_nodes(&edges);

        if nodes.is_empty() {
            return Ok(build_rows(
                vec!["source".to_owned(), "target".to_owned(), "weight".to_owned()],
                vec![],
            ));
        }

        // Build undirected adjacency with weights
        let mut adj: FxHashMap<Value, Vec<(Value, f64)>> = FxHashMap::default();
        for (s, t, w) in &edges {
            adj.entry(s.clone()).or_insert_with(Vec::new).push((t.clone(), *w));
            adj.entry(t.clone()).or_insert_with(Vec::new).push((s.clone(), *w));
        }

        // Prim's algorithm
        let mut mst_edges: Vec<Edge> = Vec::new();
        let mut in_mst: FxHashMap<Value, bool> = FxHashMap::default();
        let mut heap: std::collections::BinaryHeap<Edge> = std::collections::BinaryHeap::new();

        // Start from first node
        let start = &nodes[0];
        in_mst.insert(start.clone(), true);

        // Add edges from start node
        if let Some(neighbors) = adj.get(start) {
            for (target, weight) in neighbors {
                heap.push(Edge {
                    source: start.clone(),
                    target: target.clone(),
                    weight: *weight,
                });
            }
        }

        while let Some(edge) = heap.pop() {
            if in_mst.contains_key(&edge.target) {
                continue;
            }

            in_mst.insert(edge.target.clone(), true);
            mst_edges.push(edge.clone());

            // Add edges from newly added node
            if let Some(neighbors) = adj.get(&edge.target) {
                for (target, weight) in neighbors {
                    if !in_mst.contains_key(target) {
                        heap.push(Edge {
                            source: edge.target.clone(),
                            target: target.clone(),
                            weight: *weight,
                        });
                    }
                }
            }
        }

        // Build result
        let rows: Vec<Vec<Value>> = mst_edges
            .into_iter()
            .map(|e| vec![e.source, e.target, Value::from(e.weight)])
            .collect();

        Ok(build_rows(
            vec!["source".to_owned(), "target".to_owned(), "weight".to_owned()],
            rows,
        ))
    }
}

/// Kruskal's Minimum Spanning Forest algorithm.
///
/// Finds the MSF using Kruskal's algorithm with union-find.
/// Output: `(source, target, weight)` edges in the MSF.
pub struct KruskalMsf;

impl super::FixedRule for KruskalMsf {
    fn name(&self) -> &str {
        "kruskal"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(3)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        _options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let nodes = collect_nodes(edges);

        if nodes.is_empty() {
            return Ok(build_rows(
                vec!["source".to_owned(), "target".to_owned(), "weight".to_owned()],
                vec![],
            ));
        }

        // Sort edges by weight
        let mut sorted_edges: Vec<Edge> = edges
            .iter()
            .map(|(s, t, w)| Edge {
                source: s.clone(),
                target: t.clone(),
                weight: *w,
            })
            .collect();
        sorted_edges.sort_by(|a, b| a.weight.partial_cmp(&b.weight).unwrap_or(Ordering::Equal));

        // Kruskal's algorithm
        let mut uf = UnionFind::new(&nodes);
        let mut mst_edges: Vec<Edge> = Vec::new();

        for edge in sorted_edges {
            if !uf.connected(&edge.source, &edge.target) {
                uf.union(&edge.source, &edge.target);
                mst_edges.push(edge);
            }
        }

        // Build result
        let rows: Vec<Vec<Value>> = mst_edges
            .into_iter()
            .map(|e| vec![e.source, e.target, Value::from(e.weight)])
            .collect();

        Ok(build_rows(
            vec!["source".to_owned(), "target".to_owned(), "weight".to_owned()],
            rows,
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::v2::algo::FixedRule;

    fn triangle_edges() -> Vec<(Value, Value, f64)> {
        vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 2.0),
            (Value::from("a"), Value::from("c"), 3.0),
        ]
    }

    fn disconnected_edges() -> Vec<(Value, Value, f64)> {
        vec![
            // Component 1
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 2.0),
            // Component 2
            (Value::from("x"), Value::from("y"), 3.0),
        ]
    }

    #[test]
    fn prim_triangle() {
        let edges = triangle_edges();
        let result = PrimMst.run(&edges, &BTreeMap::new()).unwrap();

        // MST should have n-1 = 2 edges
        assert_eq!(result.len(), 2);

        // Total weight should be 1 + 2 = 3 (exclude the heaviest edge)
        let total_weight: f64 = result
            .rows
            .iter()
            .map(|r| r[2].as_float().unwrap())
            .sum();
        assert!((total_weight - 3.0).abs() < 1e-6);
    }

    #[test]
    fn kruskal_triangle() {
        let edges = triangle_edges();
        let result = KruskalMsf.run(&edges, &BTreeMap::new()).unwrap();

        assert_eq!(result.len(), 2);

        let total_weight: f64 = result
            .rows
            .iter()
            .map(|r| r[2].as_float().unwrap())
            .sum();
        assert!((total_weight - 3.0).abs() < 1e-6);
    }

    #[test]
    fn kruskal_disconnected() {
        // Kruskal should produce a spanning forest
        let edges = disconnected_edges();
        let result = KruskalMsf.run(&edges, &BTreeMap::new()).unwrap();

        // Should have (3 nodes - 1) + (2 nodes - 1) = 3 edges
        assert_eq!(result.len(), 3);

        // Check that edges from both components are present
        let sources: std::collections::HashSet<_> = result
            .rows
            .iter()
            .map(|r| r[0].as_str().unwrap().to_string())
            .collect();
        assert!(sources.contains("a") || sources.contains("b") || sources.contains("c"));
        assert!(sources.contains("x") || sources.contains("y"));
    }

    #[test]
    fn mst_empty() {
        let result = PrimMst.run(&[], &BTreeMap::new()).unwrap();
        assert!(result.is_empty());

        let result = KruskalMsf.run(&[], &BTreeMap::new()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn mst_single_node() {
        let edges = vec![(Value::from("a"), Value::from("a"), 1.0)];
        let result = PrimMst.run(&edges, &BTreeMap::new()).unwrap();
        // Self-loop should not be in MST
        assert!(result.is_empty());
    }
}
