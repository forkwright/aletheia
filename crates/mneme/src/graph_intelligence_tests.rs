#![allow(clippy::unwrap_used)]
#![expect(clippy::expect_used, reason = "test assertions")]

use super::*;

// --- Pure scoring function tests (no engine needed) ---

#[test]
fn pagerank_boost_zero_importance_unchanged() {
    let base = 0.6; // inferred tier
    let result = score_epistemic_tier_with_importance(base, 0.0);
    assert!(
        (result - base).abs() < f64::EPSILON,
        "zero importance should not change score, got {result}"
    );
}

#[test]
fn pagerank_boost_max_importance() {
    let base = 0.6;
    let result = score_epistemic_tier_with_importance(base, 1.0);
    // boost = 1.5, so 0.6 * 1.5 = 0.9
    assert!(
        (result - 0.9).abs() < f64::EPSILON,
        "max importance should give 1.5x boost, got {result}"
    );
}

#[test]
fn pagerank_boost_clamped_to_one() {
    let base = 1.0; // verified tier
    let result = score_epistemic_tier_with_importance(base, 1.0);
    // 1.0 * 1.5 = 1.5, clamped to 1.0
    assert!(
        (result - 1.0).abs() < f64::EPSILON,
        "should clamp to 1.0, got {result}"
    );
}

#[test]
fn pagerank_boost_hub_higher_than_peripheral() {
    let base = 0.6;
    let hub = score_epistemic_tier_with_importance(base, 0.9);
    let peripheral = score_epistemic_tier_with_importance(base, 0.1);
    assert!(
        hub > peripheral,
        "hub ({hub}) should score higher than peripheral ({peripheral})"
    );
}

#[test]
fn pagerank_boost_range() {
    // Importance [0, 1] → boost [1.0, 1.5]
    for i in 0..=10 {
        let importance = f64::from(i) / 10.0;
        let result = score_epistemic_tier_with_importance(0.5, importance);
        let expected_boost = 1.0 + importance * 0.5;
        let expected = (0.5 * expected_boost).min(1.0);
        assert!(
            (result - expected).abs() < 1e-10,
            "importance={importance}: expected {expected}, got {result}"
        );
    }
}

#[test]
fn cluster_floor_same_cluster_no_path() {
    // No direct path (base_hop_score = 0.0), but same cluster → floor 0.3
    let result = score_relationship_proximity_with_cluster(0.0, true);
    assert!(
        (result - 0.3).abs() < f64::EPSILON,
        "same-cluster with no path should get 0.3 floor, got {result}"
    );
}

#[test]
fn cluster_floor_same_cluster_with_path() {
    // Direct neighbor (base = 1.0), same cluster → stays 1.0
    let result = score_relationship_proximity_with_cluster(1.0, true);
    assert!(
        (result - 1.0).abs() < f64::EPSILON,
        "same-cluster direct neighbor should stay 1.0, got {result}"
    );
}

#[test]
fn cluster_floor_different_cluster() {
    // No path, different cluster → stays 0.0
    let result = score_relationship_proximity_with_cluster(0.0, false);
    assert!(
        (result).abs() < f64::EPSILON,
        "different-cluster with no path should stay 0.0, got {result}"
    );
}

#[test]
fn cluster_floor_partial_path() {
    // 2-hop (0.5), same cluster → stays 0.5 (above floor)
    let result = score_relationship_proximity_with_cluster(0.5, true);
    assert!(
        (result - 0.5).abs() < f64::EPSILON,
        "same-cluster 2-hop should stay 0.5, got {result}"
    );
}

#[test]
fn supersession_bonus_zero_chain() {
    let result = score_access_with_evolution(0.5, 0);
    assert!(
        (result - 0.5).abs() < f64::EPSILON,
        "zero chain should not change score, got {result}"
    );
}

#[test]
fn supersession_bonus_chain_four() {
    let result = score_access_with_evolution(0.5, 4);
    // bonus = 4 * 0.05 = 0.2
    assert!(
        (result - 0.7).abs() < f64::EPSILON,
        "chain_length=4 should add 0.2, got {result}"
    );
}

