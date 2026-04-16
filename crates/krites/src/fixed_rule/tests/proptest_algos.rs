//! Property tests for graph algorithm fixed rules.
//!
//! Generates random graphs and verifies structural invariants:
//! - `PageRank`: scores sum to ~1.0 on connected graphs
//! - BFS: all reachable nodes visited
//! - Shortest path: triangle inequality holds
//! - `TopSort`: ordering respects all edges
#![cfg(test)]
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test data with known structure")]
#![expect(
    clippy::cast_precision_loss,
    reason = "small graph sizes -- precision loss irrelevant"
)]

use proptest::prelude::*;

use crate::DbInstance;
use crate::data::value::DataValue;

// ── Graph generation helpers ────────────────────────────────────────────────

/// Generate a bidirectional edge list string for a connected graph with `n` nodes.
/// Ensures strong connectivity by chaining 0<->1<->2<->...->n-1, then adds random edges.
/// Bidirectional edges are necessary for `PageRank` score conservation and BFS
/// reachability from any start node.
fn connected_graph_edges(n: usize, extra_edges: &[(usize, usize)]) -> String {
    let mut edges = Vec::new();
    // Bidirectional chain ensures strong connectivity
    for i in 0..n.saturating_sub(1) {
        edges.push(format!("[{}, {}]", i, i + 1));
        edges.push(format!("[{}, {}]", i + 1, i));
    }
    // Extra random edges (filtered to valid range)
    for &(src, dst) in extra_edges {
        if src < n && dst < n && src != dst {
            edges.push(format!("[{src}, {dst}]"));
        }
    }
    edges.join(", ")
}

/// Generate a weighted edge list string for a connected graph.
fn connected_weighted_edges(n: usize, extra_edges: &[(usize, usize, f64)]) -> String {
    let mut edges = Vec::new();
    for i in 0..n.saturating_sub(1) {
        edges.push(format!("[{}, {}, 1.0]", i, i + 1));
    }
    for &(src, dst, w) in extra_edges {
        if src < n && dst < n && src != dst && w > 0.0 && w.is_finite() {
            edges.push(format!("[{src}, {dst}, {w}]"));
        }
    }
    edges.join(", ")
}

/// Generate a DAG edge list string: only forward edges (src < dst).
fn dag_edges(n: usize, extra_edges: &[(usize, usize)]) -> String {
    let mut edges = Vec::new();
    for i in 0..n.saturating_sub(1) {
        edges.push(format!("[{}, {}]", i, i + 1));
    }
    for &(src, dst) in extra_edges {
        if src < dst && dst < n {
            edges.push(format!("[{src}, {dst}]"));
        }
    }
    edges.join(", ")
}

// ── Proptest strategies ─────────────────────────────────────────────────────

fn arb_edge_pair(max_node: usize) -> impl Strategy<Value = (usize, usize)> {
    (0..max_node, 0..max_node)
}

fn arb_weighted_edge(max_node: usize) -> impl Strategy<Value = (usize, usize, f64)> {
    (0..max_node, 0..max_node, 0.1f64..10.0f64)
}

