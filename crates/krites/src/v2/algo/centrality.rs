//! Centrality algorithms: Degree, Closeness, Betweenness.

use std::collections::BTreeMap;
use std::collections::VecDeque;

use rustc_hash::FxHashMap;

use crate::v2::algo::{
    build_rows, build_undirected_adjacency, collect_nodes,
};
use crate::v2::error::Result;
use crate::v2::rows::Rows;
use crate::v2::value::Value;

/// Degree centrality algorithm.
///
/// Counts the number of edges incident to each node.
/// Output: `(node, centrality_score)` pairs.
pub struct DegreeCentrality;

impl super::FixedRule for DegreeCentrality {
    fn name(&self) -> &str {
        "degree"
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
        let adj = build_undirected_adjacency(edges);

        let rows: Vec<Vec<Value>> = nodes
            .into_iter()
            .map(|node| {
                let degree = adj.get(&node).map(|v| v.len()).unwrap_or(0) as f64;
                vec![node, Value::from(degree)]
            })
            .collect();

        Ok(build_rows(
            vec!["node".to_owned(), "centrality_score".to_owned()],
            rows,
        ))
    }
}

/// Closeness centrality algorithm.
///
/// Measures how close a node is to all other nodes (inverse of average shortest path).
/// Output: `(node, centrality_score)` pairs.
pub struct ClosenessCentrality;

impl super::FixedRule for ClosenessCentrality {
    fn name(&self) -> &str {
        "closeness"
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
        let adj = build_undirected_adjacency(edges);
        let n = nodes.len();

        let rows: Vec<Vec<Value>> = nodes
            .into_iter()
            .map(|node| {
                let sum_dist = bfs_sum_distances(&adj, &node, n);
                let centrality = if sum_dist > 0.0 && n > 1 {
                    (n - 1) as f64 / sum_dist
                } else {
                    0.0
                };
                vec![node, Value::from(centrality)]
            })
            .collect();

        Ok(build_rows(
            vec!["node".to_owned(), "centrality_score".to_owned()],
            rows,
        ))
    }
}

/// Betweenness centrality algorithm (Brandes' algorithm).
///
/// Measures the fraction of shortest paths that pass through each node.
/// Output: `(node, centrality_score)` pairs.
pub struct BetweennessCentrality;

impl super::FixedRule for BetweennessCentrality {
    fn name(&self) -> &str {
        "betweenness"
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
        let adj = build_undirected_adjacency(edges);
        let n = nodes.len();

        // Brandes' algorithm
        let mut centrality: FxHashMap<Value, f64> =
            nodes.iter().map(|n| (n.clone(), 0.0)).collect();

        for s in &nodes {
            // Single-source shortest paths
            let mut dist: FxHashMap<Value, f64> = FxHashMap::default();
            let mut pred: FxHashMap<Value, Vec<Value>> = FxHashMap::default();
            let mut sigma: FxHashMap<Value, f64> = FxHashMap::default();
            let mut queue: VecDeque<Value> = VecDeque::new();
            let mut stack: Vec<Value> = Vec::new();

            dist.insert(s.clone(), 0.0);
            sigma.insert(s.clone(), 1.0);
            queue.push_back(s.clone());

            // BFS
            while let Some(v) = queue.pop_front() {
                stack.push(v.clone());
                if let Some(neighbors) = adj.get(&v) {
                    for (w, _) in neighbors {
                        if !dist.contains_key(w) {
                            dist.insert(w.clone(), dist[&v] + 1.0);
                            queue.push_back(w.clone());
                        }
                        if dist[w] == dist[&v] + 1.0 {
                            *sigma.entry(w.clone()).or_insert(0.0) += sigma[&v];
                            pred.entry(w.clone()).or_insert_with(Vec::new).push(v.clone());
                        }
                    }
                }
            }

            // Accumulation
            let mut delta: FxHashMap<Value, f64> = FxHashMap::default();
            while let Some(w) = stack.pop() {
                if let Some(preds) = pred.get(&w) {
                    for v in preds {
                        let contribution = (sigma[v] / sigma[&w]) * (1.0 + delta.get(&w).unwrap_or(&0.0));
                        *delta.entry(v.clone()).or_insert(0.0) += contribution;
                    }
                }
                if w != *s {
                    if let Some(c) = centrality.get_mut(&w) {
                        *c += delta.get(&w).unwrap_or(&0.0);
                    }
                }
            }
        }

        // Normalize for undirected graphs
        let scale = if n > 2 { 2.0 / ((n - 1) * (n - 2)) as f64 } else { 1.0 };
        for v in centrality.values_mut() {
            *v *= scale;
        }

        let rows: Vec<Vec<Value>> = centrality
            .into_iter()
            .map(|(node, score)| vec![node, Value::from(score)])
            .collect();

        Ok(build_rows(
            vec!["node".to_owned(), "centrality_score".to_owned()],
            rows,
        ))
    }
}

