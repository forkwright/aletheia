//! Correctness tests for all 18 graph algorithm fixed rules.
//!
//! Each algorithm runs against a small, hand-computed graph so the expected
//! output can be verified exactly (or bounded, for randomised algorithms).
//!
//! Key conventions:
//! - Fixed rule options are comma-separated (same as relation args), no semicolon.
//! - Algorithms using `as_directed_graph(true)` or `as_directed_weighted_graph(true, …)`
//!   mirror edges internally: supply edges in ONE direction to avoid duplicates.
//! - Algorithms that do not mirror (DegreeCentrality, BFS, DFS) expect explicit
//!   bidirectional edges if undirected semantics are needed.

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod graph_algo_tests {
    use crate::engine::DbInstance;
    use crate::engine::data::value::DataValue;

    // ────────────────────────────────────────────────────────────────────────
    // 1. ShortestPathDijkstra
    // ────────────────────────────────────────────────────────────────────────

    /// Basic correctness: 0→1→2→3→4 with edge weights, cost must be 5.0.
    #[test]
    fn test_shortest_path_dijkstra_when_path_exists_returns_correct_cost() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [0, 2, 4.0], [1, 2, 1.0],
                           [1, 3, 5.0], [2, 3, 1.0], [3, 4, 2.0]]
start[] <- [[0]]
?[from, to, cost, path] <~ ShortestPathDijkstra(edges[], start[])
:order to
"#,
            )
            .expect("Dijkstra query should execute successfully")
            .rows;

        assert!(!res.is_empty(), "Dijkstra must return results");
        let row = res
            .iter()
            .find(|r| r[1] == DataValue::from(4i64))
            .expect("row for destination node 4 should exist");
        let cost = row[2].get_float().expect("cost field should be a float");
        assert!(
            (cost - 5.0).abs() < 1e-9,
            "Dijkstra 0→4 cost = 5.0, got {cost}"
        );
        assert_eq!(
            row[3]
                .get_slice()
                .expect("path field should be a slice")
                .len(),
            5,
            "path 0→1→2→3→4 has 5 nodes"
        );
    }

    /// Dijkstra on a graph where start and destination are disconnected returns
    /// an infinite cost and empty path.
    #[test]
    fn test_shortest_path_dijkstra_when_disconnected_returns_no_path() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0]]
start[] <- [[0]]
end[]   <- [[2]]
?[from, to, cost, path] <~ ShortestPathDijkstra(edges[], start[], end[])
"#,
            )
            .expect("Dijkstra disconnected query should execute successfully")
            .rows;

        // Node 2 is unreachable; Dijkstra emits no row for it.
        assert!(
            res.is_empty()
                || res
                    .iter()
                    .all(|r| r[2].get_float().is_none_or(|c| !c.is_finite())),
            "Disconnected node must have infinite cost or produce no row"
        );
    }

    /// Dijkstra with a self-loop start+end: cost is 0.
    #[test]
    fn test_shortest_path_dijkstra_when_start_equals_end_cost_is_zero() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [1, 2, 1.0]]
start[] <- [[0]]
end[]   <- [[0]]
?[from, to, cost, path] <~ ShortestPathDijkstra(edges[], start[], end[])
"#,
            )
            .expect("Dijkstra self-loop query should execute successfully")
            .rows;

        assert_eq!(
            res.len(),
            1,
            "self-loop query should return exactly one row"
        );
        let cost = res[0][2].get_float().expect("cost field should be a float");
        assert!(
            cost.abs() < 1e-9,
            "Cost from node to itself = 0, got {cost}"
        );
    }

    /// Dijkstra with multiple starting nodes discovers all reachable targets.
    #[test]
    fn test_shortest_path_dijkstra_when_multiple_starts_covers_all_targets() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [2, 3, 1.0]]
start[] <- [[0], [2]]
?[from, to, cost, path] <~ ShortestPathDijkstra(edges[], start[])
"#,
            )
            .expect("Dijkstra multi-start query should execute successfully")
            .rows;

        assert!(
            !res.is_empty(),
            "Each start node should reach at least its neighbour"
        );
    }

    // ────────────────────────────────────────────────────────────────────────
    // 2. BetweennessCentrality
    // ────────────────────────────────────────────────────────────────────────

    /// On a path graph 0-1-2-3-4, intermediate nodes have higher betweenness.
    #[test]
    fn test_betweenness_centrality_when_path_graph_interior_nodes_are_higher() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [0, 2, 4.0], [1, 2, 1.0],
                           [1, 3, 5.0], [2, 3, 1.0], [3, 4, 2.0]]
?[node, bc] <~ BetweennessCentrality(edges[])
:order node
"#,
            )
            .expect("BetweennessCentrality query should execute successfully")
            .rows;

        assert_eq!(res.len(), 5, "5 nodes → 5 betweenness rows");
        let bc_0 = res
            .iter()
            .find(|r| r[0] == DataValue::from(0i64))
            .expect("row for node 0 should exist")[1]
            .get_float()
            .expect("betweenness score for node 0 should be a float");
        let bc_2 = res
            .iter()
            .find(|r| r[0] == DataValue::from(2i64))
            .expect("row for node 2 should exist")[1]
            .get_float()
            .expect("betweenness score for node 2 should be a float");
        assert!(
            bc_2 >= bc_0,
            "Node 2 (transit hub) BC >= node 0 BC; bc_2={bc_2}, bc_0={bc_0}"
        );
    }

    /// On a star graph, the center has strictly higher betweenness than leaves.
    #[test]
    fn test_betweenness_centrality_when_star_graph_center_is_highest() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [0, 2, 1.0], [0, 3, 1.0], [0, 4, 1.0]]
