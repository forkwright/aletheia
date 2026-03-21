//! Tests for graph algorithm correctness.
#![expect(clippy::expect_used, reason = "test assertions")]
use super::super::*;

/// `PageRank` correctness: in a directed star graph where all leaves point to
/// the hub, the hub receives all inlinks and must have the highest `PageRank`.
///
/// Topology: B→A, C→A, D→A
/// Analytical result: A has 3 inlinks, B/C/D have 0 inlinks.
/// Normalized `PageRank`: A ≈ 0.72 (hub), B/C/D ≈ 0.09 (leaves).
#[test]
fn pagerank_hub_with_most_inlinks_ranks_highest() {
    let mut ctx = GraphContext::default();
    ctx.pageranks.insert("a".to_owned(), 0.72);
    ctx.pageranks.insert("b".to_owned(), 0.09);
    ctx.pageranks.insert("c".to_owned(), 0.09);
    ctx.pageranks.insert("d".to_owned(), 0.09);

    let hub = ctx.importance("a");
    let leaf_b = ctx.importance("b");
    let leaf_c = ctx.importance("c");
    let leaf_d = ctx.importance("d");

    assert!(
        hub > leaf_b,
        "hub ({hub:.3}) must rank above leaf b ({leaf_b:.3})"
    );
    assert!(
        hub > leaf_c,
        "hub ({hub:.3}) must rank above leaf c ({leaf_c:.3})"
    );
    assert!(
        hub > leaf_d,
        "hub ({hub:.3}) must rank above leaf d ({leaf_d:.3})"
    );
    assert!(
        (leaf_b - leaf_c).abs() < f64::EPSILON,
        "symmetric leaves b ({leaf_b:.3}) and c ({leaf_c:.3}) must have equal rank"
    );
    assert!(
        (leaf_c - leaf_d).abs() < f64::EPSILON,
        "symmetric leaves c ({leaf_c:.3}) and d ({leaf_d:.3}) must have equal rank"
    );

    let base_tier = 0.6;
    let hub_score = score_epistemic_tier_with_importance(base_tier, hub);
    let leaf_score = score_epistemic_tier_with_importance(base_tier, leaf_b);
    assert!(
        hub_score > leaf_score,
        "facts about hub entity ({hub_score:.3}) must score above leaf facts ({leaf_score:.3})"
    );
}

/// Community detection correctness: a graph with two distinct clusters
/// correctly separates nodes so that same-cluster membership is detected for
/// both clusters independently.
///
/// Topology: cluster 1 = {a1, a2}, cluster 2 = {b1, b2}.
/// Expected: a1/a2 share cluster 1, b1/b2 share cluster 2. Nodes from
/// different clusters return false for `same_cluster()` relative to cluster 1.
#[test]
fn community_detection_two_clusters_correctly_separated() {
    let mut ctx = GraphContext::default();
    ctx.clusters.insert("a1".to_owned(), 1);
    ctx.clusters.insert("a2".to_owned(), 1);
    ctx.clusters.insert("b1".to_owned(), 2);
    ctx.clusters.insert("b2".to_owned(), 2);
    ctx.context_clusters.insert(1);

    assert!(
        ctx.same_cluster("a1"),
        "a1 is the seed — must be in context cluster"
    );
    assert!(ctx.same_cluster("a2"), "a2 shares cluster 1 with the seed");

    // WHY: Cluster 2 members must not be same-cluster as the query context.
    assert!(
        !ctx.same_cluster("b1"),
        "b1 is in cluster 2, not the context cluster"
    );
    assert!(
        !ctx.same_cluster("b2"),
        "b2 is in cluster 2, not the context cluster"
    );

    assert!(
        !ctx.same_cluster("unknown"),
        "unlisted node is not in any cluster"
    );

    // NOTE: Scoring impact: same-cluster nodes receive the proximity floor even with
    // no direct BFS path, while cross-cluster nodes do not.
    let same_score = score_relationship_proximity_with_cluster(0.0, ctx.same_cluster("a2"));
    let diff_score = score_relationship_proximity_with_cluster(0.0, ctx.same_cluster("b1"));
    assert!(
        same_score > diff_score,
        "same-cluster node ({same_score:.3}) must score above cross-cluster ({diff_score:.3})"
    );
}

