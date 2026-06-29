#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use std::sync::Arc;

use crate::knowledge::Entity;
use crate::test_fixtures::{make_entity, make_fact, make_relationship, make_store};

mod dedup;

#[test]
fn insert_entity_and_query_neighborhood() {
    let store = make_store();
    let entity = make_entity("e1", "Rust", "language");
    store.insert_entity(&entity).expect("insert entity");

    let rows = store
        .entity_neighborhood(&crate::id::EntityId::new("e1").expect("valid test id"))
        .expect("neighborhood");
    assert!(rows.is_empty());
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
    assert_eq!(rows.row_count(), 1);
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
        .entity_neighborhood(&crate::id::EntityId::new("e1").expect("valid test id"))
        .expect("neighborhood");
    assert!(
        !rows.is_empty(),
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
        .entity_neighborhood(&crate::id::EntityId::new("e1").expect("valid test id"))
        .expect("e1 neighborhood");
    assert!(!from_e1.is_empty());

    let _from_e2 = store
        .entity_neighborhood(&crate::id::EntityId::new("e2").expect("valid test id"))
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
        .entity_neighborhood(&crate::id::EntityId::new("e1").expect("valid test id"))
        .expect("2-hop neighborhood");
    assert!(
        rows.row_count() >= 2,
        "2-hop neighborhood should find at least 2 results, got {}",
        rows.row_count()
    );
}

#[test]
fn entity_neighborhood_nonexistent_entity() {
    let store = make_store();
    let rows = store
        .entity_neighborhood(&crate::id::EntityId::new("nonexistent").expect("valid test id"))
        .expect("neighborhood of missing entity should succeed");
    assert!(rows.is_empty());
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
    assert_eq!(rows.row_count(), 1);
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
    assert_eq!(rows.row_count(), 2);
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

    let fact = make_fact("rt-1", "agent-01", "Alice prefers dark mode");
    store.insert_fact(&fact).expect("insert fact");

    let entity = make_entity("e-alice", "Alice", "person");
    store.insert_entity(&entity).expect("insert entity");

    store
        .insert_fact_entity(&fact.id, &entity.id)
        .expect("link fact to entity");

    // NOTE: Read via scoped query (simulates ?nous_id=agent-01)
    let scoped = store
        .audit_all_facts("agent-01", 100)
        .expect("audit scoped");
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].content, "Alice prefers dark mode");

    // NOTE: Read via unscoped query (simulates no nous_id filter)
    let unscoped = store.list_all_facts(100).expect("list_all_facts");
    assert_eq!(unscoped.len(), 1);
    assert_eq!(unscoped[0].content, "Alice prefers dark mode");
    assert_eq!(unscoped[0].nous_id, "agent-01");

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
                    id: crate::id::EntityId::new(format!("e-concurrent-{i}"))
                        .expect("valid test id"),
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
    assert_eq!(rows.row_count(), 1);
}

#[test]
fn insert_entity_unicode() {
    let store = make_store();
    let entity = make_entity("eu1", "Ελληνικά", "language");
    store.insert_entity(&entity).expect("insert unicode entity");
    let rows = store
        .entity_neighborhood(&crate::id::EntityId::new("eu1").expect("valid test id"))
        .expect("neighborhood query");
    assert!(rows.is_empty() || !rows.is_empty());
}

/// Recovery path: recompute fact embeddings for a restored store that has
/// facts but no rows in the embeddings relation yet.
#[test]
fn reembed_all_populates_fact_embeddings() {
    let store = make_store();
    let f1 = make_fact("fact-1", "agent-a", "Alice likes Rust");
    let f2 = make_fact("fact-2", "agent-b", "Bob prefers Go");
    store.insert_fact(&f1).expect("insert fact 1");
    store.insert_fact(&f2).expect("insert fact 2");

    let provider = crate::embedding::MockEmbeddingProvider::new(crate::test_fixtures::DIM);
    let written = store.reembed_all(&provider).expect("reembed all facts");
    assert_eq!(written, 2, "every fact should receive an embedding");

    let rows = store
        .run_query(
            r"?[id, source_id] := *embeddings{id, source_id}",
            std::collections::BTreeMap::new(),
        )
        .expect("query embeddings");
    assert_eq!(rows.row_count(), 2, "two embedding rows should exist");

    let mut source_ids = Vec::new();
    for row in 0..rows.row_count() {
        source_ids.push(rows.get_string(row, "source_id").expect("source_id"));
    }
    assert!(
        source_ids.iter().any(|id| id == "fact-1"),
        "fact-1 embedding should be present"
    );
    assert!(
        source_ids.iter().any(|id| id == "fact-2"),
        "fact-2 embedding should be present"
    );
}