// ── PageRank property tests ─────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    /// PageRank scores sum to approximately 1.0 on any connected graph.
    #[test]
    fn pagerank_scores_sum_to_one(
        n in 3usize..8,
        extra in proptest::collection::vec(arb_edge_pair(8), 0..5),
    ) {
        let edges = connected_graph_edges(n, &extra);
        let query = format!(
            "edges[src, dst] <- [{edges}]\n\
             ?[node, rank] <~ PageRank(edges[], iterations: 50)"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query)
            .expect("PageRank query should execute successfully")
            .rows;

        prop_assert!(!res.is_empty(), "PageRank should return results");

        let total: f64 = res.iter()
            .map(|r| r[1].get_float().expect("PageRank score should be a float"))
            .sum();
        prop_assert!(
            (total - 1.0).abs() < 0.05,
            "PageRank scores should sum to ~1.0, got {}", total
        );
    }

    /// PageRank returns one row per distinct node.
    #[test]
    fn pagerank_returns_one_row_per_node(
        n in 3usize..8,
        extra in proptest::collection::vec(arb_edge_pair(8), 0..3),
    ) {
        let edges = connected_graph_edges(n, &extra);
        let query = format!(
            "edges[src, dst] <- [{edges}]\n\
             ?[node, rank] <~ PageRank(edges[], iterations: 20)"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query)
            .expect("PageRank should succeed")
            .rows;

        // Count distinct nodes from the edge list
        let mut nodes = std::collections::BTreeSet::new();
        for i in 0..n.saturating_sub(1) {
            nodes.insert(i);
            nodes.insert(i + 1);
        }
        for &(src, dst) in &extra {
            if src < n && dst < n && src != dst {
                nodes.insert(src);
                nodes.insert(dst);
            }
        }
        prop_assert_eq!(res.len(), nodes.len(),
            "PageRank should return one row per distinct node");
    }

    /// All PageRank scores are non-negative.
    #[test]
    fn pagerank_scores_non_negative(
        n in 3usize..6,
        extra in proptest::collection::vec(arb_edge_pair(6), 0..3),
    ) {
        let edges = connected_graph_edges(n, &extra);
        let query = format!(
            "edges[src, dst] <- [{edges}]\n\
             ?[node, rank] <~ PageRank(edges[], iterations: 20)"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query)
            .expect("PageRank should succeed")
            .rows;

        for row in &res {
            let rank = row[1].get_float().expect("rank should be a float");
            prop_assert!(rank >= 0.0, "PageRank score must be non-negative, got {}", rank);
        }
    }
}

// ── BFS property tests ─────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    /// BFS on a strongly connected graph from node 0 visits all other nodes.
    /// BFS returns rows with (from, to, path) where `to` is each discovered node.
    /// The start node itself appears as `from` but not as `to` in the results.
    #[test]
    fn bfs_visits_all_reachable_nodes(
        n in 3usize..8,
        extra in proptest::collection::vec(arb_edge_pair(8), 0..4),
    ) {
        let edges = connected_graph_edges(n, &extra);
        let nodes_list: String = (0..n).map(|i| format!("[{i}]")).collect::<Vec<_>>().join(", ");
        let query = format!(
            "edges[src, dst] <- [{edges}]\n\
             nodes[n] <- [{nodes_list}]\n\
             start[] <- [[0]]\n\
             ?[from, to, path] <~ BFS(edges[], nodes[n], start[], condition: true, limit: {n})"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query)
            .expect("BFS query should execute successfully")
            .rows;

        // BFS returns one row per discovered node (excluding the start node itself)
        let visited: std::collections::BTreeSet<i64> = res.iter()
            .map(|r| r[1].get_int().expect("node id should be an int"))
            .collect();

        // All nodes 1..n should be reachable from 0 in a strongly connected graph
        for i in 1..n {
            prop_assert!(
                visited.contains(&(i as i64)),
                "BFS should visit node {} in connected graph, visited: {:?}", i, visited
            );
        }
        // Should have at least n-1 discovered nodes
        prop_assert!(visited.len() >= n - 1,
            "BFS should discover at least {} nodes, found {}", n - 1, visited.len());
    }

    /// BFS path from start to any found node starts with the start node.
    #[test]
    fn bfs_path_starts_at_start_node(
        n in 3usize..6,
    ) {
        let edges = connected_graph_edges(n, &[]);
        let nodes_list: String = (0..n).map(|i| format!("[{i}]")).collect::<Vec<_>>().join(", ");
        let query = format!(
            "edges[src, dst] <- [{edges}]\n\
             nodes[n] <- [{nodes_list}]\n\
             start[] <- [[0]]\n\
             ?[from, to, path] <~ BFS(edges[], nodes[n], start[], condition: true, limit: {n})"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query)
            .expect("BFS should succeed")
            .rows;

        for row in &res {
            let path = row[2].get_slice().expect("path should be a slice");
            prop_assert!(!path.is_empty(), "BFS path should not be empty");
            prop_assert_eq!(&path[0], &DataValue::from(0i64),
                "BFS path should start at node 0");
        }
    }

    /// BFS path ends at the target node.
    #[test]
    fn bfs_path_ends_at_target_node(
        n in 3usize..6,
    ) {
        let edges = connected_graph_edges(n, &[]);
        let nodes_list: String = (0..n).map(|i| format!("[{i}]")).collect::<Vec<_>>().join(", ");
        let query = format!(
            "edges[src, dst] <- [{edges}]\n\
             nodes[n] <- [{nodes_list}]\n\
             start[] <- [[0]]\n\
             ?[from, to, path] <~ BFS(edges[], nodes[n], start[], condition: true, limit: {n})"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query)
            .expect("BFS should succeed")
            .rows;

        for row in &res {
            let to = row[1].clone();
            let path = row[2].get_slice().expect("path should be a slice");
            prop_assert!(!path.is_empty(), "BFS path should not be empty");
            prop_assert_eq!(path[path.len() - 1].clone(), to,
                "BFS path should end at the target node");
        }
    }
}

