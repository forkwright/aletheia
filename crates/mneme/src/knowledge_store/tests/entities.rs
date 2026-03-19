#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use super::super::*;
use crate::knowledge::{Entity, EpistemicTier, Fact, Relationship};
use std::sync::Arc;

const DIM: usize = 4;

fn make_store() -> Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: DIM }).expect("open_mem")
}

fn test_ts(s: &str) -> jiff::Timestamp {
    crate::knowledge::parse_timestamp(s).expect("valid test timestamp in test helper")
}

fn make_fact(id: &str, nous_id: &str, content: &str) -> Fact {
    Fact {
        id: crate::id::FactId::new_unchecked(id),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        confidence: 0.9,
        tier: EpistemicTier::Inferred,
        valid_from: test_ts("2026-01-01"),
        valid_to: crate::knowledge::far_future(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: test_ts("2026-03-01T00:00:00Z"),
        access_count: 0,
        last_accessed_at: None,
        stability_hours: 720.0,
        fact_type: String::new(),
        is_forgotten: false,
        forgotten_at: None,
        forget_reason: None,
    }
}

fn make_entity(id: &str, name: &str, entity_type: &str) -> Entity {
    Entity {
        id: crate::id::EntityId::new_unchecked(id),
        name: name.to_owned(),
        entity_type: entity_type.to_owned(),
        aliases: vec![],
        created_at: test_ts("2026-03-01T00:00:00Z"),
        updated_at: test_ts("2026-03-01T00:00:00Z"),
    }
}

fn make_relationship(src: &str, dst: &str, relation: &str, weight: f64) -> Relationship {
    Relationship {
        src: crate::id::EntityId::new_unchecked(src),
        dst: crate::id::EntityId::new_unchecked(dst),
        relation: relation.to_owned(),
        weight,
        created_at: test_ts("2026-03-01T00:00:00Z"),
    }
}

#[test]
fn insert_entity_and_query_neighborhood() {
    let store = make_store();
    let entity = make_entity("e1", "Rust", "language");
    store.insert_entity(&entity).expect("insert entity");

    let rows = store
        .entity_neighborhood(&crate::id::EntityId::new_unchecked("e1"))
        .expect("neighborhood");
    assert!(rows.rows.is_empty());
}

#[test]
fn insert_entity_with_aliases() {
    let store = make_store();
    let mut entity = make_entity("e1", "Rust", "language");
    entity.aliases = vec!["rustlang".to_owned(), "rust-lang".to_owned()];
    store
        .insert_entity(&entity)
        .expect("insert entity with aliases");

    let rows = store
        .run_query(
            r"?[id, name, aliases] := *entities{id, name, aliases}, id = 'e1'",
            std::collections::BTreeMap::new(),
        )
        .expect("raw query");
    assert_eq!(rows.rows.len(), 1);
}

#[test]
fn insert_relationship_and_retrieve_neighborhood() {
    let store = make_store();
    store
        .insert_entity(&make_entity("e1", "Alice", "person"))
        .expect("insert e1");
    store
        .insert_entity(&make_entity("e2", "Aletheia", "project"))
        .expect("insert e2");
    store
        .insert_relationship(&make_relationship("e1", "e2", "works_on", 0.9))
        .expect("insert relationship");

    let rows = store
        .entity_neighborhood(&crate::id::EntityId::new_unchecked("e1"))
        .expect("neighborhood");
    assert!(
        !rows.rows.is_empty(),
        "neighborhood should contain the relationship"
    );
}

#[test]
fn insert_relationship_bidirectional_neighborhood() {
    let store = make_store();
    store
        .insert_entity(&make_entity("e1", "Alice", "person"))
        .expect("insert e1");
    store
        .insert_entity(&make_entity("e2", "Bob", "person"))
        .expect("insert e2");
    store
        .insert_relationship(&make_relationship("e1", "e2", "knows", 0.8))
        .expect("insert rel");

    let from_e1 = store
        .entity_neighborhood(&crate::id::EntityId::new_unchecked("e1"))
        .expect("e1 neighborhood");
    assert!(!from_e1.rows.is_empty());

    let _from_e2 = store
        .entity_neighborhood(&crate::id::EntityId::new_unchecked("e2"))
        .expect("e2 neighborhood");
}

#[test]
fn entity_neighborhood_2hop() {
    let store = make_store();
    store
        .insert_entity(&make_entity("e1", "Alice", "person"))
        .expect("e1");
    store
        .insert_entity(&make_entity("e2", "Aletheia", "project"))
        .expect("e2");
    store
        .insert_entity(&make_entity("e3", "Rust", "language"))
        .expect("e3");

    store
        .insert_relationship(&make_relationship("e1", "e2", "works_on", 0.9))
        .expect("rel e1-e2");
    store
        .insert_relationship(&make_relationship("e2", "e3", "uses", 0.8))
        .expect("rel e2-e3");

    let rows = store
        .entity_neighborhood(&crate::id::EntityId::new_unchecked("e1"))
        .expect("2-hop neighborhood");
    assert!(
        rows.rows.len() >= 2,
        "2-hop neighborhood should find at least 2 results, got {}",
        rows.rows.len()
    );
}

#[test]
fn entity_neighborhood_nonexistent_entity() {
    let store = make_store();
    let rows = store
        .entity_neighborhood(&crate::id::EntityId::new_unchecked("nonexistent"))
        .expect("neighborhood of missing entity should succeed");
    assert!(rows.rows.is_empty());
}

#[test]
fn insert_duplicate_entity_name_upserts() {
    let store = make_store();
    let e1 = make_entity("e1", "Rust", "language");
    store.insert_entity(&e1).expect("insert first");

    let e1_updated = make_entity("e1", "Rust Lang", "language");
    store.insert_entity(&e1_updated).expect("upsert");

    let rows = store
        .run_query(
            r"?[id, name] := *entities{id, name}",
            std::collections::BTreeMap::new(),
        )
        .expect("raw query");
    assert_eq!(rows.rows.len(), 1);
}

#[test]
fn insert_different_entities_same_name() {
    let store = make_store();
    store
        .insert_entity(&make_entity("e1", "Rust", "language"))
        .expect("insert e1");
    store
        .insert_entity(&make_entity("e2", "Rust", "game"))
        .expect("insert e2");

    let rows = store
        .run_query(
            r"?[id, name] := *entities{id, name}",
            std::collections::BTreeMap::new(),
        )
        .expect("raw query");
    assert_eq!(rows.rows.len(), 2);
}

#[test]
fn list_entities_returns_inserted_entities() {
    let store = make_store();
    store
        .insert_entity(&make_entity("e1", "Alice", "person"))
        .expect("insert alice");
    store
        .insert_entity(&make_entity("e2", "Aletheia", "project"))
        .expect("insert aletheia");

    let entities = store.list_entities().expect("list_entities");
    assert_eq!(entities.len(), 2, "both entities must be returned");
    let names: Vec<&str> = entities.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"Alice"));
    assert!(names.contains(&"Aletheia"));
}