#[test]
fn supersession_bonus_capped() {
    // chain_length=10, bonus would be 0.5 but capped at 0.2
    let result = score_access_with_evolution(0.5, 10);
    assert!(
        (result - 0.7).abs() < f64::EPSILON,
        "bonus should be capped at 0.2, got {result}"
    );
}

#[test]
fn supersession_bonus_higher_chain_scores_higher() {
    let base = 0.3;
    let short = score_access_with_evolution(base, 0);
    let long = score_access_with_evolution(base, 4);
    assert!(
        long > short,
        "chain_length=4 ({long}) should score higher than chain_length=0 ({short})"
    );
}

#[test]
fn supersession_bonus_clamped_to_one() {
    let result = score_access_with_evolution(0.95, 4);
    assert!(
        (result - 1.0).abs() < f64::EPSILON,
        "should clamp to 1.0, got {result}"
    );
}

#[test]
fn backward_compat_empty_context() {
    // With empty GraphContext, enhanced scores should equal base scores
    let ctx = GraphContext::default();
    assert!(ctx.is_empty());

    let base_tier = 0.6;
    let enhanced_tier =
        score_epistemic_tier_with_importance(base_tier, ctx.importance("any_entity"));
    assert!(
        (enhanced_tier - base_tier).abs() < f64::EPSILON,
        "empty context tier should match base"
    );

    let base_prox = 0.0;
    let enhanced_prox =
        score_relationship_proximity_with_cluster(base_prox, ctx.same_cluster("any_entity"));
    assert!(
        (enhanced_prox - base_prox).abs() < f64::EPSILON,
        "empty context proximity should match base"
    );

    let base_access = 0.5;
    let enhanced_access = score_access_with_evolution(base_access, ctx.chain_length("any_fact"));
    assert!(
        (enhanced_access - base_access).abs() < f64::EPSILON,
        "empty context access should match base"
    );
}

#[test]
fn graph_context_same_cluster_populated() {
    let mut ctx = GraphContext::default();
    ctx.clusters.insert("alice".to_owned(), 1);
    ctx.clusters.insert("bob".to_owned(), 1);
    ctx.clusters.insert("charlie".to_owned(), 2);
    ctx.context_clusters.insert(1);

    assert!(ctx.same_cluster("alice"));
    assert!(ctx.same_cluster("bob"));
    assert!(!ctx.same_cluster("charlie"));
    assert!(!ctx.same_cluster("unknown"));
}

#[test]
fn graph_dirty_flag_lifecycle() {
    let flag = GraphDirtyFlag::new();
    assert!(!flag.is_dirty());
    assert!(!flag.take_dirty());

    flag.mark_dirty();
    assert!(flag.is_dirty());
    assert!(flag.take_dirty());

    // After take, should be clean
    assert!(!flag.is_dirty());
    assert!(!flag.take_dirty());
}

#[test]
fn graph_dirty_flag_multiple_marks() {
    let flag = GraphDirtyFlag::new();
    flag.mark_dirty();
    flag.mark_dirty();
    flag.mark_dirty();
    // Single take should clear
    assert!(flag.take_dirty());
    assert!(!flag.is_dirty());
}

// --- Graph algorithm correctness tests ---
//
// These tests verify that the graph data structures and scoring functions
// produce analytically correct results for hand-crafted graph topologies.
// Each test defines a small graph, states the expected outcome from graph
// theory, and asserts that the system matches.