?[node, bc] <~ BetweennessCentrality(edges[], undirected: true)
:order -bc
"#,
            )
            .expect("BetweennessCentrality star query should execute successfully")
            .rows;

        assert!(
            !res.is_empty(),
            "BetweennessCentrality should return results for star graph"
        );
        assert_eq!(
            res[0][0],
            DataValue::from(0i64),
            "Star center (node 0) must have highest betweenness"
        );
    }

    /// Empty-ish graph (one isolated edge) returns zero betweenness for both nodes.
    #[test]
    fn test_betweenness_centrality_when_single_edge_returns_zero_betweenness() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0]]
?[node, bc] <~ BetweennessCentrality(edges[])
"#,
            )
            .expect("BetweennessCentrality single-edge query should execute successfully")
            .rows;

        assert_eq!(
            res.len(),
            2,
            "One edge → 2 nodes, each with zero betweenness"
        );
        for row in &res {
            assert!(
                (row[1]
                    .get_float()
                    .expect("betweenness score should be a float"))
                .abs()
                    < 1e-9,
                "No intermediate nodes ⇒ zero betweenness"
            );
        }
    }

    // ────────────────────────────────────────────────────────────────────────
    // 3. ClosenessCentrality
    // ────────────────────────────────────────────────────────────────────────

    /// On a weighted path graph, all closeness scores must be non-negative.
    #[test]
    fn test_closeness_centrality_when_path_graph_all_scores_non_negative() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [0, 2, 4.0], [1, 2, 1.0],
                           [1, 3, 5.0], [2, 3, 1.0], [3, 4, 2.0]]
?[node, cc] <~ ClosenessCentrality(edges[])
:order node
"#,
            )
            .expect("ClosenessCentrality query should execute successfully")
            .rows;

        assert_eq!(res.len(), 5, "5 nodes → 5 closeness rows");
        for row in &res {
            let cc = row[1]
                .get_float()
                .expect("closeness score should be a float");
            assert!(cc >= 0.0, "Closeness must be non-negative, got {cc}");
        }
    }

    /// Single edge: closeness is defined and non-negative for both endpoints.
    #[test]
    fn test_closeness_centrality_when_single_edge_returns_non_negative() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0]]
?[node, cc] <~ ClosenessCentrality(edges[])
"#,
            )
            .expect("ClosenessCentrality single-edge query should execute successfully")
            .rows;

        assert_eq!(res.len(), 2, "single edge should produce 2 closeness rows");
        for row in &res {
            assert!(
                row[1]
                    .get_float()
                    .expect("closeness score should be a float")
                    >= 0.0,
                "closeness score should be non-negative"
            );
        }
    }

    // ────────────────────────────────────────────────────────────────────────
    // 4. ShortestPathAStar
    // ────────────────────────────────────────────────────────────────────────

    /// A* with zero heuristic is equivalent to Dijkstra: cost 5.0 for 0→4.
    #[test]
    fn test_astar_when_zero_heuristic_equals_dijkstra_cost() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [0, 2, 4.0], [1, 2, 1.0],
                           [1, 3, 5.0], [2, 3, 1.0], [3, 4, 2.0]]
nodes[n] <- [[0], [1], [2], [3], [4]]
start[] <- [[0]]
goal[] <- [[4]]
?[from, to, cost, path] <~ ShortestPathAStar(edges[], nodes[n], start[], goal[], heuristic: 0)
"#,
            )
            .expect("A* query should execute successfully")
            .rows;

        assert_eq!(res.len(), 1, "One start-goal pair → one result");
        let cost = res[0][2]
            .get_float()
            .expect("A* cost field should be a float");
        assert!((cost - 5.0).abs() < 1e-9, "A* 0→4 cost = 5.0, got {cost}");
        assert_eq!(
            res[0][3]
                .get_slice()
                .expect("A* path field should be a slice")
                .len(),
            5,
            "path has 5 nodes"
        );
    }

    /// A* with no path between start and goal returns infinity.
    #[test]
    fn test_astar_when_no_path_returns_infinity() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0]]
nodes[n] <- [[0], [1], [2]]
start[] <- [[0]]
goal[] <- [[2]]
?[from, to, cost, path] <~ ShortestPathAStar(edges[], nodes[n], start[], goal[], heuristic: 0)
"#,
            )
            .expect("A* no-path query should execute successfully")
            .rows;

        assert_eq!(
            res.len(),
            1,
            "A* should return one row even when no path exists"
        );
        let cost = res[0][2]
            .get_float()
            .expect("A* cost field should be a float");
        assert!(cost.is_infinite(), "No path ⇒ cost = infinity, got {cost}");
    }

    /// A* on a direct single-hop edge returns cost equal to edge weight.
    #[test]
    fn test_astar_when_direct_edge_returns_edge_weight() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 3.5]]
nodes[n] <- [[0], [1]]
start[] <- [[0]]
goal[] <- [[1]]
?[from, to, cost, path] <~ ShortestPathAStar(edges[], nodes[n], start[], goal[], heuristic: 0)
"#,
            )
            .expect("A* direct-edge query should execute successfully")
            .rows;

        assert_eq!(res.len(), 1, "direct edge query should return one result");
        assert!(
            (res[0][2]
                .get_float()
                .expect("A* cost field should be a float")
                - 3.5)
                .abs()
                < 1e-9,
            "A* cost for direct edge should equal edge weight 3.5"
        );
        assert_eq!(
            res[0][3]
                .get_slice()
                .expect("A* path field should be a slice")
                .len(),
            2,
            "direct edge path should have 2 nodes"
        );
    }

    // ────────────────────────────────────────────────────────────────────────
    // 5. BFS (BreadthFirstSearch)
    // ────────────────────────────────────────────────────────────────────────

    /// BFS on a linear chain finds the end in exactly 4 hops.
    #[test]
    fn test_bfs_when_linear_chain_finds_target_at_depth_4() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [2, 3], [3, 4]]
