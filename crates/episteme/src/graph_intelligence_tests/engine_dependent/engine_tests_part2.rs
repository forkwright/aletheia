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
fn bfs_proximity_cycle_no_infinite_loop() {
    let store = test_store();

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

    let proximity = store
        .compute_bfs_proximity(&["alice".to_owned()])
        .expect("bfs with cycle terminates");

    assert_eq!(
        proximity.get("alice").copied(),
        Some(0),
        "seed alice should be at hop 0 even in cycle"
    );
    assert_eq!(
        proximity.get("bob").copied(),
        Some(1),
        "bob should be at hop 1 in cycle"
    );
    assert_eq!(
        proximity.get("charlie").copied(),
        Some(2),
        "charlie should be at hop 2 in cycle"
    );
}

#[test]
fn bfs_proximity_5hop_chain_excludes_5th_node() {
    let store = test_store();

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

    assert_eq!(
        proximity.get("a").copied(),
        Some(0),
        "seed a should be at hop 0"
    );
    assert_eq!(proximity.get("b").copied(), Some(1), "b should be at hop 1");
    assert_eq!(
        proximity.get("e").copied(),
        Some(4),
        "e should be at hop 4 (boundary)"
    );
    assert_eq!(
        proximity.get("f").copied(),
        None,
        "hop-5 node must be excluded from results"
    );
}

/// Betweenness centrality: in a star graph the hub must have the highest score.
#[test]
fn centrality_hub_has_highest_score() {
    let store = test_store();

    for (id, name) in [
        ("hub", "Hub"),
        ("leaf1", "Leaf1"),
        ("leaf2", "Leaf2"),
        ("leaf3", "Leaf3"),
    ] {
        store
            .insert_entity(&make_entity(id, name))
            .expect("insert entity");
    }

    // Bidirectional star so the graph is undirected for centrality.
    for leaf in ["leaf1", "leaf2", "leaf3"] {
        store
            .insert_relationship(&make_relationship("hub", leaf, "CONNECTS", 0.8))
            .expect("insert rel");
        store
            .insert_relationship(&make_relationship(leaf, "hub", "CONNECTS", 0.8))
            .expect("insert rel");
    }

    let centrality = store.compute_centrality();
    let hub_id = crate::id::EntityId::new("hub").expect("valid id");
    let leaf1_id = crate::id::EntityId::new("leaf1").expect("valid id");

    let hub_score = centrality.get(&hub_id).copied().unwrap_or(0.0);
    let leaf1_score = centrality.get(&leaf1_id).copied().unwrap_or(0.0);

    assert!(
        hub_score > leaf1_score,
        "hub ({hub_score}) must have higher centrality than leaf ({leaf1_score})"
    );
}

/// Shortest path: trivial 3-hop directed chain.
#[test]
fn shortest_path_three_hop_chain() {
    let store = test_store();

    for (id, name) in [("a", "A"), ("b", "B"), ("c", "C"), ("d", "D")] {
        store
            .insert_entity(&make_entity(id, name))
            .expect("insert entity");
    }

    store
        .insert_relationship(&make_relationship("a", "b", "NEXT", 0.5))
        .expect("insert rel");
    store
        .insert_relationship(&make_relationship("b", "c", "NEXT", 0.5))
        .expect("insert rel");
    store
        .insert_relationship(&make_relationship("c", "d", "NEXT", 0.5))
        .expect("insert rel");

    let path = store.shortest_path(
        &crate::id::EntityId::new("a").expect("valid id"),
        &crate::id::EntityId::new("d").expect("valid id"),
    );

    assert!(path.is_some(), "path must exist");
    let path = path.expect("path");
    assert_eq!(path.len(), 4, "path should be 4 nodes: a-b-c-d");
    assert_eq!(path[0].as_str(), "a");
    assert_eq!(path[1].as_str(), "b");
    assert_eq!(path[2].as_str(), "c");
    assert_eq!(path[3].as_str(), "d");
}

/// Shortest path returns None when no directed path exists.
#[test]
fn shortest_path_no_path_returns_none() {
    let store = test_store();

    for (id, name) in [("a", "A"), ("b", "B")] {
        store
            .insert_entity(&make_entity(id, name))
            .expect("insert entity");
    }

    // No relationships → no path.
    let path = store.shortest_path(
        &crate::id::EntityId::new("a").expect("valid id"),
        &crate::id::EntityId::new("b").expect("valid id"),
    );
    assert!(path.is_none(), "no path should exist");
}