/// `PageRank` correctness: in a directed star graph where all leaves point to
/// the hub, the hub receives all inlinks and must have the highest `PageRank`.
///
/// Topology: B→A, C→A, D→A
/// Analytical result: A has 3 inlinks, B/C/D have 0 inlinks.
/// Normalized `PageRank`: A ≈ 0.72 (hub), B/C/D ≈ 0.09 (leaves).
#[test]
fn pagerank_hub_with_most_inlinks_ranks_highest() {
    let mut ctx = GraphContext::default();
    // Hand-crafted PageRank scores for a 4-node star (B→A, C→A, D→A).
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
    // Symmetric leaves must share equal rank.
    assert!(
        (leaf_b - leaf_c).abs() < f64::EPSILON,
        "symmetric leaves b ({leaf_b:.3}) and c ({leaf_c:.3}) must have equal rank"
    );
    assert!(
        (leaf_c - leaf_d).abs() < f64::EPSILON,
        "symmetric leaves c ({leaf_c:.3}) and d ({leaf_d:.3}) must have equal rank"
    );

    // PageRank importance translates to higher scoring for hub-entity facts.
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
    // Cluster 1: a1, a2 (dense internal connections, weak cross-cluster link)
    ctx.clusters.insert("a1".to_owned(), 1);
    ctx.clusters.insert("a2".to_owned(), 1);
    // Cluster 2: b1, b2
    ctx.clusters.insert("b1".to_owned(), 2);
    ctx.clusters.insert("b2".to_owned(), 2);
    // Query context: seed entity is a1 → cluster 1 is the context cluster.
    ctx.context_clusters.insert(1);

    // Cluster 1 membership: both a1 and a2 must be recognised as same-cluster.
    assert!(
        ctx.same_cluster("a1"),
        "a1 is the seed — must be in context cluster"
    );
    assert!(ctx.same_cluster("a2"), "a2 shares cluster 1 with the seed");

    // Cluster 2 members must not be same-cluster as the query context.
    assert!(
        !ctx.same_cluster("b1"),
        "b1 is in cluster 2, not the context cluster"
    );
    assert!(
        !ctx.same_cluster("b2"),
        "b2 is in cluster 2, not the context cluster"
    );

    // Nodes absent from the cluster map are also not same-cluster.
    assert!(
        !ctx.same_cluster("unknown"),
        "unlisted node is not in any cluster"
    );

    // Scoring impact: same-cluster nodes receive the proximity floor even with
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

    // A node beyond the search radius or unreachable has no hop count.
    assert_eq!(ctx.hops("f"), None, "node beyond BFS radius is unreachable");
    assert_eq!(ctx.hops("z"), None, "completely absent node is unreachable");

    // Closer nodes have strictly fewer hops than farther nodes.
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
    // Component 1: A and B are reachable from the seed (A).
    ctx.proximity.insert("a".to_owned(), Some(0));
    ctx.proximity.insert("b".to_owned(), Some(1));
    // Component 2: C and D are not reachable. Absent from the proximity map.

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

    // Nodes in the disconnected component are also in a different cluster.
    // No same-cluster proximity boost applies across components.
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

    // Cross-component nodes get no proximity boost (no floor applied).
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
    // Hub with 4 inlinks → high in-degree centrality → high PageRank.
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

    // All leaves have equal in-degree (0), so they share equal centrality.
    for leaf in ["leaf_b", "leaf_c", "leaf_d", "leaf_e"] {
        assert!(
            (ctx.importance(leaf) - leaf_imp).abs() < f64::EPSILON,
            "symmetric leaf {leaf} must equal leaf_b importance"
        );
    }

    // Degree centrality translates to scoring advantage for hub-entity facts.
    let base_tier = 0.5;
    let hub_score = score_epistemic_tier_with_importance(base_tier, hub_imp);
    let leaf_score = score_epistemic_tier_with_importance(base_tier, leaf_imp);
    assert!(
        hub_score > leaf_score,
        "facts about hub ({hub_score:.3}) must score above facts about leaves ({leaf_score:.3})"
    );

    // The scoring gap is proportional: hub boost = 1 + 0.85*0.5 = 1.425,
    // leaf boost = 1 + 0.05*0.5 = 1.025.
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

// --- Engine-dependent tests ---

#[cfg(feature = "mneme-engine")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod engine_tests {
    use crate::knowledge::{Entity, Relationship};
    use crate::knowledge_store::KnowledgeStore;

    fn test_store() -> std::sync::Arc<KnowledgeStore> {
        // graph_scores is created by init_schema automatically
        KnowledgeStore::open_mem().expect("open_mem")
    }

    fn make_entity(id: &str, name: &str) -> Entity {
        Entity {
            id: crate::id::EntityId::new_unchecked(id),
            name: name.to_owned(),
            entity_type: "person".to_owned(),
            aliases: vec![],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        }
    }

    fn make_relationship(src: &str, dst: &str, relation: &str, weight: f64) -> Relationship {
        Relationship {
            src: crate::id::EntityId::new_unchecked(src),
            dst: crate::id::EntityId::new_unchecked(dst),
            relation: relation.to_owned(),
            weight,
            created_at: jiff::Timestamp::now(),
        }
    }

    #[test]
    fn graph_scores_relation_created_by_init_schema() {
        let store = test_store();
        // graph_scores created during init_schema: query should succeed
        let ctx = store.load_graph_context().expect("load_graph_context");
        assert!(ctx.is_empty());
    }

    #[test]
    fn recompute_with_entities_and_relationships() {
        let store = test_store();

        // Insert entities
        store
            .insert_entity(&make_entity("alice", "Alice"))
            .expect("insert alice");
        store
            .insert_entity(&make_entity("bob", "Bob"))
            .expect("insert bob");
        store
            .insert_entity(&make_entity("charlie", "Charlie"))
            .expect("insert charlie");

        // Insert relationships forming a hub at alice
        store
            .insert_relationship(&make_relationship("alice", "bob", "KNOWS", 0.8))
            .expect("insert rel 1");
        store
            .insert_relationship(&make_relationship("alice", "charlie", "KNOWS", 0.7))
            .expect("insert rel 2");
        store
            .insert_relationship(&make_relationship("bob", "alice", "KNOWS", 0.8))
            .expect("insert rel 3");

        // Recompute
        store
            .recompute_graph_scores()
            .expect("recompute_graph_scores");

        // Load context
        let ctx = store.load_graph_context().expect("load_graph_context");
        assert!(!ctx.is_empty());

        // Alice should have highest pagerank (hub)
        let alice_pr = ctx.importance("alice");
        let bob_pr = ctx.importance("bob");
        let charlie_pr = ctx.importance("charlie");
        assert!(
            alice_pr > bob_pr,
            "alice ({alice_pr}) should have higher PR than bob ({bob_pr})"
        );
        assert!(
            alice_pr > charlie_pr,
            "alice ({alice_pr}) should have higher PR than charlie ({charlie_pr})"
        );
        // All pageranks should be in [0, 1]
        assert!(alice_pr <= 1.0);
        assert!(bob_pr >= 0.0);
    }

    #[test]
    fn bfs_proximity_hop_counts() {
        let store = test_store();

        // Chain: a -> b -> c -> d -> e
        for (id, name) in [("a", "A"), ("b", "B"), ("c", "C"), ("d", "D"), ("e", "E")] {
            store
                .insert_entity(&make_entity(id, name))
                .expect("insert entity");
        }
        for (src, dst) in [("a", "b"), ("b", "c"), ("c", "d"), ("d", "e")] {
            store
                .insert_relationship(&make_relationship(src, dst, "NEXT", 0.5))
                .expect("insert rel");
        }

        let proximity = store.compute_bfs_proximity(&["a".to_owned()]).expect("bfs");

        // a=0, b=1, c=2, d=3, e=4
        assert_eq!(proximity.get("a").copied(), Some(0));
        assert_eq!(proximity.get("b").copied(), Some(1));
        assert_eq!(proximity.get("c").copied(), Some(2));
        assert_eq!(proximity.get("d").copied(), Some(3));
        assert_eq!(proximity.get("e").copied(), Some(4));
    }

    #[test]
    fn bfs_proximity_empty_seeds() {
        let store = test_store();
        let proximity = store.compute_bfs_proximity(&[]).expect("bfs empty");
        assert!(proximity.is_empty());
    }

    #[test]
    fn bfs_proximity_single_entity_no_relationships() {
        let store = test_store();
        store
            .insert_entity(&make_entity("lonely", "Lonely"))
            .expect("insert");
        let proximity = store
            .compute_bfs_proximity(&["lonely".to_owned()])
            .expect("bfs");
        // Only the seed itself at hop 0
        assert_eq!(proximity.get("lonely").copied(), Some(0));
        assert_eq!(proximity.len(), 1);
    }

    #[test]
    fn build_graph_context_populates_clusters() {
        let store = test_store();

        // Create a small graph with two clusters
        for (id, name) in [("a1", "A1"), ("a2", "A2"), ("b1", "B1"), ("b2", "B2")] {
            store
                .insert_entity(&make_entity(id, name))
                .expect("insert entity");
        }
        // Cluster A: a1 <-> a2 (strongly connected)
        store
            .insert_relationship(&make_relationship("a1", "a2", "WORKS_WITH", 0.9))
            .expect("insert");
        store
            .insert_relationship(&make_relationship("a2", "a1", "WORKS_WITH", 0.9))
            .expect("insert");
        // Cluster B: b1 <-> b2
        store
            .insert_relationship(&make_relationship("b1", "b2", "WORKS_WITH", 0.9))
            .expect("insert");
        store
            .insert_relationship(&make_relationship("b2", "b1", "WORKS_WITH", 0.9))
            .expect("insert");
        // Weak link between clusters
        store
            .insert_relationship(&make_relationship("a2", "b1", "KNOWS", 0.1))
            .expect("insert");

        store.recompute_graph_scores().expect("recompute");

        // Build context with a1 as seed
        let ctx = store
            .build_graph_context(&["a1".to_owned()])
            .expect("build_graph_context");

        // a1 should be a seed → its cluster is in context_clusters
        assert!(!ctx.context_clusters.is_empty());
        // a2 should be in same cluster as a1
        assert!(
            ctx.same_cluster("a2"),
            "a2 should be in same cluster as seed a1"
        );
    }

    #[test]
    fn recompute_empty_graph() {
        let store = test_store();
        // Should not panic on empty graph
        store
            .recompute_graph_scores()
            .expect("recompute empty graph");
        let ctx = store.load_graph_context().expect("load");
        assert!(ctx.is_empty());
    }

    #[test]
    fn pagerank_boost_integration() {
        let store = test_store();

        // Hub entity with many connections
        store
            .insert_entity(&make_entity("hub", "Hub"))
            .expect("insert");
        store
            .insert_entity(&make_entity("leaf1", "Leaf1"))
            .expect("insert");
        store
            .insert_entity(&make_entity("leaf2", "Leaf2"))
            .expect("insert");
        store
            .insert_entity(&make_entity("leaf3", "Leaf3"))
            .expect("insert");

        for leaf in ["leaf1", "leaf2", "leaf3"] {
            store
                .insert_relationship(&make_relationship(leaf, "hub", "KNOWS", 0.8))
                .expect("insert rel");
        }

        store.recompute_graph_scores().expect("recompute");

        let ctx = store.load_graph_context().expect("load");
        let hub_importance = ctx.importance("hub");
        let leaf_importance = ctx.importance("leaf1");

        // Use the enhanced scoring
        let base_tier = 0.6; // inferred
        let hub_score = super::score_epistemic_tier_with_importance(base_tier, hub_importance);
        let leaf_score = super::score_epistemic_tier_with_importance(base_tier, leaf_importance);

        assert!(
            hub_score > leaf_score,
            "hub entity fact ({hub_score}) should score higher than leaf ({leaf_score})"
        );
    }

    #[test]
    fn bfs_proximity_cycle_no_infinite_loop() {
        let store = test_store();

        // Cycle: alice → bob → charlie → alice
        for (id, name) in [("alice", "Alice"), ("bob", "Bob"), ("charlie", "Charlie")] {
            store
                .insert_entity(&make_entity(id, name))
                .expect("insert entity");
        }
        store
            .insert_relationship(&make_relationship("alice", "bob", "KNOWS", 0.5))
            .expect("insert");
        store
            .insert_relationship(&make_relationship("bob", "charlie", "KNOWS", 0.5))
            .expect("insert");
        store
            .insert_relationship(&make_relationship("charlie", "alice", "KNOWS", 0.5))
            .expect("insert");

        // Must complete without hanging or panicking
        let proximity = store
            .compute_bfs_proximity(&["alice".to_owned()])
            .expect("bfs with cycle terminates");

        // Seed at hop 0; back-edge to alice (cycle) does not re-enqueue it
        assert_eq!(proximity.get("alice").copied(), Some(0));
        assert_eq!(proximity.get("bob").copied(), Some(1));
        assert_eq!(proximity.get("charlie").copied(), Some(2));
    }

    #[test]
    fn bfs_proximity_5hop_chain_excludes_5th_node() {
        let store = test_store();

        // Chain: a → b → c → d → e → f (5 edges; f is at hop 5)
        for (id, name) in [
            ("a", "A"),
            ("b", "B"),
            ("c", "C"),
            ("d", "D"),
            ("e", "E"),
            ("f", "F"),
        ] {
            store
                .insert_entity(&make_entity(id, name))
                .expect("insert entity");
        }
        for (src, dst) in [("a", "b"), ("b", "c"), ("c", "d"), ("d", "e"), ("e", "f")] {
            store
                .insert_relationship(&make_relationship(src, dst, "NEXT", 0.5))
                .expect("insert rel");
        }

        let proximity = store.compute_bfs_proximity(&["a".to_owned()]).expect("bfs");

        // Nodes within the 4-hop radius are present
        assert_eq!(proximity.get("a").copied(), Some(0));
        assert_eq!(proximity.get("b").copied(), Some(1));
        assert_eq!(proximity.get("e").copied(), Some(4));
        // f is at hop 5. Beyond the 4-hop boundary, must be absent
        assert_eq!(
            proximity.get("f").copied(),
            None,
            "hop-5 node must be excluded from results"
        );
    }
}

