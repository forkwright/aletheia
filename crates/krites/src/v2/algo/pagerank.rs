//! PageRank algorithm implementation.

use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

use crate::v2::algo::{build_rows, collect_nodes, f64_option};
use crate::v2::error::Result;
use crate::v2::rows::Rows;
use crate::v2::value::Value;

/// PageRank algorithm.
///
/// Computes the importance of nodes in a graph based on the link structure.
/// Output: `(node, rank)` pairs.
pub struct PageRank;

impl PageRank {
    /// Default damping factor (probability of following a link vs. random jump).
    const DEFAULT_DAMPING: f64 = 0.85;
    /// Default maximum iterations.
    const DEFAULT_ITERATIONS: i64 = 100;
    /// Default convergence epsilon.
    const DEFAULT_EPSILON: f64 = 1e-6;
}

impl super::FixedRule for PageRank {
    fn name(&self) -> &str {
        "pagerank"
    }

    fn arity(&self, _options: &BTreeMap<String, Value>) -> Result<usize> {
        Ok(2)
    }

    fn run(
        &self,
        edges: &[(Value, Value, f64)],
        options: &BTreeMap<String, Value>,
    ) -> Result<Rows> {
        let damping = f64_option(options, "damping", Self::DEFAULT_DAMPING);
        let max_iter = f64_option(options, "iterations", Self::DEFAULT_ITERATIONS as f64) as usize;
        let epsilon = f64_option(options, "epsilon", Self::DEFAULT_EPSILON);

        let nodes = collect_nodes(edges);
        let n = nodes.len();

        if n == 0 {
            return Ok(build_rows(vec!["node".to_owned(), "rank".to_owned()], vec![]));
        }

        // Build adjacency list (outgoing edges)
        let mut adj: FxHashMap<Value, Vec<Value>> = FxHashMap::default();
        let mut out_degree: FxHashMap<Value, usize> = FxHashMap::default();

        for (s, t, _w) in edges {
            adj.entry(s.clone()).or_insert_with(Vec::new).push(t.clone());
            *out_degree.entry(s.clone()).or_insert(0) += 1;
        }

        // Initialize PageRank scores uniformly
        let initial_rank = 1.0 / n as f64;
        let mut ranks: FxHashMap<Value, f64> = nodes
            .iter()
            .map(|node| (node.clone(), initial_rank))
            .collect();

        // Power iteration
        let teleport = (1.0 - damping) / n as f64;

        for _ in 0..max_iter {
            let mut new_ranks: FxHashMap<Value, f64> = FxHashMap::default();
            let mut delta = 0.0;

            for node in &nodes {
                let mut rank = teleport;

                // Sum contributions from incoming edges
                for (src, targets) in &adj {
                    if targets.contains(node) {
                        let src_out = out_degree.get(src).copied().unwrap_or(1).max(1);
                        rank += damping * ranks.get(src).unwrap_or(&0.0) / src_out as f64;
                    }
                }

                delta += (rank - ranks.get(node).unwrap_or(&0.0)).abs();
                new_ranks.insert(node.clone(), rank);
            }

            ranks = new_ranks;

            if delta < epsilon {
                break;
            }
        }

        // Build result rows
        let mut rows: Vec<Vec<Value>> = ranks
            .into_iter()
            .map(|(node, rank)| vec![node, Value::from(rank)])
            .collect();

        // Sort by rank descending for consistent output
        rows.sort_by(|a, b| {
            b[1].as_float()
                .unwrap_or(0.0)
                .total_cmp(&a[1].as_float().unwrap_or(0.0))
        });

        Ok(build_rows(vec!["node".to_owned(), "rank".to_owned()], rows))
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
    fn pagerank_cycle() {
        // 3-node cycle: a -> b -> c -> a
        let edges = vec![
            (Value::from("a"), Value::from("b"), 1.0),
            (Value::from("b"), Value::from("c"), 1.0),
            (Value::from("c"), Value::from("a"), 1.0),
        ];

        let result = PageRank.run(&edges, &BTreeMap::new()).unwrap();
        assert_eq!(result.headers, vec!["node", "rank"]);
        assert_eq!(result.len(), 3);

        // In a symmetric cycle, all nodes should have equal rank
        let ranks: Vec<f64> = result
            .rows
            .iter()
            .map(|r| r[1].as_float().unwrap())
            .collect();
        for rank in &ranks {
            assert!((rank - ranks[0]).abs() < 1e-6);
        }
    }

    #[test]
    fn pagerank_star() {
        // Star graph: center 'hub' with 3 spokes
        let edges = vec![
            (Value::from("hub"), Value::from("a"), 1.0),
            (Value::from("hub"), Value::from("b"), 1.0),
            (Value::from("hub"), Value::from("c"), 1.0),
        ];

        let result = PageRank.run(&edges, &BTreeMap::new()).unwrap();
        assert_eq!(result.len(), 4);

        // Leaves should have higher rank than hub (they receive links from hub)
        let hub_rank = result
            .rows
            .iter()
            .find(|r| r[0].as_str() == Some("hub"))
            .map(|r| r[1].as_float().unwrap())
            .unwrap();
        let leaf_rank = result
            .rows
            .iter()
            .find(|r| r[0].as_str() == Some("a"))
            .map(|r| r[1].as_float().unwrap())
            .unwrap();

        // Leaves receive rank from hub, so they have higher rank
        assert!(leaf_rank > hub_rank);
    }

    #[test]
    fn pagerank_empty() {
        let result = PageRank.run(&[], &BTreeMap::new()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn pagerank_single_node() {
        let edges = vec![(Value::from("a"), Value::from("a"), 1.0)];
        let result = PageRank.run(&edges, &BTreeMap::new()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result.rows[0][0].as_str(), Some("a"));
    }
}