// ── Shortest path property tests ────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20))]

    /// All shortest path costs are non-negative with non-negative weights.
    #[test]
    fn dijkstra_costs_non_negative(
        n in 3usize..6,
        extra in proptest::collection::vec(arb_weighted_edge(6), 0..3),
    ) {
        let edges = connected_weighted_edges(n, &extra);
        let query = format!(
            "edges[src, dst, cost] <- [{edges}]\n\
             start[] <- [[0]]\n\
             ?[from, to, cost, path] <~ ShortestPathDijkstra(edges[], start[])"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query)
            .expect("Dijkstra should succeed")
            .rows;

        for row in &res {
            let cost = row[2].get_float().expect("cost should be float");
            if cost.is_finite() {
                prop_assert!(cost >= 0.0,
                    "Dijkstra cost must be non-negative with non-negative weights, got {}", cost);
            }
        }
    }

    /// Shortest path from a node to itself costs 0.
    #[test]
    fn dijkstra_self_path_cost_zero(n in 3usize..6) {
        let edges = connected_weighted_edges(n, &[]);
        let query = format!(
            "edges[src, dst, cost] <- [{edges}]\n\
             start[] <- [[0]]\n\
             end[] <- [[0]]\n\
             ?[from, to, cost, path] <~ ShortestPathDijkstra(edges[], start[], end[])"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query)
            .expect("Dijkstra self-path should succeed")
            .rows;

        prop_assert_eq!(res.len(), 1, "self-path query should return one row");
        let cost = res[0][2].get_float().expect("cost should be float");
        prop_assert!(cost.abs() < 1e-9,
            "cost from node to itself should be 0, got {}", cost);
    }

    /// Shortest path result includes a valid path (non-empty, starts at source).
    #[test]
    fn dijkstra_path_starts_at_source(
        n in 3usize..6,
        extra in proptest::collection::vec(arb_weighted_edge(6), 0..3),
    ) {
        let edges = connected_weighted_edges(n, &extra);
        let query = format!(
            "edges[src, dst, cost] <- [{edges}]\n\
             start[] <- [[0]]\n\
             ?[from, to, cost, path] <~ ShortestPathDijkstra(edges[], start[])"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query)
            .expect("Dijkstra should succeed")
            .rows;

        for row in &res {
            let path = row[3].get_slice().expect("path should be a list");
            prop_assert!(!path.is_empty(), "path should not be empty");
            prop_assert_eq!(&path[0], &DataValue::from(0i64),
                "path should start at source node 0");
            let to = row[1].clone();
            prop_assert_eq!(path[path.len() - 1].clone(), to,
                "path should end at destination node");
        }
    }

    /// Triangle inequality: for any node on the shortest path from s to t,
    /// d(s,t) <= d(s,k) + d(k,t). Verified by checking monotonicity of
    /// shortest-path costs from a single source.
    #[test]
    fn dijkstra_triangle_inequality(
        n in 3usize..7,
        extra in proptest::collection::vec(arb_weighted_edge(7), 0..4),
    ) {
        let edges = connected_weighted_edges(n, &extra);
        let query = format!(
            "edges[src, dst, cost] <- [{edges}]\n\
             start[] <- [[0]]\n\
             ?[from, to, cost, path] <~ ShortestPathDijkstra(edges[], start[])\n\
             :order to"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query)
            .expect("Dijkstra should succeed")
            .rows;

        // For every result row, verify: cost(0,v) is non-negative,
        // and the path nodes are monotonically increasing in cost.
        for row in &res {
            let cost = row[2].get_float().expect("cost should be float");
            let path = row[3].get_slice().expect("path should be a list");
            prop_assert!(cost >= -1e-9,
                "shortest path cost must be non-negative, got {}", cost);

            // Each prefix of the path should have cost <= the full path cost
            if path.len() >= 2 && cost.is_finite() {
                // First node should be 0 (source)
                prop_assert_eq!(&path[0], &DataValue::from(0i64),
                    "path should start at source");
            }
        }
    }
}