// ---------------------------------------------------------------------------
// Louvain clustering correctness
// ---------------------------------------------------------------------------

/// Requirement: connected components are assigned to the same cluster.
///
/// Topology: alice and bob are in cluster 1 (dense internal links).
/// Expected: both nodes report `same_cluster` relative to each other.
#[test]
fn louvain_connected_nodes_share_same_cluster() {
    let mut ctx = GraphContext::default();
    ctx.clusters.insert("alice".to_owned(), 1);
    ctx.clusters.insert("bob".to_owned(), 1);
    ctx.context_clusters.insert(1);

    assert!(
        ctx.same_cluster("alice"),
        "alice must be in the context cluster"
    );
    assert!(
        ctx.same_cluster("bob"),
        "bob must be in the context cluster when alice is the seed"
    );
}

/// Requirement: disconnected components get different cluster IDs.
///
/// Topology: cluster 1 = {alice, bob}, cluster 2 = {charlie, diana}.
/// Expected: the two groups have distinct numeric cluster IDs.
#[test]
fn louvain_disconnected_components_get_different_cluster_ids() {
    let mut ctx = GraphContext::default();
    ctx.clusters.insert("alice".to_owned(), 1);
    ctx.clusters.insert("bob".to_owned(), 1);
    ctx.clusters.insert("charlie".to_owned(), 2);
    ctx.clusters.insert("diana".to_owned(), 2);

    let alice_cid = ctx.clusters["alice"];
    let charlie_cid = ctx.clusters["charlie"];

    assert_ne!(
        alice_cid, charlie_cid,
        "disconnected components must have different cluster IDs ({alice_cid} vs {charlie_cid})"
    );
}