nodes[n] <- [[0], [1], [2], [3], [4]]
start[] <- [[0]]
?[from, to, path] <~ BFS(edges[], nodes[n], start[], condition: n == 4, limit: 1)
"#,
            )
            .expect("BFS query should execute successfully")
            .rows;

        assert_eq!(res.len(), 1, "BFS finds exactly one matching node");
        assert_eq!(res[0][1], DataValue::from(4i64), "BFS target is node 4");
        assert_eq!(
            res[0][2]
                .get_slice()
                .expect("BFS path field should be a slice")
                .len(),
            5,
            "path 0..4 has 5 nodes"
        );
    }

    /// BFS with no matching condition returns no rows.
    #[test]
    fn test_bfs_when_condition_never_true_returns_empty() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2]]
nodes[n] <- [[0], [1], [2]]
start[] <- [[0]]
?[from, to, path] <~ BFS(edges[], nodes[n], start[], condition: n == 99, limit: 1)
"#,
            )
            .expect("BFS no-match query should execute successfully")
            .rows;

        assert!(res.is_empty(), "Condition never true → no results");
    }

    // ────────────────────────────────────────────────────────────────────────
    // 6. DFS (DepthFirstSearch)
    // ────────────────────────────────────────────────────────────────────────

    /// DFS finds a target reachable via a chain.
    #[test]
    fn test_dfs_when_linear_chain_finds_target() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [2, 3], [3, 4]]
nodes[n] <- [[0], [1], [2], [3], [4]]
start[] <- [[0]]
?[from, to, path] <~ DFS(edges[], nodes[n], start[], condition: n == 4, limit: 1)
"#,
            )
            .expect("DFS query should execute successfully")
            .rows;

        assert_eq!(res.len(), 1, "DFS finds exactly one matching node");
        assert_eq!(res[0][1], DataValue::from(4i64), "DFS target is node 4");
        let path = res[0][2]
            .get_slice()
            .expect("DFS path field should be a slice");
        assert_eq!(path[0], DataValue::from(0i64), "path starts at 0");
        assert_eq!(
            path[path.len() - 1],
            DataValue::from(4i64),
            "path ends at 4"
        );
    }

    /// DFS with unreachable target returns no rows.
    #[test]
    fn test_dfs_when_target_unreachable_returns_empty() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1]]
nodes[n] <- [[0], [1], [2]]
start[] <- [[0]]
?[from, to, path] <~ DFS(edges[], nodes[n], start[], condition: n == 2, limit: 1)
"#,
            )
            .expect("DFS unreachable-target query should execute successfully")
            .rows;

        assert!(res.is_empty(), "Node 2 is unreachable from 0");
    }

    // ────────────────────────────────────────────────────────────────────────
    // 7. DegreeCentrality
    // ────────────────────────────────────────────────────────────────────────

    /// Triangle+tail: node 2 is most connected, total degree = 6.
    #[test]
    fn test_degree_centrality_when_triangle_plus_tail_hub_has_highest_degree() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 0],
                    [1, 2], [2, 1],
                    [2, 0], [0, 2],
                    [2, 3], [3, 2],
                    [3, 4], [4, 3]]
?[node, total, out_deg, in_deg] <~ DegreeCentrality(edges[])
:order node
"#,
            )
            .expect("DegreeCentrality query should execute successfully")
            .rows;

        assert_eq!(res.len(), 5, "5 distinct nodes");
        let node2 = res
            .iter()
            .find(|r| r[0] == DataValue::from(2i64))
            .expect("row for node 2 should exist");
        assert_eq!(
            node2[1].get_int().expect("degree field should be an int"),
            6,
            "Node 2 total degree = 6"
        );
    }

    /// Leaf node has in-degree = 1, out-degree = 0 when only receiving edges.
    #[test]
    fn test_degree_centrality_when_directed_star_hub_has_zero_in_degree() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [0, 2], [0, 3]]
?[node, total, out_deg, in_deg] <~ DegreeCentrality(edges[])
:order node
"#,
            )
            .expect("DegreeCentrality star query should execute successfully")
            .rows;

        let node0 = res
            .iter()
            .find(|r| r[0] == DataValue::from(0i64))
            .expect("row for hub node 0 should exist");
        assert_eq!(
            node0[2]
                .get_int()
                .expect("out-degree field should be an int"),
            3,
            "Hub out-degree = 3"
        );
        assert_eq!(
            node0[3]
                .get_int()
                .expect("in-degree field should be an int"),
            0,
            "Hub in-degree = 0"
        );
    }

    /// Isolated node included via optional nodes[] relation has degree 0.
    #[test]
    fn test_degree_centrality_when_isolated_node_included_has_zero_degree() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1]]
isolated[n]    <- [[5]]
?[node, total, out_deg, in_deg] <~ DegreeCentrality(edges[], isolated[])
:order node
"#,
            )
            .expect("DegreeCentrality isolated-node query should execute successfully")
            .rows;

        let node5 = res
            .iter()
            .find(|r| r[0] == DataValue::from(5i64))
            .expect("row for isolated node 5 should exist");
        assert_eq!(
            node5[1]
                .get_int()
                .expect("total degree field should be an int"),
            0,
            "isolated node total degree should be 0"
        );
        assert_eq!(
            node5[2]
                .get_int()
                .expect("out-degree field should be an int"),
            0,
            "isolated node out-degree should be 0"
        );
        assert_eq!(
            node5[3]
                .get_int()
                .expect("in-degree field should be an int"),
            0,
            "isolated node in-degree should be 0"
        );
    }

    // ────────────────────────────────────────────────────────────────────────
    // 8. MinimumSpanningForestKruskal
    // ────────────────────────────────────────────────────────────────────────

    /// Connected 5-node graph: MST has exactly 4 edges with total cost 7.
    #[test]
    fn test_kruskal_when_connected_graph_returns_n_minus_1_edges() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [1, 2, 2.0], [0, 2, 5.0],
                           [2, 3, 3.0], [3, 4, 1.0]]
