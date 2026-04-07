//! Clustering algorithms: KCore, Clustering Coefficients, TopSort.

use std::collections::BTreeMap;
use std::collections::VecDeque;

use rustc_hash::FxHashMap;

use crate::v2::algo::{
    build_adjacency, build_undirected_adjacency, build_rows, collect_nodes,
};
use crate::v2::error::Result;
use crate::v2::rows::Rows;
use crate::v2::value::Value;

/// K-Core decomposition algorithm.
///
/// Finds the maximal subgraph where each node has degree at least k.
/// Output: `(node, core_number)` pairs.
#[expect(
    clippy::cast_possible_truncation,
    reason = "usize→i64: core numbers bounded by node count which is < i64::MAX"
)]
pub struct KCore;

impl super::FixedRule for KCore {
    fn name(&self) -> &str {
        "kcore"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(2)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        _options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let nodes = collect_nodes(edges);

        if nodes.is_empty() {
            return Ok(build_rows(
                vec!["node".to_owned(), "core_number".to_owned()],
                vec![],
            ));
        }

        // Build undirected adjacency
        let adj = build_undirected_adjacency(edges);

        // Compute degrees
        let mut degree: FxHashMap<Value, usize> = nodes
            .iter()
            .map(|n| {
                let d = adj.get(n).map(|v| v.len()).unwrap_or(0);
                (n.clone(), d)
            })
            .collect();

        // Bucket sort by degree
        let max_degree = degree.values().copied().max().unwrap_or(0);
        let mut buckets: Vec<Vec<Value>> = vec![Vec::new(); max_degree + 1];
        let mut node_pos: FxHashMap<Value, usize> = FxHashMap::default();

        for (node, d) in &degree {
            buckets[*d].push(node.clone());
            node_pos.insert(node.clone(), buckets[*d].len() - 1);
        }

        // Process nodes in order of degree
        let mut core: FxHashMap<Value, usize> = FxHashMap::default();
        let mut processed: FxHashMap<Value, bool> = FxHashMap::default();

        for i in 0..=max_degree {
            while let Some(node) = buckets[i].pop() {
                if processed.contains_key(&node) {
                    continue;
                }

                processed.insert(node.clone(), true);
                core.insert(node.clone(), i);

                // Decrease degree of neighbors
                if let Some(neighbors) = adj.get(&node) {
                    for (neighbor, _) in neighbors {
                        if !processed.contains_key(neighbor) {
                            let old_deg = degree[neighbor];
                            if old_deg > i {
                                let new_deg = old_deg - 1;
                                degree.insert(neighbor.clone(), new_deg);
                                buckets[new_deg].push(neighbor.clone());
                            }
                        }
                    }
                }
            }
        }

        // Build result
        let rows: Vec<Vec<Value>> = nodes
            .into_iter()
            .map(|node| {
                let c = core.get(&node).copied().unwrap_or(0) as i64;
                vec![node, Value::from(c)]
            })
            .collect();

        Ok(build_rows(
            vec!["node".to_owned(), "core_number".to_owned()],
            rows,
        ))
    }
}

/// Clustering Coefficients algorithm.
///
/// Computes the local clustering coefficient for each node.
/// Output: `(node, clustering_coeff)` pairs.
#[expect(
    clippy::cast_precision_loss,
    reason = "usize→f64: triangle counts bounded by graph size, precision loss acceptable"
)]
pub struct ClusteringCoefficients;

