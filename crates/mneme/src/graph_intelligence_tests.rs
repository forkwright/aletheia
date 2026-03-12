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
    let enhanced_access =
        score_access_with_evolution(base_access, ctx.chain_length("any_fact"));
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

// --- Engine-dependent tests ---

#[cfg(feature = "mneme-engine")]
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
        // graph_scores created during init_schema — query should succeed
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
        let leaf_score =
            super::score_epistemic_tier_with_importance(base_tier, leaf_importance);

        assert!(
            hub_score > leaf_score,
            "hub entity fact ({hub_score}) should score higher than leaf ({leaf_score})"
        );
    }
}