?[src, dst, cost] <~ MinimumSpanningForestKruskal(edges[])
"#,
            )
            .expect("Kruskal MST query should execute successfully")
            .rows;

        assert_eq!(res.len(), 4, "MST of 5 nodes = 4 edges");
        let total: f64 = res
            .iter()
            .map(|r| r[2].get_float().expect("MST edge cost should be a float"))
            .sum();
        assert!(
            (total - 7.0).abs() < 1e-9,
            "Kruskal MST total cost = 7.0, got {total}"
        );
    }

    /// Disconnected graph produces a spanning forest, not a tree.
    #[test]
    fn test_kruskal_when_disconnected_graph_returns_spanning_forest() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [1, 2, 2.0],
                           [3, 4, 5.0]]
?[src, dst, cost] <~ MinimumSpanningForestKruskal(edges[])
"#,
            )
            .expect("Kruskal spanning forest query should execute successfully")
            .rows;

        // 5 nodes in 2 components → forest has 3 edges (2 from comp 1, 1 from comp 2).
        assert_eq!(res.len(), 3, "Forest has n - components edges");
    }

    /// Triangle: Kruskal picks the two cheapest edges.
    #[test]
    fn test_kruskal_when_triangle_picks_two_cheapest_edges() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [1, 2, 2.0], [0, 2, 3.0]]
?[src, dst, cost] <~ MinimumSpanningForestKruskal(edges[])
"#,
            )
            .expect("Kruskal triangle query should execute successfully")
            .rows;

        assert_eq!(res.len(), 2, "Triangle MST = 2 edges");
        let total: f64 = res
            .iter()
            .map(|r| r[2].get_float().expect("MST edge cost should be a float"))
            .sum();
        assert!(
            (total - 3.0).abs() < 1e-9,
            "Triangle MST cost = 1+2 = 3.0, got {total}"
        );
    }

    // ────────────────────────────────────────────────────────────────────────
    // 9. MinimumSpanningTreePrim
    // ────────────────────────────────────────────────────────────────────────

    /// Prim on the same 5-node graph produces the same MST cost as Kruskal.
    #[test]
    fn test_prim_when_connected_graph_returns_same_cost_as_kruskal() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [1, 2, 2.0], [0, 2, 5.0],
                           [2, 3, 3.0], [3, 4, 1.0]]
?[src, dst, cost] <~ MinimumSpanningTreePrim(edges[])
"#,
            )
            .expect("Prim MST query should execute successfully")
            .rows;

        assert_eq!(res.len(), 4, "Prim MST of 5 nodes = 4 edges");
        let total: f64 = res
            .iter()
            .map(|r| r[2].get_float().expect("MST edge cost should be a float"))
            .sum();
        assert!(
            (total - 7.0).abs() < 1e-9,
            "Prim MST total cost = 7.0, got {total}"
        );
    }

    /// Prim with explicit start node finds the same spanning tree.
    #[test]
    fn test_prim_when_start_node_specified_returns_valid_spanning_tree() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [1, 2, 2.0], [2, 3, 3.0]]
start[] <- [[2]]
?[src, dst, cost] <~ MinimumSpanningTreePrim(edges[], start[])
"#,
            )
            .expect("Prim MST with start node query should execute successfully")
            .rows;

        assert_eq!(res.len(), 3, "Linear 4-node graph MST = 3 edges");
    }

    // ────────────────────────────────────────────────────────────────────────
    // 10. LabelPropagation
    // ────────────────────────────────────────────────────────────────────────

    /// Two triangles joined by a bridge: all 6 nodes receive a label.
    #[test]
    fn test_label_propagation_when_two_clusters_all_nodes_labeled() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [2, 0],
                    [3, 4], [4, 5], [5, 3],
                    [2, 3]]
?[label, node] <~ LabelPropagation(edges[], undirected: true, max_iter: 50)
"#,
            )
            .expect("LabelPropagation two-cluster query should execute successfully")
            .rows;

        assert_eq!(res.len(), 6, "Every node must receive a community label");
    }

    /// Single isolated edge: both endpoints receive a label.
    #[test]
    fn test_label_propagation_when_single_edge_returns_two_labels() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1]]
?[label, node] <~ LabelPropagation(edges[], undirected: true)
"#,
            )
            .expect("LabelPropagation single-edge query should execute successfully")
            .rows;

        assert_eq!(res.len(), 2, "Single edge → 2 nodes → 2 label rows");
    }

    /// Disconnected graph: nodes in each component still receive labels.
    #[test]
    fn test_label_propagation_when_disconnected_graph_all_nodes_labeled() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [2, 3]]
?[label, node] <~ LabelPropagation(edges[], undirected: true)
"#,
            )
            .expect("LabelPropagation disconnected-graph query should execute successfully")
            .rows;

        assert_eq!(res.len(), 4, "4 nodes → 4 label rows");
    }

    // ────────────────────────────────────────────────────────────────────────
    // 11. PageRank
    // ────────────────────────────────────────────────────────────────────────

    /// Star topology: sink node has highest rank.
    #[test]
    fn test_pagerank_when_star_graph_sink_has_highest_rank() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 4], [1, 4], [2, 4], [3, 4]]
?[node, rank] <~ PageRank(edges[], iterations: 20)
:order -rank
"#,
            )
            .expect("PageRank star query should execute successfully")
            .rows;

        assert!(!res.is_empty(), "PageRank must return results");
        assert_eq!(
            res[0][0],
            DataValue::from(4i64),
            "Node 4 (sink) should have highest PageRank"
        );
    }

    /// PageRank on a symmetric cycle should assign approximately uniform rank.
    #[test]
    fn test_pagerank_when_cycle_scores_are_approximately_uniform() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [2, 3], [3, 0]]
