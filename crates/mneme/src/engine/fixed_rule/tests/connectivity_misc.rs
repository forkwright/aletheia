//! Connectivity and misc tests: SCC, CC, TopSort, Clustering, KSP, Louvain, KCore.
#![cfg(test)]
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use crate::engine::DbInstance;
use crate::engine::data::value::DataValue;

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