impl super::FixedRule for ClusteringCoefficients {
    fn name(&self) -> &str {
        "clustering_coeff"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(2)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        _options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let nodes = collect_nodes(edges);

        if nodes.is_empty() {
            return Ok(build_rows(
                vec!["node".to_owned(), "clustering_coeff".to_owned()],
                vec![],
            ));
        }

        // Build undirected adjacency as sets for fast lookup
        let mut adj: FxHashMap<Value, std::collections::HashSet<Value>> = FxHashMap::default();
        for (s, t, _w) in edges {
            adj.entry(s.clone())
                .or_insert_with(std::collections::HashSet::new)
                .insert(t.clone());
            adj.entry(t.clone())
                .or_insert_with(std::collections::HashSet::new)
                .insert(s.clone());
        }

        // Compute clustering coefficient for each node
        let rows: Vec<Vec<Value>> = nodes
            .into_iter()
            .map(|node| {
                let neighbors = adj.get(&node).cloned().unwrap_or_default();
                let k = neighbors.len();

                if k < 2 {
                    return vec![node, Value::from(0.0)];
                }

                // Count triangles
                let mut triangles = 0;
                let neighbor_vec: Vec<_> = neighbors.iter().collect();
                for i in 0..neighbor_vec.len() {
                    for j in (i + 1)..neighbor_vec.len() {
                        let u = neighbor_vec[i];
                        let v = neighbor_vec[j];
                        if adj.get(u).map(|s| s.contains(v)).unwrap_or(false) {
                            triangles += 1;
                        }
                    }
                }

                let max_edges = k * (k - 1) / 2;
                let coeff = if max_edges > 0 {
                    triangles as f64 / max_edges as f64
                } else {
                    0.0
                };

                vec![node, Value::from(coeff)]
            })
            .collect();

        Ok(build_rows(
            vec!["node".to_owned(), "clustering_coeff".to_owned()],
            rows,
        ))
    }
}

/// Topological Sort algorithm (Kahn's algorithm).
///
/// Linear ordering of nodes in a DAG such that for every edge (u,v),
/// u comes before v.
/// Output: `(node, position)` pairs.
#[expect(
    clippy::cast_possible_truncation,
    reason = "usize→i64: position bounded by node count which is < i64::MAX"
)]
pub struct TopSort;