/// Shortest path from a node to itself returns a singleton.
#[test]
fn shortest_path_same_node_singleton() {
    let store = test_store();
    store
        .insert_entity(&make_entity("a", "A"))
        .expect("insert entity");

    let path = store.shortest_path(
        &crate::id::EntityId::new("a").expect("valid id"),
        &crate::id::EntityId::new("a").expect("valid id"),
    );
    assert!(path.is_some(), "self-path should exist");
    let path = path.expect("path");
    assert_eq!(path.len(), 1);
    assert_eq!(path[0].as_str(), "a");
}

/// Connected components: two disconnected subgraphs → 2 components.
#[test]
fn connected_components_two_disconnected_subgraphs() {
    let store = test_store();

    for (id, name) in [("a1", "A1"), ("a2", "A2"), ("b1", "B1"), ("b2", "B2")] {
        store
            .insert_entity(&make_entity(id, name))
            .expect("insert entity");
    }

    store
        .insert_relationship(&make_relationship("a1", "a2", "LINK", 0.8))
        .expect("insert rel");
    store
        .insert_relationship(&make_relationship("a2", "a1", "LINK", 0.8))
        .expect("insert rel");
    store
        .insert_relationship(&make_relationship("b1", "b2", "LINK", 0.8))
        .expect("insert rel");
    store
        .insert_relationship(&make_relationship("b2", "b1", "LINK", 0.8))
        .expect("insert rel");

    let components = store.connected_components();
    assert_eq!(components.len(), 2, "should have exactly 2 components");

    let mut component_ids: Vec<Vec<&str>> = components
        .iter()
        .map(|c| c.iter().map(eidos::id::EntityId::as_str).collect())
        .collect();
    for c in &mut component_ids {
        c.sort_unstable();
    }
    component_ids.sort();

    assert_eq!(component_ids[0], vec!["a1", "a2"]);
    assert_eq!(component_ids[1], vec!["b1", "b2"]);
}

/// Connected components: isolated entity is its own component.
#[test]
fn connected_components_isolated_entity_singleton() {
    let store = test_store();
    store
        .insert_entity(&make_entity("lonely", "Lonely"))
        .expect("insert entity");

    let components = store.connected_components();
    assert_eq!(
        components.len(),
        1,
        "isolated entity should be one component"
    );
    assert_eq!(
        components[0]
            .iter()
            .map(eidos::id::EntityId::as_str)
            .collect::<Vec<_>>(),
        vec!["lonely"]
    );
}

/// Proximity decay: score drops geometrically with distance.
#[test]
fn bfs_proximity_decay_geometric_drop() {
    let store = test_store();

    for (id, name) in [("a", "A"), ("b", "B"), ("c", "C"), ("d", "D")] {
        store
            .insert_entity(&make_entity(id, name))
            .expect("insert entity");
    }

    store
        .insert_relationship(&make_relationship("a", "b", "NEXT", 0.5))
        .expect("insert rel");
    store
        .insert_relationship(&make_relationship("b", "c", "NEXT", 0.5))
        .expect("insert rel");
    store
        .insert_relationship(&make_relationship("c", "d", "NEXT", 0.5))
        .expect("insert rel");

    let decay = 0.5;
    let scores = store
        .compute_bfs_proximity_decay(&[crate::id::EntityId::new("a").expect("valid id")], decay);

    let a_id = crate::id::EntityId::new("a").expect("valid id");
    let b_id = crate::id::EntityId::new("b").expect("valid id");
    let c_id = crate::id::EntityId::new("c").expect("valid id");
    let d_id = crate::id::EntityId::new("d").expect("valid id");

    let a_score = scores.get(&a_id).copied().unwrap_or(0.0);
    let b_score = scores.get(&b_id).copied().unwrap_or(0.0);
    let c_score = scores.get(&c_id).copied().unwrap_or(0.0);
    let d_score = scores.get(&d_id).copied().unwrap_or(0.0);

    assert!(
        (a_score - 1.0).abs() < f64::EPSILON,
        "seed score must be 1.0, got {a_score}"
    );
    assert!(
        (b_score - 0.5).abs() < f64::EPSILON,
        "distance 1 score must be 0.5, got {b_score}"
    );
    assert!(
        (c_score - 0.25).abs() < f64::EPSILON,
        "distance 2 score must be 0.25, got {c_score}"
    );
    assert!(
        (d_score - 0.125).abs() < f64::EPSILON,
        "distance 3 score must be 0.125, got {d_score}"
    );
}

/// Proximity decay with empty seeds returns empty map.
#[test]
fn bfs_proximity_decay_empty_seeds() {
    let store = test_store();
    let scores = store.compute_bfs_proximity_decay(&[], 0.5);
    assert!(
        scores.is_empty(),
        "empty seed set should produce empty scores"
    );
}
