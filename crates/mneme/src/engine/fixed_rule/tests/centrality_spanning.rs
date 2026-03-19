//! Centrality and spanning tree tests: DegreeCentrality, MST, LabelProp, PageRank, RandomWalk.
#![cfg(test)]
#![expect(clippy::expect_used, reason = "test assertions")]
use crate::engine::DbInstance;
use crate::engine::data::value::DataValue;

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
