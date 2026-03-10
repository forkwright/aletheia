//! Correctness tests for all 18 graph algorithm fixed rules.
//!
//! Each algorithm runs against a small, hand-computed graph so the expected
//! output can be verified exactly (or bounded, for randomised algorithms).
//!
//! Key conventions:
//! - Fixed rule options are comma-separated (same as relation args), no semicolon.
//! - Algorithms using `as_directed_graph(true)` or `as_directed_weighted_graph(true, …)`
//!   mirror edges internally — supply edges in ONE direction to avoid duplicates.
//! - Algorithms that do not mirror (DegreeCentrality, BFS, DFS) expect explicit
//!   bidirectional edges if undirected semantics are needed.

#[cfg(test)]
mod graph_algo_tests {
    use crate::engine::DbInstance;
    use crate::engine::data::value::DataValue;

    // ── 1. ShortestPathDijkstra ──────────────────────────────────────────────
    // Directed weighted graph (5 nodes):
    //   0→1 (1), 0→2 (4), 1→2 (1), 1→3 (5), 2→3 (1), 3→4 (2)
    //   Shortest 0→4 = 0→1→2→3→4, cost 5.
    #[test]
    fn test_shortest_path_dijkstra_basic() {
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
            .unwrap()
            .rows;

        assert!(!res.is_empty(), "Dijkstra must return results");
        let row = res.iter().find(|r| r[1] == DataValue::from(4i64)).unwrap();
        let cost = row[2].get_float().unwrap();
        assert!(
            (cost - 5.0).abs() < 1e-9,
            "Dijkstra 0→4 cost = 5.0, got {cost}"
        );
        assert_eq!(row[3].get_slice().unwrap().len(), 5, "path 0→1→2→3→4 has 5 nodes");
    }

    // ── 2. BetweennessCentrality ─────────────────────────────────────────────
    #[test]
    fn test_betweenness_centrality() {
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
            .unwrap()
            .rows;

        assert_eq!(res.len(), 5, "5 nodes → 5 betweenness rows");
        let bc_0 = res.iter().find(|r| r[0] == DataValue::from(0i64)).unwrap()[1]
            .get_float()
            .unwrap();
        let bc_2 = res.iter().find(|r| r[0] == DataValue::from(2i64)).unwrap()[1]
            .get_float()
            .unwrap();
        assert!(
            bc_2 >= bc_0,
            "Node 2 (transit hub) BC >= node 0 BC; bc_2={bc_2}, bc_0={bc_0}"
        );
    }

    // ── 3. ClosenessCentrality ───────────────────────────────────────────────
    #[test]
    fn test_closeness_centrality() {
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
            .unwrap()
            .rows;

        assert_eq!(res.len(), 5, "5 nodes → 5 closeness rows");
        for row in &res {
            let cc = row[1].get_float().unwrap();
            assert!(cc >= 0.0, "Closeness must be non-negative, got {cc}");
        }
    }

    // ── 4. ShortestPathAStar ─────────────────────────────────────────────────
    // heuristic: 0 is admissible → A* finds same optimal path as Dijkstra.
    #[test]
    fn test_astar_zero_heuristic_equals_dijkstra() {
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
            .unwrap()
            .rows;

        assert_eq!(res.len(), 1, "One start-goal pair → one result");
        let cost = res[0][2].get_float().unwrap();
        assert!((cost - 5.0).abs() < 1e-9, "A* 0→4 cost = 5.0, got {cost}");
        assert_eq!(res[0][3].get_slice().unwrap().len(), 5, "path has 5 nodes");
    }

    // ── 5. BFS (BreadthFirstSearch) ─────────────────────────────────────────
    // Linear chain — BFS finds node 4 from 0 in exactly 4 hops.
    #[test]
    fn test_bfs_finds_target() {
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
            .unwrap()
            .rows;

        assert_eq!(res.len(), 1, "BFS finds exactly one matching node");
        assert_eq!(res[0][1], DataValue::from(4i64), "BFS target is node 4");
        assert_eq!(res[0][2].get_slice().unwrap().len(), 5, "path 0..4 has 5 nodes");
    }