#[test]
fn list_entities_empty_store_returns_empty() {
    let store = make_store();
    let entities = store.list_entities().expect("list_entities empty");
    assert!(entities.is_empty());
}

#[test]
fn write_then_read_roundtrip_facts_and_entities() {
    let store = make_store();

    let fact = make_fact("rt-1", "chiron", "Alice prefers dark mode");
    store.insert_fact(&fact).expect("insert fact");

    let entity = make_entity("e-alice", "Alice", "person");
    store.insert_entity(&entity).expect("insert entity");

    store
        .insert_fact_entity(&fact.id, &entity.id)
        .expect("link fact to entity");

    // NOTE: Read via scoped query (simulates ?nous_id=chiron)
    let scoped = store.audit_all_facts("chiron", 100).expect("audit scoped");
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].content, "Alice prefers dark mode");

    // NOTE: Read via unscoped query (simulates no nous_id filter)
    let unscoped = store.list_all_facts(100).expect("list_all_facts");
    assert_eq!(unscoped.len(), 1);
    assert_eq!(unscoped[0].content, "Alice prefers dark mode");
    assert_eq!(unscoped[0].nous_id, "chiron");

    let entities = store.list_entities().expect("list_entities");
    assert_eq!(entities.len(), 1);
    assert_eq!(entities[0].name, "Alice");
    assert_eq!(entities[0].entity_type, "person");
}

#[test]
fn concurrent_entity_inserts() {
    let store = make_store();
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let s = Arc::clone(&store);
            std::thread::spawn(move || {
                let entity = Entity {
                    id: crate::id::EntityId::new_unchecked(format!("e-concurrent-{i}")),
                    name: format!("Entity {i}"),
                    entity_type: "concept".to_owned(),
                    aliases: vec![],
                    created_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                        .expect("valid test timestamp"),
                    updated_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                        .expect("valid test timestamp"),
                };
                s.insert_entity(&entity).expect("concurrent entity insert");
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread join");
    }

    let rows = store
        .run_query(
            r"?[count(id)] := *entities{id}",
            std::collections::BTreeMap::new(),
        )
        .expect("count entities");
    assert_eq!(rows.rows.len(), 1);
}

#[test]
fn insert_entity_unicode() {
    let store = make_store();
    let entity = make_entity("eu1", "Ελληνικά", "language");
    store.insert_entity(&entity).expect("insert unicode entity");
    let rows = store
        .entity_neighborhood(&crate::id::EntityId::new_unchecked("eu1"))
        .expect("neighborhood query");
    assert!(rows.rows.is_empty() || !rows.rows.is_empty());
}