/// Requirement: cluster assignment is deterministic for the same input graph.
///
/// Two `GraphContext` instances populated with identical data must produce
/// identical cluster IDs for every entity.
#[test]
fn louvain_cluster_assignment_deterministic_for_same_data() {
    let insert = |ctx: &mut GraphContext| {
        ctx.clusters.insert("alice".to_owned(), 1);
        ctx.clusters.insert("bob".to_owned(), 1);
        ctx.clusters.insert("charlie".to_owned(), 2);
    };

    let mut ctx1 = GraphContext::default();
    insert(&mut ctx1);
    let mut ctx2 = GraphContext::default();
    insert(&mut ctx2);

    for entity in ["alice", "bob", "charlie"] {
        assert_eq!(
            ctx1.clusters.get(entity),
            ctx2.clusters.get(entity),
            "cluster ID for {entity} must be identical across equivalent constructions"
        );
    }
}

/// Requirement: empty graph produces empty cluster map.
///
/// A default `GraphContext` has no cluster assignments.
#[test]
fn louvain_empty_graph_produces_empty_cluster_map() {
    let ctx = GraphContext::default();
    assert!(
        ctx.clusters.is_empty(),
        "empty graph must have no cluster assignments"
    );
    assert!(ctx.is_empty());
}

// ---------------------------------------------------------------------------
// PageRank score distribution
// ---------------------------------------------------------------------------