    // ── 6. DFS (DepthFirstSearch) ────────────────────────────────────────────
    #[test]
    fn test_dfs_finds_target() {
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
            .unwrap()
            .rows;

        assert_eq!(res.len(), 1, "DFS finds exactly one matching node");
        assert_eq!(res[0][1], DataValue::from(4i64), "DFS target is node 4");
        let path = res[0][2].get_slice().unwrap();
        assert_eq!(path[0], DataValue::from(0i64), "path starts at 0");
        assert_eq!(path[path.len() - 1], DataValue::from(4i64), "path ends at 4");
    }

    // ── 7. DegreeCentrality ─────────────────────────────────────────────────
    // Undirected graph: triangle 0-1-2 + tail 2-3-4.
    // Provide BOTH directions (DegreeCentrality does not mirror internally).
    // Node 2: connects to 0, 1, 3 → total degree 6.
    #[test]
    fn test_degree_centrality() {
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
            .unwrap()
            .rows;

        assert_eq!(res.len(), 5, "5 distinct nodes");
        let node2 = res.iter().find(|r| r[0] == DataValue::from(2i64)).unwrap();
        assert_eq!(node2[1].get_int().unwrap(), 6, "Node 2 total degree = 6");
    }

    // ── 8. MinimumSpanningForestKruskal ─────────────────────────────────────
    // Triangle+tail, one direction per edge (Kruskal uses undirected=true internally).
    // MST: 0-1(1) + 1-2(2) + 3-4(1) + 2-3(3) = 7. Four edges for 5 nodes.
    #[test]
    fn test_kruskal_mst() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [1, 2, 2.0], [0, 2, 5.0],
                           [2, 3, 3.0], [3, 4, 1.0]]
?[src, dst, cost] <~ MinimumSpanningForestKruskal(edges[])
"#,
            )
            .unwrap()
            .rows;

        assert_eq!(res.len(), 4, "MST of 5 nodes = 4 edges");
        let total: f64 = res.iter().map(|r| r[2].get_float().unwrap()).sum();
        assert!((total - 7.0).abs() < 1e-9, "Kruskal MST total cost = 7.0, got {total}");
    }

    // ── 9. MinimumSpanningTreePrim ───────────────────────────────────────────
    #[test]
    fn test_prim_mst() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst, cost] <- [[0, 1, 1.0], [1, 2, 2.0], [0, 2, 5.0],
                           [2, 3, 3.0], [3, 4, 1.0]]
?[src, dst, cost] <~ MinimumSpanningTreePrim(edges[])
"#,
            )
            .unwrap()
            .rows;

        assert_eq!(res.len(), 4, "Prim MST of 5 nodes = 4 edges");
        let total: f64 = res.iter().map(|r| r[2].get_float().unwrap()).sum();
        assert!((total - 7.0).abs() < 1e-9, "Prim MST total cost = 7.0, got {total}");
    }

    // ── 10. LabelPropagation ────────────────────────────────────────────────
    // Two triangles joined by a bridge. All 6 nodes must receive a label.
    #[test]
    fn test_label_propagation_node_count() {
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
            .unwrap()
            .rows;

        assert_eq!(res.len(), 6, "Every node must receive a community label");
    }

    // ── 11. PageRank ─────────────────────────────────────────────────────────
    // Star topology: nodes 0-3 all point to node 4 → node 4 has highest rank.
    #[test]
    fn test_pagerank_sink_gets_highest_rank() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 4], [1, 4], [2, 4], [3, 4]]