impl super::FixedRule for TopSort {
    fn name(&self) -> &str {
        "topsort"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(2)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        _options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let nodes = collect_nodes(edges);

        if nodes.is_empty() {
            return Ok(build_rows(
                vec!["node".to_owned(), "position".to_owned()],
                vec![],
            ));
        }

        // Build adjacency and compute in-degrees
        let adj = build_adjacency(edges);
        let mut in_degree: FxHashMap<Value, usize> = nodes
            .iter()
            .map(|n| (n.clone(), 0))
            .collect();

        for (_, targets) in &adj {
            for (target, _) in targets {
                *in_degree.entry(target.clone()).or_insert(0) += 1;
            }
        }

        // Kahn's algorithm
        let mut queue: VecDeque<Value> = nodes
            .iter()
            .filter(|n| in_degree.get(*n).copied().unwrap_or(0) == 0)
            .cloned()
            .collect();

        let mut sorted: Vec<Value> = Vec::new();

        while let Some(node) = queue.pop_front() {
            sorted.push(node.clone());

            if let Some(neighbors) = adj.get(&node) {
                for (neighbor, _) in neighbors {
                    if let Some(deg) = in_degree.get_mut(neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor.clone());
                        }
                    }
                }
            }
        }

        // Check for cycle
        if sorted.len() != nodes.len() {
            // Graph has a cycle, return empty or partial
            // Return what we have with -1 for remaining
        }

        // Build result
        let sorted_set: std::collections::HashSet<_> = sorted.iter().cloned().collect();
        let mut rows: Vec<Vec<Value>> = sorted
            .into_iter()
            .enumerate()
            .map(|(pos, node)| vec![node, Value::from(pos as i64)])
            .collect();

        // Add remaining nodes (in cycle) with position -1
        for node in nodes {
            if !sorted_set.contains(&node) {
                rows.push(vec![node, Value::from(-1_i64)]);
            }
        }

        Ok(build_rows(
            vec!["node".to_owned(), "position".to_owned()],
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

    #[test]
    fn kcore_triangle() {
        // Triangle: all nodes have degree 2, so core number is 2
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
            (Value::from("c"), Value::from("a"), 1.0),
        ];

        let result = KCore.run(&edges, &BTreeMap::new()).unwrap();
        assert_eq!(result.headers, vec!["node", "core_number"]);
        assert_eq!(result.len(), 3);

        // All nodes should have core number 2
        for row in &result.rows {
            assert_eq!(row[1].as_int(), Some(2));
        }
    }

    #[test]
    fn kcore_kite() {
        // Kite graph:
        //   a -- b
        //   |\  |
        //   | \ |
        //   c -- d
        //   |
        //   e
        // Core: a,b,c,d = 2 (form a 4-clique minus one edge, actually a 4-cycle with diagonal)
        // Actually let's do simpler:
        // Square with diagonal (a-b-c-d-a) + diagonal a-c
        // a,b,c,d form a structure where min degree is 2, so core >= 2
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
            (Value::from("c"), Value::from("d"), 1.0),
            (Value::from("d"), Value::from("a"), 1.0),
            (Value::from("a"), Value::from("c"), 1.0),
            // e is a pendant node attached to a
            (Value::from("a"), Value::from("e"), 1.0),
        ];

        let result = KCore.run(&edges, &BTreeMap::new()).unwrap();

        let get_core = |n: &str| {
            result
                .rows
                .iter()
                .find(|r| r[0].as_str() == Some(n))
                .map(|r| r[1].as_int().unwrap())
                .unwrap()
        };

        // a,b,c,d should have core >= 2 (they form 2-connected structure)
        assert!(get_core("a") >= 2);
        assert!(get_core("b") >= 2);
        assert!(get_core("c") >= 2);
        assert!(get_core("d") >= 2);

        // e is a leaf, core number 1
        assert_eq!(get_core("e"), 1);
    }

    #[test]
    fn clustering_coeff_triangle() {
        // Triangle: each node has 2 neighbors that are connected
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
            (Value::from("c"), Value::from("a"), 1.0),
        ];

        let result = ClusteringCoefficients.run(&edges, &BTreeMap::new()).unwrap();
        assert_eq!(result.len(), 3);

        // All nodes have clustering coefficient 1.0
        for row in &result.rows {
            assert!((row[1].as_float().unwrap() - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn clustering_coeff_star() {
        // Star graph: center with 4 leaves
        let edges = vec![
            (Value::from("center"), Value::from("a"), 1.0),
            (Value::from("center"), Value::from("b"), 1.0),
            (Value::from("center"), Value::from("c"), 1.0),
            (Value::from("center"), Value::from("d"), 1.0),
        ];

        let result = ClusteringCoefficients.run(&edges, &BTreeMap::new()).unwrap();

        let get_coeff = |n: &str| {
            result
                .rows
                .iter()
                .find(|r| r[0].as_str() == Some(n))
                .map(|r| r[1].as_float().unwrap())
                .unwrap()
        };

        // Center has 4 neighbors, none connected to each other -> coeff = 0
        assert!(get_coeff("center") < 1e-6);

        // Leaves have degree 1 -> coeff = 0
        assert!(get_coeff("a") < 1e-6);
    }

    #[test]
    fn topsort_dag() {
        // DAG: a -> b -> c, a -> c
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
            (Value::from("a"), Value::from("c"), 1.0),
        ];

        let result = TopSort.run(&edges, &BTreeMap::new()).unwrap();
        assert_eq!(result.len(), 3);

        let get_pos = |n: &str| {
            result
                .rows
                .iter()
                .find(|r| r[0].as_str() == Some(n))
                .map(|r| r[1].as_int().unwrap())
                .unwrap()
        };

        // a should come before b and c
        assert!(get_pos("a") < get_pos("b"));
        assert!(get_pos("a") < get_pos("c"));
        // b should come before c
        assert!(get_pos("b") < get_pos("c"));
    }

    #[test]
    fn topsort_cycle() {
        // Cycle: a -> b -> c -> a
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
            (Value::from("c"), Value::from("a"), 1.0),
        ];

        let result = TopSort.run(&edges, &BTreeMap::new()).unwrap();

        // All nodes should have position -1 (indicating cycle)
        for row in &result.rows {
            assert_eq!(row[1].as_int(), Some(-1));
        }
    }

    #[test]
    fn clustering_empty() {
        let result = KCore.run(&[], &BTreeMap::new()).unwrap();
        assert!(result.is_empty());

        let result = ClusteringCoefficients.run(&[], &BTreeMap::new()).unwrap();
        assert!(result.is_empty());

        let result = TopSort.run(&[], &BTreeMap::new()).unwrap();
        assert!(result.is_empty());
    }
}