?[node, rank] <~ PageRank(edges[], iterations: 100)
"#,
            )
            .expect("PageRank cycle query should execute successfully")
            .rows;

        assert_eq!(res.len(), 4, "4 nodes in cycle");
        let ranks: Vec<f64> = res
            .iter()
            .map(|r| r[1].get_float().expect("PageRank score should be a float"))
            .collect();
        let mean = ranks.iter().sum::<f64>() / ranks.len() as f64;
        for &r in &ranks {
            assert!(
                (r - mean).abs() < 0.05,
                "Cycle PageRank should be ~uniform; mean={mean}, got {r}"
            );
        }
    }

    /// PageRank returns no results for an empty edge relation.
    #[test]
    fn test_pagerank_when_empty_edges_returns_no_rows() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- []
?[node, rank] <~ PageRank(edges[])
"#,
            )
            .expect("PageRank empty-graph query should execute successfully")
            .rows;

        assert!(res.is_empty(), "Empty graph → no PageRank rows");
    }

    // ────────────────────────────────────────────────────────────────────────
    // 12. RandomWalk
    // ────────────────────────────────────────────────────────────────────────

    /// Cycle ensures walk never gets stuck; steps=5 → path length 6.
    #[test]
    fn test_random_walk_when_cycle_graph_path_has_expected_length() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [2, 3], [3, 0]]
nodes[n] <- [[0], [1], [2], [3]]
start[] <- [[0]]
?[walk_id, start_node, path] <~ RandomWalk(edges[], nodes[n], start[], steps: 5, iterations: 1)
"#,
            )
            .expect("RandomWalk cycle query should execute successfully")
            .rows;

        assert_eq!(res.len(), 1, "1 iteration → 1 walk");
        assert_eq!(
            res[0][2]
                .get_slice()
                .expect("walk path field should be a slice")
                .len(),
            6,
            "steps=5 → 6-node path"
        );
        assert_eq!(res[0][1], DataValue::from(0i64), "walk starts at node 0");
    }

    /// Multiple iterations produce multiple walk rows.
    #[test]
    fn test_random_walk_when_multiple_iterations_returns_multiple_rows() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [2, 0]]
nodes[n] <- [[0], [1], [2]]
start[] <- [[0]]
?[walk_id, start_node, path] <~ RandomWalk(edges[], nodes[n], start[], steps: 3, iterations: 5)
"#,
            )
            .expect("RandomWalk multi-iteration query should execute successfully")
            .rows;

        assert_eq!(res.len(), 5, "5 iterations → 5 rows");
    }

    /// Walk from a dead-end node stops early (path shorter than steps+1).
    #[test]
    fn test_random_walk_when_dead_end_path_is_shorter_than_max_steps() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1]]
nodes[n] <- [[0], [1]]
start[] <- [[1]]
?[walk_id, start_node, path] <~ RandomWalk(edges[], nodes[n], start[], steps: 10, iterations: 1)
"#,
            )
            .expect("RandomWalk dead-end query should execute successfully")
            .rows;

        assert_eq!(res.len(), 1, "dead-end walk should return exactly one row");
        // Node 1 has no outgoing edges, so path is just [1].
        let path_len = res[0][2]
            .get_slice()
            .expect("walk path field should be a slice")
            .len();
        assert!(path_len <= 2, "Dead-end walk path len = {path_len}");
    }

    // ────────────────────────────────────────────────────────────────────────
    // 13. StronglyConnectedComponents
    // ────────────────────────────────────────────────────────────────────────

    /// Two disjoint cycles joined by a bridge: exactly 2 SCCs.
    #[test]
    fn test_scc_when_two_disjoint_cycles_returns_two_components() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [2, 0],
                    [3, 4], [4, 5], [5, 3],
                    [2, 3]]
?[node, component] <~ StronglyConnectedComponents(edges[])
:order node
"#,
            )
            .expect("SCC two-cycles query should execute successfully")
            .rows;

        assert_eq!(res.len(), 6, "6 nodes must all be assigned a component");

        let comp = |n: i64| -> DataValue {
            res.iter()
                .find(|r| r[0] == DataValue::from(n))
                .map(|r| r[1].clone())
                .expect("row for requested node should exist")
        };
        assert_eq!(comp(0), comp(1), "Nodes 0 and 1 share a SCC");
        assert_eq!(comp(0), comp(2), "Nodes 0 and 2 share a SCC");
        assert_ne!(comp(0), comp(3), "SCC {{0,1,2}} and {{3,4,5}} are distinct");
        assert_eq!(comp(3), comp(4), "Nodes 3 and 4 share a SCC");
    }

    /// In a DAG every node is its own SCC.
    #[test]
    fn test_scc_when_dag_each_node_is_its_own_component() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [2, 3]]
?[node, component] <~ StronglyConnectedComponents(edges[])
:order node
"#,
            )
            .expect("SCC DAG query should execute successfully")
            .rows;

        assert_eq!(res.len(), 4, "4 nodes in DAG");
        let comps: Vec<_> = res.iter().map(|r| r[1].clone()).collect();
        let unique: std::collections::BTreeSet<_> = comps.iter().cloned().collect();
        assert_eq!(unique.len(), 4, "Each DAG node is its own SCC");
    }

    /// Single self-loop is its own SCC.
    #[test]
    fn test_scc_when_single_self_loop_returns_one_component() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 0]]
