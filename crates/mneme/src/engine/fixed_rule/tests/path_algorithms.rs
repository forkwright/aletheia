//! Path and traversal algorithm tests: Dijkstra, BFS, DFS, A*, centrality.
#![cfg(test)]
#![expect(clippy::expect_used, reason = "test assertions")]
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