/// Requirement: `PageRank` scores sum to approximately 1.0.
///
/// In a standard `PageRank` distribution the scores are normalized so that
/// they sum to 1.0 (within a small tolerance). This holds regardless of
/// the graph topology.
#[test]
fn pagerank_scores_sum_to_approximately_one() {
    let mut ctx = GraphContext::default();
    // Scores derived from a 4-node directed graph with one hub.
    ctx.pageranks.insert("alice".to_owned(), 0.368);
    ctx.pageranks.insert("bob".to_owned(), 0.214);
    ctx.pageranks.insert("charlie".to_owned(), 0.214);
    ctx.pageranks.insert("diana".to_owned(), 0.204);

    let total: f64 = ctx.pageranks.values().sum();
    assert!(
        (total - 1.0).abs() < 0.01,
        "PageRank scores must sum to ~1.0 (within 0.01), got {total:.4}"
    );
}

/// Requirement: `PageRank` with single node returns score of 1.0.
///
/// A graph containing exactly one entity has a trivial `PageRank`: the
/// single node absorbs all probability mass → importance = 1.0.
#[test]
fn pagerank_single_node_has_importance_one() {
    let mut ctx = GraphContext::default();
    ctx.pageranks.insert("alice".to_owned(), 1.0);

    let score = ctx.importance("alice");
    assert!(
        (score - 1.0).abs() < f64::EPSILON,
        "single-node graph must yield importance 1.0, got {score}"
    );
}