?[node, component] <~ StronglyConnectedComponents(edges[])
"#,
            )
            .expect("SCC self-loop query should execute successfully")
            .rows;

        assert_eq!(res.len(), 1, "Self-loop is one SCC of one node");
    }

    // ────────────────────────────────────────────────────────────────────────
    // 14. ConnectedComponents (undirected SCCs)
    // ────────────────────────────────────────────────────────────────────────

    /// Two disjoint triangles: exactly 2 components.
    #[test]
    fn test_connected_components_when_two_triangles_returns_two_components() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [2, 0],
                    [3, 4], [4, 5], [5, 3]]
?[node, component] <~ ConnectedComponents(edges[])
:order node
"#,
            )
            .expect("ConnectedComponents two-triangles query should execute successfully")
            .rows;

        assert_eq!(res.len(), 6, "6 nodes");
        let comp_of = |n: i64| {
            res.iter()
                .find(|r| r[0] == DataValue::from(n))
                .expect("row for requested node should exist")[1]
                .clone()
        };
        assert_eq!(
            comp_of(0),
            comp_of(1),
            "nodes 0 and 1 should be in the same component"
        );
        assert_eq!(
            comp_of(0),
            comp_of(2),
            "nodes 0 and 2 should be in the same component"
        );
        assert_ne!(
            comp_of(0),
            comp_of(3),
            "nodes 0 and 3 should be in different components"
        );
        assert_eq!(
            comp_of(3),
            comp_of(4),
            "nodes 3 and 4 should be in the same component"
        );
        assert_eq!(
            comp_of(4),
            comp_of(5),
            "nodes 4 and 5 should be in the same component"
        );
    }

    /// Single connected component: all nodes get the same label.
    #[test]
    fn test_connected_components_when_single_chain_all_nodes_same_label() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [2, 3], [3, 4]]
?[node, component] <~ ConnectedComponents(edges[])
"#,
            )
            .expect("ConnectedComponents single-chain query should execute successfully")
            .rows;

        let comps: Vec<_> = res.iter().map(|r| r[1].clone()).collect();
        let unique: std::collections::BTreeSet<_> = comps.iter().cloned().collect();
        assert_eq!(unique.len(), 1, "All nodes in one component");
    }

    /// Directed star treated as undirected: all nodes in one component.
    #[test]
    fn test_connected_components_when_directed_star_returns_single_component() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [0, 2], [0, 3]]
?[node, component] <~ ConnectedComponents(edges[])
"#,
            )
            .expect("ConnectedComponents directed-star query should execute successfully")
            .rows;

        let comps: Vec<_> = res.iter().map(|r| r[1].clone()).collect();
        let unique: std::collections::BTreeSet<_> = comps.iter().cloned().collect();
        assert_eq!(unique.len(), 1, "Undirected view merges all star nodes");
    }

    // ────────────────────────────────────────────────────────────────────────
    // 15. TopSort
    // ────────────────────────────────────────────────────────────────────────

    /// DAG 0→1→3, 0→2→3, 3→4: topological ordering must respect all edges.
    #[test]
    fn test_top_sort_when_dag_ordering_respects_all_edges() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [0, 2], [1, 3], [2, 3], [3, 4]]
?[idx, node] <~ TopSort(edges[])
:order idx
"#,
            )
            .expect("TopSort DAG query should execute successfully")
            .rows;

        assert_eq!(res.len(), 5, "5 nodes in topological order");

        let mut pos = [0usize; 5];
        for row in &res {
            let idx = row[0]
                .get_int()
                .expect("topological index should be an int") as usize;
            let node = row[1].get_int().expect("node id should be an int") as usize;
            pos[node] = idx;
        }
        assert!(pos[0] < pos[1], "0 before 1");
        assert!(pos[0] < pos[2], "0 before 2");
        assert!(pos[1] < pos[3], "1 before 3");
        assert!(pos[2] < pos[3], "2 before 3");
        assert!(pos[3] < pos[4], "3 before 4");
    }

    /// Linear chain: topological order equals natural order.
    #[test]
    fn test_top_sort_when_linear_chain_returns_natural_order() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [2, 3]]
?[idx, node] <~ TopSort(edges[])
:order idx
"#,
            )
            .expect("TopSort linear-chain query should execute successfully")
            .rows;

        assert_eq!(res.len(), 4, "linear chain of 4 nodes should return 4 rows");
        let nodes: Vec<i64> = res
            .iter()
            .map(|r| r[1].get_int().expect("node id should be an int"))
            .collect();
        assert_eq!(nodes, vec![0, 1, 2, 3], "Linear chain order is 0,1,2,3");
    }

    /// A single directed edge 0→1: TopSort returns both nodes in order 0, 1.
    #[test]
    fn test_top_sort_when_single_edge_returns_two_nodes_in_order() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1]]
?[idx, node] <~ TopSort(edges[])
:order idx
"#,
            )
            .expect("TopSort single-edge query should execute successfully")
            .rows;

        assert_eq!(res.len(), 2, "Single edge → 2 nodes sorted");
        let nodes: Vec<i64> = res
            .iter()
            .map(|r| r[1].get_int().expect("node id should be an int"))
            .collect();
        assert_eq!(nodes, vec![0, 1], "0 must precede 1");
    }

    // ────────────────────────────────────────────────────────────────────────
    // 16. ClusteringCoefficients (TriangleCounting)
    // ────────────────────────────────────────────────────────────────────────

    /// Triangle 0-1-2 + tail node 3: nodes 0,1,2 each in 1 triangle.
    #[test]
    fn test_clustering_coefficients_when_triangle_each_node_has_one_triangle() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [0, 2], [2, 3]]