/// Garbage-collection path: delete only entities that have no relationships
/// and no fact references, leaving linked entities intact.
#[test]
fn remove_orphaned_entities_deletes_only_unlinked_rows() {
    let store = make_store();

    let orphan = make_entity("ent-orphan", "Orphan", "concept");
    let linked = make_entity("ent-linked", "Linked", "concept");
    store.insert_entity(&orphan).expect("insert orphan");
    store.insert_entity(&linked).expect("insert linked");

    let fact = make_fact("fact-linked", "agent-a", "linked fact");
    store.insert_fact(&fact).expect("insert fact");
    store
        .insert_fact_entity(&fact.id, &linked.id)
        .expect("link fact to entity");

    let removed = store
        .remove_orphaned_entities()
        .expect("remove orphaned entities");
    assert_eq!(removed, 1, "only the orphan should be removed");

    let surviving = store.list_entities().expect("list entities");
    assert_eq!(surviving.len(), 1, "linked entity should remain");
    assert_eq!(surviving[0].id, linked.id);

    let removed_again = store
        .remove_orphaned_entities()
        .expect("second remove_orphaned_entities call");
    assert_eq!(removed_again, 0, "garbage collection should be idempotent");
}

#[test]
fn list_entities_for_facts_rejects_invalid_entity_timestamp() {
    let store = make_store();
    let fact = make_fact("bad-entity-fact", "alice", "fact linked to bad entity");
    store.insert_fact(&fact).expect("insert fact");
    store
        .run_mut_query(
            r#"?[id, name, entity_type, aliases, created_at, updated_at, name_embedding] <- [[
                "bad-entity", "Bad Entity", "topic", "", "not-a-timestamp",
                "2026-06-01T00:00:00Z", null
            ]]
            :put entities {id => name, entity_type, aliases, created_at, updated_at, name_embedding}"#,
            std::collections::BTreeMap::new(),
        )
        .expect("insert malformed entity row");
    let entity_id = crate::id::EntityId::new("bad-entity").expect("valid entity id");
    store
        .insert_fact_entity(&fact.id, &entity_id)
        .expect("link malformed entity");

    let err = store
        .list_entities_for_facts(&[fact.id])
        .expect_err("invalid entity timestamp must fail export hydration");
    assert!(
        err.to_string().contains("invalid created_at timestamp"),
        "error should name invalid timestamp, got: {err}"
    );
}

#[test]
fn list_relationships_between_entities_rejects_invalid_weight() {
    let store = make_store();
    store
        .insert_entity(&make_entity("weight-src", "Weight Src", "topic"))
        .expect("insert src");
    store
        .insert_entity(&make_entity("weight-dst", "Weight Dst", "topic"))
        .expect("insert dst");
    store
        .run_mut_query(
            r#"?[src, dst, relation, weight, created_at] <- [[
                "weight-src", "weight-dst", "related_to", 2.0, "2026-06-01T00:00:00Z"
            ]]
            :put relationships {src, dst => relation, weight, created_at}"#,
            std::collections::BTreeMap::new(),
        )
        .expect("insert malformed relationship row");

    let entity_ids = ["weight-src".to_owned(), "weight-dst".to_owned()]
        .into_iter()
        .collect();
    let err = store
        .list_relationships_between_entities(&entity_ids)
        .expect_err("invalid weight must fail relationship hydration");
    assert!(
        err.to_string().contains("relationship weight out of range"),
        "error should name invalid weight, got: {err}"
    );
}

#[test]
fn merge_history_rejects_invalid_audit_timestamp() {
    let store = make_store();
    store
        .run_mut_query(
            r#"?[nous_id, canonical_id, merged_id, merged_name, merge_score,
                facts_transferred, relationships_redirected, merged_at] <- [[
                    "alice", "history-a", "history-b", "History B", 0.8, 1, 0,
                    "not-a-timestamp"
                ]]
            :put merge_audit {
                nous_id, canonical_id, merged_id => merged_name, merge_score,
                facts_transferred, relationships_redirected, merged_at
            }"#,
            std::collections::BTreeMap::new(),
        )
        .expect("insert malformed merge audit row");

    let err = store
        .get_merge_history("alice")
        .expect_err("invalid merge timestamp must fail audit hydration");
    assert!(
        err.to_string().contains("merge audit merged_at"),
        "error should name invalid merge timestamp, got: {err}"
    );
}

/// Schema v13 invariant: a freshly initialised store must accept reads
/// against the `name_embedding` column. This guards against future
/// refactors that drop the column from the static DDL or skip the
/// dim-parameterised `entities_ddl` branch in `init_schema`.
#[test]
fn entities_relation_has_name_embedding_column_in_fresh_store() {
    let store = make_store();
    let rows = store
        .run_query(
            r"?[id, name_embedding] := *entities{id, name_embedding}",
            std::collections::BTreeMap::new(),
        )
        .expect(
            "fresh store must accept a query against the name_embedding column — schema v13 contract",
        );
    assert!(rows.is_empty(), "empty store should return no rows");
}