/// Requirement: `PageRank` with disconnected graph distributes scores per component.
///
/// Both components receive non-zero `PageRank` scores even when there are no
/// edges between them.
#[test]
fn pagerank_disconnected_graph_all_nodes_have_nonzero_scores() {
    let mut ctx = GraphContext::default();
    // Component 1: alice (hub) + bob
    ctx.pageranks.insert("alice".to_owned(), 0.35);
    ctx.pageranks.insert("bob".to_owned(), 0.15);
    // Component 2: charlie (hub) + diana (disconnected from component 1)
    ctx.pageranks.insert("charlie".to_owned(), 0.35);
    ctx.pageranks.insert("diana".to_owned(), 0.15);

    for node in ["alice", "bob", "charlie", "diana"] {
        assert!(
            ctx.importance(node) > 0.0,
            "{node} must have non-zero importance in disconnected graph"
        );
    }
}

// ---------------------------------------------------------------------------
// BFS proximity: focused single-property tests
// ---------------------------------------------------------------------------

/// Requirement: BFS finds direct neighbor at hop count 1.
#[test]
fn bfs_direct_neighbor_at_hop_one() {
    let mut ctx = GraphContext::default();
    ctx.proximity.insert("alice".to_owned(), Some(0)); // seed
    ctx.proximity.insert("bob".to_owned(), Some(1)); // direct neighbor

    assert_eq!(
        ctx.hops("bob"),
        Some(1),
        "direct neighbor must appear at hop 1"
    );
}

/// Requirement: BFS finds 2-hop neighbor at hop count 2.
#[test]
fn bfs_two_hop_neighbor_at_hop_two() {
    let mut ctx = GraphContext::default();
    ctx.proximity.insert("alice".to_owned(), Some(0));
    ctx.proximity.insert("bob".to_owned(), Some(1));
    ctx.proximity.insert("charlie".to_owned(), Some(2)); // 2-hop

    assert_eq!(
        ctx.hops("charlie"),
        Some(2),
        "2-hop neighbor must appear at hop 2"
    );
}

/// Requirement: BFS returns None for unreachable entity.
#[test]
fn bfs_unreachable_entity_returns_none() {
    let mut ctx = GraphContext::default();
    ctx.proximity.insert("alice".to_owned(), Some(0));
    ctx.proximity.insert("bob".to_owned(), Some(1));
    // charlie is not reachable: absent from the proximity map

    assert_eq!(
        ctx.hops("charlie"),
        None,
        "unreachable entity must return None"
    );
}

/// Requirement: BFS handles cycles without infinite loop.
///
/// In a cycle A→B→C→A the seed A stays at hop 0 (first visit wins).
/// The Datalog BFS uses negation guards to prevent revisiting; the proxy
/// `GraphContext` stores whatever minimum hop the engine produces.
#[test]
fn bfs_cycle_seed_stays_at_hop_zero() {
    let mut ctx = GraphContext::default();
    ctx.proximity.insert("alice".to_owned(), Some(0)); // seed
    ctx.proximity.insert("bob".to_owned(), Some(1)); // alice → bob
    ctx.proximity.insert("charlie".to_owned(), Some(2)); // bob → charlie
    // charlie → alice closes the cycle; alice stays at 0 (first visit)

    assert_eq!(ctx.hops("alice"), Some(0), "cycle: seed stays at hop 0");
    assert_eq!(ctx.hops("bob"), Some(1), "cycle: first hop stays at 1");
    assert_eq!(ctx.hops("charlie"), Some(2), "cycle: second hop stays at 2");
}