?[node, cc, triangles, degree] <~ ClusteringCoefficients(edges[])
:order node
"#,
            )
            .expect("ClusteringCoefficients triangle query should execute successfully")
            .rows;

        assert_eq!(res.len(), 4, "4 distinct nodes");

        for &n in &[0i64, 1, 2] {
            let tri = res
                .iter()
                .find(|r| r[0] == DataValue::from(n))
                .expect("row for triangle node should exist")[2]
                .get_int()
                .expect("triangle count should be an int");
            assert_eq!(tri, 1, "Node {n} should have 1 triangle, got {tri}");
        }
        let tri3 = res
            .iter()
            .find(|r| r[0] == DataValue::from(3i64))
            .expect("row for tail node 3 should exist")[2]
            .get_int()
            .expect("triangle count for node 3 should be an int");
        assert_eq!(tri3, 0, "Node 3 has no triangles");
    }

    /// Complete graph K4: every node participates in 3 triangles with cc = 1.0.
    #[test]
    fn test_clustering_coefficients_when_k4_every_node_has_cc_one() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0,1],[0,2],[0,3],[1,2],[1,3],[2,3]]
?[node, cc, triangles, degree] <~ ClusteringCoefficients(edges[])
:order node
"#,
            )
            .expect("ClusteringCoefficients K4 query should execute successfully")
            .rows;

        assert_eq!(res.len(), 4, "K4 has 4 nodes");
        for row in &res {
            let tri = row[2].get_int().expect("triangle count should be an int");
            assert_eq!(tri, 3, "Every K4 node is in 3 triangles; got {tri}");
            let cc = row[1]
                .get_float()
                .expect("clustering coefficient should be a float");
            assert!(
                (cc - 1.0).abs() < 1e-9,
                "K4 clustering coefficient = 1.0; got {cc}"
            );
        }
    }

    /// Single edge: no triangles possible for either endpoint.
    #[test]
    fn test_clustering_coefficients_when_single_edge_no_triangles() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1]]
?[node, cc, triangles, degree] <~ ClusteringCoefficients(edges[])
"#,
            )
            .expect("ClusteringCoefficients single-edge query should execute successfully")
            .rows;

        assert_eq!(res.len(), 2, "single edge should produce 2 clustering rows");
        for row in &res {
            assert_eq!(
                row[2].get_int().expect("triangle count should be an int"),
                0,
                "No triangles for a single edge"
            );
        }
    }

    // ────────────────────────────────────────────────────────────────────────
    // 17. KShortestPathYen
    // ────────────────────────────────────────────────────────────────────────

    /// Two paths 0→3: k=2 returns both in cost order.
    #[test]
    fn test_yen_when_two_paths_returns_both_in_cost_order() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [1, 3, 1.0],
                           [0, 2, 2.0], [2, 3, 2.0]]
start[] <- [[0]]
goal[] <- [[3]]
?[from, to, cost, path] <~ KShortestPathYen(edges[], start[], goal[], k: 2)
:order cost
"#,
            )
            .expect("KShortestPathYen query should execute successfully")
            .rows;

        assert_eq!(res.len(), 2, "k=2 → 2 shortest paths");
        let c1 = res[0][2]
            .get_float()
            .expect("first path cost should be a float");
        let c2 = res[1][2]
            .get_float()
            .expect("second path cost should be a float");
        assert!((c1 - 2.0).abs() < 1e-9, "1st path cost = 2.0, got {c1}");
        assert!((c2 - 4.0).abs() < 1e-9, "2nd path cost = 4.0, got {c2}");
    }

    /// Requesting k=3 when only 1 path exists returns 1 row.
    #[test]
    fn test_yen_when_fewer_paths_than_k_returns_all_available() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [1, 2, 1.0]]
start[] <- [[0]]
goal[] <- [[2]]
?[from, to, cost, path] <~ KShortestPathYen(edges[], start[], goal[], k: 3)
"#,
            )
            .expect("KShortestPathYen fewer-paths query should execute successfully")
            .rows;

        assert_eq!(res.len(), 1, "Only 1 path exists; k=3 still returns 1");
    }

    // ────────────────────────────────────────────────────────────────────────
    // 18. CommunityDetectionLouvain
    // ────────────────────────────────────────────────────────────────────────

    /// All 6 nodes in two triangles bridged together must receive community labels.
    #[test]
    fn test_louvain_when_two_triangles_all_nodes_labeled() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, w] <- [[0, 1, 1.0], [1, 2, 1.0], [2, 0, 1.0],
                        [3, 4, 1.0], [4, 5, 1.0], [5, 3, 1.0],
                        [2, 3, 0.1]]
?[label, node] <~ CommunityDetectionLouvain(edges[], undirected: true, max_iter: 20)
"#,
            )
            .expect("Louvain two-triangles query should execute successfully")
            .rows;

        assert_eq!(res.len(), 6, "All 6 nodes must receive a community label");
    }

    /// Single edge: both endpoints receive a community label.
    #[test]
    fn test_louvain_when_single_edge_both_nodes_labeled() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, w] <- [[0, 1, 1.0]]
?[label, node] <~ CommunityDetectionLouvain(edges[], undirected: true)
"#,
            )
            .expect("Louvain single-edge query should execute successfully")
            .rows;

        assert_eq!(res.len(), 2, "Single edge → 2 nodes receive labels");
    }

    // ────────────────────────────────────────────────────────────────────────
    // 19. ShortestPathBFS
    // ────────────────────────────────────────────────────────────────────────

    /// Known social-graph path: alice→eve→bob has 3 nodes.
    #[test]
    fn test_shortest_path_bfs_when_known_path_returns_three_nodes() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
love[loving, loved] <- [['alice', 'eve'], ['bob', 'alice'], ['eve', 'alice'],
                         ['eve', 'bob'], ['eve', 'charlie'], ['charlie', 'eve'],
                         ['david', 'george'], ['george', 'george']]