?[node, rank] <~ PageRank(edges[], iterations: 20)
:order -rank
"#,
            )
            .unwrap()
            .rows;

        assert!(!res.is_empty(), "PageRank must return results");
        assert_eq!(
            res[0][0],
            DataValue::from(4i64),
            "Node 4 (sink) should have highest PageRank"
        );
    }

    // ── 12. RandomWalk ───────────────────────────────────────────────────────
    // Cycle 0→1→2→3→0 ensures the walk never gets stuck.
    // steps=5 → path of 6 nodes (start + 5).
    #[test]
    fn test_random_walk_path_length() {
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
            .unwrap()
            .rows;

        assert_eq!(res.len(), 1, "1 iteration → 1 walk");
        assert_eq!(res[0][2].get_slice().unwrap().len(), 6, "steps=5 → 6-node path");
        assert_eq!(res[0][1], DataValue::from(0i64), "walk starts at node 0");
    }

    // ── 13. StronglyConnectedComponents ────────────────────────────────────
    // Cycle A: 0→1→2→0; Cycle B: 3→4→5→3; bridge 2→3 (one-way).
    // Tarjan: exactly 2 SCCs.
    #[test]
    fn test_scc_two_cycles() {
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
            .unwrap()
            .rows;

        assert_eq!(res.len(), 6, "6 nodes must all be assigned a component");

        let comp = |n: i64| -> DataValue {
            res.iter()
                .find(|r| r[0] == DataValue::from(n))
                .map(|r| r[1].clone())
                .unwrap()
        };
        assert_eq!(comp(0), comp(1), "Nodes 0 and 1 share a SCC");
        assert_eq!(comp(0), comp(2), "Nodes 0 and 2 share a SCC");
        assert_ne!(comp(0), comp(3), "SCC {{0,1,2}} and {{3,4,5}} are distinct");
        assert_eq!(comp(3), comp(4), "Nodes 3 and 4 share a SCC");
    }

    // ── 14. TopSort ─────────────────────────────────────────────────────────
    // DAG: 0→1→3, 0→2→3, 3→4.
    // Valid order: pos[0]<pos[1], pos[0]<pos[2], pos[1]<pos[3], pos[2]<pos[3], pos[3]<pos[4].
    #[test]
    fn test_top_sort_dag() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [0, 2], [1, 3], [2, 3], [3, 4]]
?[idx, node] <~ TopSort(edges[])
:order idx
"#,
            )
            .unwrap()
            .rows;

        assert_eq!(res.len(), 5, "5 nodes in topological order");

        let mut pos = [0usize; 5];
        for row in &res {
            let idx = row[0].get_int().unwrap() as usize;
            let node = row[1].get_int().unwrap() as usize;
            pos[node] = idx;
        }
        assert!(pos[0] < pos[1], "0 before 1");
        assert!(pos[0] < pos[2], "0 before 2");
        assert!(pos[1] < pos[3], "1 before 3");
        assert!(pos[2] < pos[3], "2 before 3");
        assert!(pos[3] < pos[4], "3 before 4");
    }

    // ── 15. ClusteringCoefficients (triangle counting) ────────────────────────
    // Triangle 0-1-2 + tail 2-3 (one direction each; algorithm mirrors internally).
    // Each of 0,1,2 participates in 1 triangle; node 3 has none.
    #[test]
    fn test_clustering_coefficients_triangle() {
        let db = DbInstance::default();
        let res = db
            .run_default(
                r#"
edges[src, dst] <- [[0, 1], [1, 2], [0, 2], [2, 3]]
?[node, cc, triangles, degree] <~ ClusteringCoefficients(edges[])
:order node
"#,
            )
            .unwrap()
            .rows;

        assert_eq!(res.len(), 4, "4 distinct nodes");

        for &n in &[0i64, 1, 2] {
            let tri = res
                .iter()
                .find(|r| r[0] == DataValue::from(n))
                .unwrap()[2]
                .get_int()
                .unwrap();
            assert_eq!(tri, 1, "Node {n} should have 1 triangle, got {tri}");
        }
        let tri3 = res
            .iter()
            .find(|r| r[0] == DataValue::from(3i64))
            .unwrap()[2]
            .get_int()
            .unwrap();
        assert_eq!(tri3, 0, "Node 3 has no triangles");
    }

    // ── 16. KShortestPathYen ─────────────────────────────────────────────────
    // Two paths 0→3: A=0→1→3 (cost 2), B=0→2→3 (cost 4).
    #[test]
    fn test_yen_k_shortest_paths() {
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
            .unwrap()
            .rows;

        assert_eq!(res.len(), 2, "k=2 → 2 shortest paths");
        let c1 = res[0][2].get_float().unwrap();
        let c2 = res[1][2].get_float().unwrap();
        assert!((c1 - 2.0).abs() < 1e-9, "1st path cost = 2.0, got {c1}");
        assert!((c2 - 4.0).abs() < 1e-9, "2nd path cost = 4.0, got {c2}");
    }

    // ── 17. CommunityDetectionLouvain (existing — regression guard) ──────────
    #[test]
    fn test_louvain_assigns_labels() {
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
            .unwrap()
            .rows;

        assert_eq!(res.len(), 6, "All 6 nodes must receive a community label");
    }

    // ── 18. ShortestPathBFS (existing — regression guard) ────────────────────
    #[test]
    fn test_shortest_path_bfs_regression() {
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
            .unwrap()
            .rows;
        assert_eq!(res[0][2].get_slice().unwrap().len(), 3);
    }
}
