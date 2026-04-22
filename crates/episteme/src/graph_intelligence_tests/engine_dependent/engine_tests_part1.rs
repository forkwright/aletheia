//! (Part of split `engine_dependent` — see parent mod for context.)

use crate::knowledge::{Entity, Relationship};
use crate::knowledge_store::KnowledgeStore;

fn test_store() -> std::sync::Arc<KnowledgeStore> {
    KnowledgeStore::open_mem().expect("open_mem")
}

fn make_entity(id: &str, name: &str) -> Entity {
    Entity {
        id: crate::id::EntityId::new(id).expect("valid test id"),
        name: name.to_owned(),
        entity_type: "person".to_owned(),
        aliases: vec![],
        created_at: jiff::Timestamp::now(),
        updated_at: jiff::Timestamp::now(),
    }
}

fn make_relationship(src: &str, dst: &str, relation: &str, weight: f64) -> Relationship {
    Relationship {
        src: crate::id::EntityId::new(src).expect("valid test id"),
        dst: crate::id::EntityId::new(dst).expect("valid test id"),
        relation: relation.to_owned(),
        weight,
        created_at: jiff::Timestamp::now(),
    }
}

#[test]
fn graph_scores_relation_created_by_init_schema() {
    let store = test_store();
    let ctx = store.load_graph_context().expect("load_graph_context");
    assert!(
        ctx.is_empty(),
        "freshly created store should have empty graph context"
    );
}

#[test]
fn recompute_with_entities_and_relationships() {
    let store = test_store();

    store
        .insert_entity(&make_entity("alice", "Alice"))
        .expect("insert alice");
    store
        .insert_entity(&make_entity("bob", "Bob"))
        .expect("insert bob");
    store
        .insert_entity(&make_entity("charlie", "Charlie"))
        .expect("insert charlie");

    store
        .insert_relationship(&make_relationship("alice", "bob", "KNOWS", 0.8))
        .expect("insert rel 1");
    store
        .insert_relationship(&make_relationship("alice", "charlie", "KNOWS", 0.7))
        .expect("insert rel 2");
    store
        .insert_relationship(&make_relationship("bob", "alice", "KNOWS", 0.8))
        .expect("insert rel 3");

    store
        .recompute_graph_scores()
        .expect("recompute_graph_scores");

    let ctx = store.load_graph_context().expect("load_graph_context");
    assert!(
        !ctx.is_empty(),
        "graph context should be populated after recompute"
    );

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
    assert!(alice_pr <= 1.0, "alice pagerank should be at most 1.0");
    assert!(bob_pr >= 0.0, "bob pagerank should be non-negative");
}

#[test]
fn bfs_proximity_hop_counts() {
    let store = test_store();

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

    assert_eq!(
        proximity.get("a").copied(),
        Some(0),
        "seed a should be at hop 0"
    );
    assert_eq!(
        proximity.get("b").copied(),
        Some(1),
        "node b should be at hop 1"
    );
    assert_eq!(
        proximity.get("c").copied(),
        Some(2),
        "node c should be at hop 2"
    );
    assert_eq!(
        proximity.get("d").copied(),
        Some(3),
        "node d should be at hop 3"
    );
    assert_eq!(
        proximity.get("e").copied(),
        Some(4),
        "node e should be at hop 4"
    );
}

#[test]
fn bfs_proximity_empty_seeds() {
    let store = test_store();
    let proximity = store.compute_bfs_proximity(&[]).expect("bfs empty");
    assert!(
        proximity.is_empty(),
        "empty seed set should produce empty proximity map"
    );
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
    assert_eq!(
        proximity.get("lonely").copied(),
        Some(0),
        "isolated seed should be at hop 0"
    );
    assert_eq!(
        proximity.len(),
        1,
        "isolated entity should produce proximity map of size 1"
    );
}

#[test]
fn build_graph_context_populates_clusters() {
    let store = test_store();

    for (id, name) in [("a1", "A1"), ("a2", "A2"), ("b1", "B1"), ("b2", "B2")] {
        store
            .insert_entity(&make_entity(id, name))
            .expect("insert entity");
    }
    store
        .insert_relationship(&make_relationship("a1", "a2", "WORKS_WITH", 0.9))
        .expect("insert");
    store
        .insert_relationship(&make_relationship("a2", "a1", "WORKS_WITH", 0.9))
        .expect("insert");
    store
        .insert_relationship(&make_relationship("b1", "b2", "WORKS_WITH", 0.9))
        .expect("insert");
    store
        .insert_relationship(&make_relationship("b2", "b1", "WORKS_WITH", 0.9))
        .expect("insert");
    store
        .insert_relationship(&make_relationship("a2", "b1", "KNOWS", 0.1))
        .expect("insert");

    store.recompute_graph_scores().expect("recompute");

    let ctx = store
        .build_graph_context(&["a1".to_owned()])
        .expect("build_graph_context");

    assert!(
        !ctx.context_clusters.is_empty(),
        "context clusters should be populated when seed entity has a cluster"
    );
    assert!(
        ctx.same_cluster("a2"),
        "a2 should be in same cluster as seed a1"
    );
}

#[test]
fn build_graph_context_always_loads_graph_data() {
    let store = test_store();

    for (id, name) in [("x1", "X1"), ("x2", "X2")] {
        store
            .insert_entity(&make_entity(id, name))
            .expect("insert entity");
    }
    store
        .insert_relationship(&make_relationship("x1", "x2", "LINKED", 0.8))
        .expect("insert relationship");
    store.recompute_graph_scores().expect("recompute");

    let ctx = store
        .build_graph_context(&["x1".to_owned()])
        .expect("build_graph_context");
    assert!(
        !ctx.is_empty(),
        "graph context should load data regardless of recall weight (#3432)"
    );
    assert!(
        !ctx.proximity.is_empty(),
        "BFS proximity should be computed"
    );
    // Chain lengths are computed from supersession chains in the facts
    // table; this test only inserts entities/relationships, so they
    // will legitimately be empty.
}

#[test]
fn recompute_empty_graph() {
    let store = test_store();
    store
        .recompute_graph_scores()
        .expect("recompute empty graph");
    let ctx = store.load_graph_context().expect("load");
    assert!(
        ctx.is_empty(),
        "recomputing empty graph should produce empty context"
    );
}

#[test]
fn pagerank_boost_integration() {
    let store = test_store();

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

    let base_tier = 0.6; // inferred
    let hub_score =
        crate::graph_intelligence::score_epistemic_tier_with_importance(base_tier, hub_importance);
    let leaf_score =
        crate::graph_intelligence::score_epistemic_tier_with_importance(base_tier, leaf_importance);

    assert!(
        hub_score > leaf_score,
        "hub entity fact ({hub_score}) should score higher than leaf ({leaf_score})"
    );
}