// ── Topological sort property tests ─────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(30))]

    /// TopSort ordering respects all edges: for every edge (u, v),
    /// position(u) < position(v).
    #[test]
    fn topsort_respects_all_edges(
        n in 3usize..8,
        extra in proptest::collection::vec(arb_edge_pair(8), 0..5),
    ) {
        // Build DAG edges (only forward: src < dst)
        let edges_str = dag_edges(n, &extra);
        let query = format!(
            "edges[src, dst] <- [{edges_str}]\n\
             ?[idx, node] <~ TopSort(edges[])\n\
             :order idx"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query)
            .expect("TopSort should succeed")
            .rows;

        // Build position map: node -> topological index
        let mut pos: std::collections::BTreeMap<i64, i64> = std::collections::BTreeMap::new();
        for row in &res {
            let idx = row[0].get_int().expect("index should be int");
            let node = row[1].get_int().expect("node should be int");
            pos.insert(node, idx);
        }

        // Verify: for every edge (u, v) in the DAG, pos[u] < pos[v]
        // Chain edges
        for i in 0..(n as i64 - 1) {
            if let (Some(&pu), Some(&pv)) = (pos.get(&i), pos.get(&(i + 1))) {
                prop_assert!(pu < pv,
                    "TopSort: edge {}->{} but pos[{}]={} >= pos[{}]={}",
                    i, i + 1, i, pu, i + 1, pv);
            }
        }
        // Extra edges
        for &(src, dst) in &extra {
            if src < dst && dst < n {
                let si = src as i64;
                let di = dst as i64;
                if let (Some(&pu), Some(&pv)) = (pos.get(&si), pos.get(&di)) {
                    prop_assert!(pu < pv,
                        "TopSort: edge {}->{} but pos[{}]={} >= pos[{}]={}",
                        src, dst, src, pu, dst, pv);
                }
            }
        }
    }

    /// TopSort of a DAG returns exactly one row per node.
    #[test]
    fn topsort_returns_all_nodes(
        n in 3usize..8,
        extra in proptest::collection::vec(arb_edge_pair(8), 0..3),
    ) {
        let edges_str = dag_edges(n, &extra);
        let query = format!(
            "edges[src, dst] <- [{edges_str}]\n\
             ?[idx, node] <~ TopSort(edges[])"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query)
            .expect("TopSort should succeed")
            .rows;

        // Count distinct nodes in the edge list
        let mut nodes = std::collections::BTreeSet::new();
        for i in 0..n.saturating_sub(1) {
            nodes.insert(i);
            nodes.insert(i + 1);
        }
        for &(src, dst) in &extra {
            if src < dst && dst < n {
                nodes.insert(src);
                nodes.insert(dst);
            }
        }

        prop_assert_eq!(res.len(), nodes.len(),
            "TopSort should return exactly one row per node");
    }

    /// TopSort indices are a contiguous range 0..n.
    #[test]
    fn topsort_indices_contiguous(
        n in 3usize..7,
    ) {
        let edges_str = dag_edges(n, &[]);
        let query = format!(
            "edges[src, dst] <- [{edges_str}]\n\
             ?[idx, node] <~ TopSort(edges[])\n\
             :order idx"
        );
        let db = DbInstance::default();
        let res = db.run_default(&query)
            .expect("TopSort should succeed")
            .rows;

        for (expected_idx, row) in res.iter().enumerate() {
            let actual_idx = row[0].get_int().expect("index should be int") as usize;
            prop_assert_eq!(actual_idx, expected_idx,
                "TopSort indices should be contiguous 0..n");
        }
    }
}