start[] <- [['alice']]
end[] <- [['bob']]
?[fr, to, path] <~ ShortestPathBFS(love[], start[], end[])
"#,
            )
            .expect("ShortestPathBFS social-graph query should execute successfully")
            .rows;
        assert_eq!(
            res[0][2]
                .get_slice()
                .expect("BFS path field should be a slice")
                .len(),
            3,
            "alice→eve→bob path should have 3 nodes"
        );
    }

    /// No path between disconnected nodes: result path is Null.
    #[test]
    fn test_shortest_path_bfs_when_disconnected_returns_null() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
love[loving, loved] <- [['alice', 'eve'], ['david', 'george']]
start[] <- [['alice']]
end[] <- [['george']]
?[fr, to, path] <~ ShortestPathBFS(love[], start[], end[])
"#,
            )
            .expect("ShortestPathBFS disconnected query should execute successfully")
            .rows;
        assert_eq!(res[0][2], DataValue::Null, "Disconnected ⇒ path is Null");
    }

    /// Direct edge: path has exactly 2 nodes.
    #[test]
    fn test_shortest_path_bfs_when_direct_edge_path_has_two_nodes() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1]]
start[] <- [[0]]
end[]   <- [[1]]
?[fr, to, path] <~ ShortestPathBFS(edges[], start[], end[])
"#,
            )
            .expect("ShortestPathBFS direct-edge query should execute successfully")
            .rows;

        assert_eq!(
            res[0][2]
                .get_slice()
                .expect("BFS path field should be a slice")
                .len(),
            2,
            "direct edge path should have 2 nodes"
        );
    }

    // ────────────────────────────────────────────────────────────────────────
    // 20. KCore (new algorithm: k-core decomposition)
    // ────────────────────────────────────────────────────────────────────────

    /// Clique K4 + pendant: K4 nodes are in 3-core; pendant is in 1-core.
    ///
    /// Graph: 0-1-2-3 fully connected (K4), plus node 4 connected only to 0.
    /// Expected: 0,1,2,3 → k=3; node 4 → k=1.
    #[test]
    fn test_kcore_when_clique_plus_pendant_assigns_correct_cores() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0,1],[0,2],[0,3],[1,2],[1,3],[2,3],[0,4]]
?[node, k] <~ KCore(edges[])
:order node
"#,
            )
            .expect("KCore clique-plus-pendant query should execute successfully")
            .rows;

        assert_eq!(res.len(), 5, "5 nodes → 5 k-core rows");

        let k_of = |n: i64| {
            res.iter()
                .find(|r| r[0] == DataValue::from(n))
                .expect("row for requested node should exist")[1]
                .get_int()
                .expect("k-core value should be an int")
        };
        assert_eq!(k_of(0), 3, "Node 0 (K4 member) ∈ 3-core");
        assert_eq!(k_of(1), 3, "Node 1 (K4 member) ∈ 3-core");
        assert_eq!(k_of(2), 3, "Node 2 (K4 member) ∈ 3-core");
        assert_eq!(k_of(3), 3, "Node 3 (K4 member) ∈ 3-core");
        assert_eq!(k_of(4), 1, "Node 4 (pendant) ∈ 1-core");
    }

    /// Path graph 0-1-2-3-4: every node is in exactly the 1-core.
    /// A path has no 2-core because peeling endpoints cascades inward.
    #[test]
    fn test_kcore_when_path_graph_all_nodes_are_1core() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [2, 3], [3, 4]]
?[node, k] <~ KCore(edges[])
:order node
"#,
            )
            .expect("KCore path-graph query should execute successfully")
            .rows;

        assert_eq!(res.len(), 5, "5-node path");
        // A path graph has no 2-core: each endpoint has degree 1 and peels,
        // cascading until all nodes are removed from the 2-core.
        for row in &res {
            let k = row[1].get_int().expect("k-core value should be an int");
            assert_eq!(k, 1, "All path nodes are in exactly the 1-core, got k={k}");
        }
    }

    /// Two nodes in a single edge (simplest non-trivial graph): both in 1-core.
    #[test]
    fn test_kcore_when_single_edge_both_nodes_are_1core() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1]]
?[node, k] <~ KCore(edges[])
:order node
"#,
            )
            .expect("KCore single-edge query should execute successfully")
            .rows;

        assert_eq!(res.len(), 2, "Single edge → 2 nodes");
        for row in &res {
            assert_eq!(
                row[1].get_int().expect("k-core value should be an int"),
                1,
                "Both endpoints are in 1-core"
            );
        }
    }

    /// Empty edge relation: no rows returned.
    #[test]
    fn test_kcore_when_empty_edges_returns_no_rows() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- []
?[node, k] <~ KCore(edges[])
"#,
            )
            .expect("KCore empty-graph query should execute successfully")
            .rows;

        assert!(res.is_empty(), "Empty graph ⇒ no k-core rows");
    }

    /// Disconnected graph: two K3 cliques + isolated edge:
    /// K3 nodes are in 2-core; isolated-edge nodes are in 1-core.
    #[test]
    fn test_kcore_when_disconnected_graph_assigns_per_component_cores() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0,1],[1,2],[0,2],  [3,4]]
?[node, k] <~ KCore(edges[])
:order node
"#,
            )
            .expect("KCore disconnected-graph query should execute successfully")
            .rows;

        let k_of = |n: i64| {
            res.iter()
                .find(|r| r[0] == DataValue::from(n))
                .expect("row for requested node should exist")[1]
                .get_int()
                .expect("k-core value should be an int")
        };
        assert_eq!(k_of(0), 2, "K3 node 0 ∈ 2-core");
        assert_eq!(k_of(1), 2, "K3 node 1 ∈ 2-core");
        assert_eq!(k_of(2), 2, "K3 node 2 ∈ 2-core");
        assert_eq!(k_of(3), 1, "Edge node 3 ∈ 1-core");
        assert_eq!(k_of(4), 1, "Edge node 4 ∈ 1-core");
    }
}