/// BFS to compute sum of distances from source.
fn bfs_sum_distances(
    adj: &FxHashMap<Value, Vec<(Value, f64)>>,
    source: &Value,
    _n: usize,
) -> f64 {
    let mut dist: FxHashMap<Value, f64> = FxHashMap::default();
    let mut queue: VecDeque<Value> = VecDeque::new();

    dist.insert(source.clone(), 0.0);
    queue.push_back(source.clone());

    while let Some(v) = queue.pop_front() {
        let d = dist[&v];
        if let Some(neighbors) = adj.get(&v) {
            for (w, _) in neighbors {
                if !dist.contains_key(w) {
                    dist.insert(w.clone(), d + 1.0);
                    queue.push_back(w.clone());
                }
            }
        }
    }

    dist.values().sum()
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
    fn degree_star_graph() {
        // Star graph with center connected to 4 leaves
        let edges = vec![
            (Value::from("center"), Value::from("a"), 1.0),
            (Value::from("center"), Value::from("b"), 1.0),
            (Value::from("center"), Value::from("c"), 1.0),
            (Value::from("center"), Value::from("d"), 1.0),
        ];

        let result = DegreeCentrality.run(&edges, &BTreeMap::new()).unwrap();
        assert_eq!(result.headers, vec!["node", "centrality_score"]);
        assert_eq!(result.len(), 5);

        // Center should have degree 4
        let center_row = result
            .rows
            .iter()
            .find(|r| r[0].as_str() == Some("center"))
            .unwrap();
        assert_eq!(center_row[1].as_float(), Some(4.0));

        // Leaves should have degree 1
        let leaf_row = result
            .rows
            .iter()
            .find(|r| r[0].as_str() == Some("a"))
            .unwrap();
        assert_eq!(leaf_row[1].as_float(), Some(1.0));
    }

    #[test]
    fn closeness_line_graph() {
        // Line graph: a - b - c - d
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
            (Value::from("c"), Value::from("d"), 1.0),
        ];

        let result = ClosenessCentrality.run(&edges, &BTreeMap::new()).unwrap();

        // Center nodes (b, c) should have higher closeness than endpoints (a, d)
        let get_score = |n: &str| {
            result
                .rows
                .iter()
                .find(|r| r[0].as_str() == Some(n))
                .map(|r| r[1].as_float().unwrap())
                .unwrap()
        };

        let score_a = get_score("a");
        let score_b = get_score("b");
        let score_c = get_score("c");
        let score_d = get_score("d");

        // b and c are more central than a and d
        assert!(score_b > score_a);
        assert!(score_c > score_d);
        // Symmetry: a == d, b == c
        assert!((score_a - score_d).abs() < 1e-6);
        assert!((score_b - score_c).abs() < 1e-6);
    }

    #[test]
    fn betweenness_bridge_node() {
        // Bridge node: a - b - c, where b connects two components
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
        ];

        let result = BetweennessCentrality.run(&edges, &BTreeMap::new()).unwrap();

        let get_score = |n: &str| {
            result
                .rows
                .iter()
                .find(|r| r[0].as_str() == Some(n))
                .map(|r| r[1].as_float().unwrap())
                .unwrap()
        };

        // Bridge node b should have highest betweenness
        let score_a = get_score("a");
        let score_b = get_score("b");
        let score_c = get_score("c");

        assert!(score_b > score_a);
        assert!(score_b > score_c);
        // Endpoints have zero betweenness (no paths through them)
        assert!(score_a < 1e-6);
        assert!(score_c < 1e-6);
    }

    #[test]
    fn centrality_empty() {
        let result = DegreeCentrality.run(&[], &BTreeMap::new()).unwrap();
        assert!(result.is_empty());
    }
}