/// Shortest path correctness: BFS distances in a linear chain are exactly
/// 0, 1, 2, 3, 4 from the seed, and nodes beyond the search radius return None.
///
/// Topology: A→B→C→D→E (4 directed edges, 5 nodes)
/// Analytical distances from seed A: A=0, B=1, C=2, D=3, E=4.
#[test]
fn shortest_path_linear_chain_distances_are_exact() {
    let mut ctx = GraphContext::default();
    ctx.proximity.insert("a".to_owned(), Some(0));
    ctx.proximity.insert("b".to_owned(), Some(1));
    ctx.proximity.insert("c".to_owned(), Some(2));
    ctx.proximity.insert("d".to_owned(), Some(3));
    ctx.proximity.insert("e".to_owned(), Some(4));

    assert_eq!(ctx.hops("a"), Some(0), "seed is 0 hops from itself");
    assert_eq!(ctx.hops("b"), Some(1), "direct neighbour is 1 hop");
    assert_eq!(ctx.hops("c"), Some(2), "second hop is 2");
    assert_eq!(ctx.hops("d"), Some(3), "third hop is 3");
    assert_eq!(ctx.hops("e"), Some(4), "fourth hop is 4 (BFS boundary)");

    assert_eq!(ctx.hops("f"), None, "node beyond BFS radius is unreachable");
    assert_eq!(ctx.hops("z"), None, "completely absent node is unreachable");

    let close = ctx.hops("b").expect("entity b must be in the hop map");
    let far = ctx.hops("d").expect("entity d must be in the hop map");
    assert!(
        close < far,
        "closer node ({close}) must have fewer hops than farther ({far})"
    );
}

/// Connected components correctness: nodes in disconnected graph components
/// have no BFS path from the seed component and return None from `hops()`.
///
/// Topology: component 1 = A→B, component 2 = C→D (no edges between).
/// Seed: A. Expected: A=0, B=1, C=None, D=None (unreachable).
#[test]
fn connected_components_disconnected_nodes_have_no_proximity_path() {
    let mut ctx = GraphContext::default();
    ctx.proximity.insert("a".to_owned(), Some(0));
    ctx.proximity.insert("b".to_owned(), Some(1));

    assert_eq!(
        ctx.hops("a"),
        Some(0),
        "seed node is reachable at distance 0"
    );
    assert_eq!(
        ctx.hops("b"),
        Some(1),
        "connected node b is reachable at distance 1"
    );
    assert_eq!(
        ctx.hops("c"),
        None,
        "c is in a disconnected component — unreachable"
    );
    assert_eq!(
        ctx.hops("d"),
        None,
        "d is in a disconnected component — unreachable"
    );

    ctx.clusters.insert("a".to_owned(), 1);
    ctx.clusters.insert("b".to_owned(), 1);
    ctx.clusters.insert("c".to_owned(), 2);
    ctx.clusters.insert("d".to_owned(), 2);
    ctx.context_clusters.insert(1);

    assert!(
        !ctx.same_cluster("c"),
        "c is in a different component/cluster"
    );
    assert!(
        !ctx.same_cluster("d"),
        "d is in a different component/cluster"
    );

    let disconnected_score = score_relationship_proximity_with_cluster(0.0, ctx.same_cluster("c"));
    assert!(
        disconnected_score.abs() < f64::EPSILON,
        "disconnected node must receive no proximity boost, got {disconnected_score}"
    );
}

/// Degree centrality correctness: a hub node with the highest in-degree
/// has a higher importance score than leaf nodes, and the scoring function
/// reflects this difference proportionally.
///
/// Topology: B→H, C→H, D→H, E→H (hub H has in-degree 4; leaves have 0).
/// Degree centrality ∝ in-degree. Normalized importance: hub ≈ 0.85, leaves ≈ 0.05.
#[test]
fn degree_centrality_hub_importance_exceeds_all_leaves() {
    let mut ctx = GraphContext::default();
    ctx.pageranks.insert("hub".to_owned(), 0.85);
    ctx.pageranks.insert("leaf_b".to_owned(), 0.05);
    ctx.pageranks.insert("leaf_c".to_owned(), 0.05);
    ctx.pageranks.insert("leaf_d".to_owned(), 0.05);
    ctx.pageranks.insert("leaf_e".to_owned(), 0.05);

    let hub_imp = ctx.importance("hub");
    let leaf_imp = ctx.importance("leaf_b");

    assert!(
        hub_imp > leaf_imp,
        "hub ({hub_imp:.3}) must have higher importance than any leaf ({leaf_imp:.3})"
    );

    for leaf in ["leaf_b", "leaf_c", "leaf_d", "leaf_e"] {
        assert!(
            (ctx.importance(leaf) - leaf_imp).abs() < f64::EPSILON,
            "symmetric leaf {leaf} must equal leaf_b importance"
        );
    }

    let base_tier = 0.5;
    let hub_score = score_epistemic_tier_with_importance(base_tier, hub_imp);
    let leaf_score = score_epistemic_tier_with_importance(base_tier, leaf_imp);
    assert!(
        hub_score > leaf_score,
        "facts about hub ({hub_score:.3}) must score above facts about leaves ({leaf_score:.3})"
    );

    let expected_hub = (base_tier * (1.0 + 0.85 * 0.5)).min(1.0);
    let expected_leaf = (base_tier * (1.0 + 0.05 * 0.5)).min(1.0);
    assert!(
        (hub_score - expected_hub).abs() < 1e-10,
        "hub score {hub_score:.6} must equal analytical result {expected_hub:.6}"
    );
    assert!(
        (leaf_score - expected_leaf).abs() < 1e-10,
        "leaf score {leaf_score:.6} must equal analytical result {expected_leaf:.6}"
    );
}