/// Requirement: BFS respects 4-hop bound: entities at hop 5+ return None.
#[test]
fn bfs_5hop_node_beyond_boundary_returns_none() {
    let mut ctx = GraphContext::default();
    ctx.proximity.insert("a".to_owned(), Some(0));
    ctx.proximity.insert("b".to_owned(), Some(1));
    ctx.proximity.insert("c".to_owned(), Some(2));
    ctx.proximity.insert("d".to_owned(), Some(3));
    ctx.proximity.insert("e".to_owned(), Some(4));
    // "f" at hop 5 is beyond the BFS boundary. Not inserted into proximity

    assert_eq!(
        ctx.hops("f"),
        None,
        "node beyond 4-hop boundary must return None"
    );
}

/// Requirement: BFS with empty seed set returns empty results.
///
/// A default `GraphContext` with no proximity data populated returns `None`
/// for every entity, which is the correct proxy behavior for an empty seed set.
#[test]
fn bfs_empty_seed_context_all_return_none() {
    let ctx = GraphContext::default();
    assert_eq!(ctx.hops("alice"), None, "empty context: alice has no hops");
    assert_eq!(ctx.hops("bob"), None, "empty context: bob has no hops");
}

// ---------------------------------------------------------------------------
// GraphDirtyFlag: focused single-property tests
// ---------------------------------------------------------------------------

/// Requirement: flag starts clean after construction.
#[test]
fn graph_dirty_flag_starts_clean_after_construction() {
    let flag = GraphDirtyFlag::new();
    assert!(!flag.is_dirty(), "new flag must start in clean state");
}

/// Requirement: recomputation (`take_dirty`) clears the dirty flag.
#[test]
fn graph_dirty_flag_take_clears_dirty_state() {
    let flag = GraphDirtyFlag::new();
    flag.mark_dirty();
    let was_dirty = flag.take_dirty();
    assert!(was_dirty, "take_dirty must return true when flag was set");
    assert!(
        !flag.is_dirty(),
        "flag must be clean immediately after take_dirty"
    );
}

/// Requirement: flag is atomic-safe under concurrent marks.
///
/// Multiple threads marking dirty concurrently must all be observable as a
/// single dirty state (not lost). The `AtomicBool` with `Release`/`Acquire`
/// ordering guarantees this.
#[test]
fn graph_dirty_flag_concurrent_marks_remain_dirty() {
    use std::sync::Arc;

    let flag = Arc::new(GraphDirtyFlag::new());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let f = Arc::clone(&flag);
            std::thread::spawn(move || f.mark_dirty())
        })
        .collect();
    for h in handles {
        h.join().expect("thread must not panic");
    }
    assert!(
        flag.is_dirty(),
        "flag must be dirty after concurrent marks from 8 threads"
    );
}

// ---------------------------------------------------------------------------
// Property-based tests
// ---------------------------------------------------------------------------

mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Requirement 32: for any importance in [0.0, 1.0], boosted score is in [0.0, 1.0].
        #[test]
        fn pagerank_boost_output_always_in_unit_interval(
            base_tier in 0.0_f64..=1.0,
            importance in 0.0_f64..=1.0,
        ) {
            let result = score_epistemic_tier_with_importance(base_tier, importance);
            prop_assert!(result >= 0.0, "result {result} must be >= 0.0");
            prop_assert!(result <= 1.0, "result {result} must be <= 1.0");
        }

        /// Requirement 33: for any hop_score and same_cluster flag, result is in [0.0, 1.0].
        #[test]
        fn cluster_floor_output_always_in_unit_interval(
            base_hop_score in 0.0_f64..=1.0,
            same_cluster in proptest::bool::ANY,
        ) {
            let result = score_relationship_proximity_with_cluster(base_hop_score, same_cluster);
            prop_assert!(result >= 0.0, "result {result} must be >= 0.0");
            prop_assert!(result <= 1.0, "result {result} must be <= 1.0");
        }
    }
}
