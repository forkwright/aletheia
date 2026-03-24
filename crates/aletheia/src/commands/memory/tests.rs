use std::sync::Arc;

use aletheia_mneme::id::{EntityId, FactId};
use aletheia_mneme::knowledge::{
    Entity, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
    far_future,
};
use aletheia_mneme::knowledge_store::KnowledgeStore;

fn test_store() -> Arc<KnowledgeStore> {
    KnowledgeStore::open_mem().expect("failed to open in-memory store")
}

fn make_fact(id: &str, nous_id: &str, content: &str) -> Fact {
    let now = jiff::Timestamp::now();
    Fact {
        id: FactId::new(id).expect("valid id"),
        nous_id: nous_id.to_owned(),
        fact_type: "observation".to_owned(),
        content: content.to_owned(),
        temporal: FactTemporal {
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: 0.8,
            tier: EpistemicTier::Inferred,
            source_session_id: None,
            stability_hours: 168.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    }
}

fn make_entity(id: &str, name: &str, entity_type: &str) -> Entity {
    let now = jiff::Timestamp::now();
    Entity {
        id: EntityId::new(id).expect("valid id"),
        name: name.to_owned(),
        entity_type: entity_type.to_owned(),
        aliases: Vec::new(),
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn check_reports_empty_store_as_healthy() {
    let store = test_store();
    let report = super::build_check_report(&store).expect("check should succeed");
    assert_eq!(report.fact_count, 0, "empty store has no facts");
    assert_eq!(report.entity_count, 0, "empty store has no entities");
    assert_eq!(report.orphaned_entity_count, 0, "no orphans in empty store");
    assert_eq!(
        report.dangling_edge_count, 0,
        "no dangling edges in empty store"
    );
}

#[test]
fn check_detects_orphaned_entity() {
    let store = test_store();
    let entity = make_entity("ent-001", "Alice", "person");
    store
        .insert_entity(&entity)
        .expect("insert entity should succeed");

    let report = super::build_check_report(&store).expect("check should succeed");
    assert_eq!(report.entity_count, 1, "one entity inserted");
    assert_eq!(
        report.orphaned_entity_count, 1,
        "entity with no relationships is orphaned"
    );
    assert_eq!(report.orphaned_entity_ids, vec!["ent-001"]);
}

#[test]
fn check_entity_with_relationship_not_orphaned() {
    let store = test_store();
    let e1 = make_entity("ent-a", "Alice", "person");
    let e2 = make_entity("ent-b", "Bob", "person");
    store.insert_entity(&e1).expect("insert e1");
    store.insert_entity(&e2).expect("insert e2");

    let rel = aletheia_mneme::knowledge::Relationship {
        src: EntityId::new("ent-a").expect("valid id"),
        dst: EntityId::new("ent-b").expect("valid id"),
        relation: "knows".to_owned(),
        weight: 1.0,
        created_at: jiff::Timestamp::now(),
    };
    store
        .insert_relationship(&rel)
        .expect("insert relationship");

    let report = super::build_check_report(&store).expect("check should succeed");
    assert_eq!(report.entity_count, 2, "two entities");
    assert_eq!(
        report.orphaned_entity_count, 0,
        "entities with relationships are not orphaned"
    );
}

#[test]
fn dedup_finds_content_hash_duplicates() {
    let store = test_store();
    let f1 = make_fact("fact-001", "nous-1", "Alice likes Rust");
    let f2 = make_fact("fact-002", "nous-1", "Alice likes Rust");
    let f3 = make_fact("fact-003", "nous-1", "Bob prefers Go");
    store.insert_fact(&f1).expect("insert f1");
    store.insert_fact(&f2).expect("insert f2");
    store.insert_fact(&f3).expect("insert f3");

    let dupes =
        super::find_content_hash_duplicates(&store, "nous-1").expect("dedup scan should succeed");
    assert_eq!(dupes.len(), 1, "one set of duplicates");
    assert_eq!(dupes[0].1.len(), 2, "two copies of the duplicate");
}

#[test]
fn dedup_no_false_positives_on_unique_content() {
    let store = test_store();
    let f1 = make_fact("fact-a", "nous-1", "fact one");
    let f2 = make_fact("fact-b", "nous-1", "fact two");
    store.insert_fact(&f1).expect("insert f1");
    store.insert_fact(&f2).expect("insert f2");

    let dupes =
        super::find_content_hash_duplicates(&store, "nous-1").expect("dedup scan should succeed");
    assert!(dupes.is_empty(), "unique facts produce no duplicates");
}

#[test]
fn sample_returns_requested_count() {
    let items: Vec<i32> = (0..100).collect();
    let sampled = super::sample_random(&items, 10);
    assert_eq!(sampled.len(), 10, "sample returns exactly requested count");
}

#[test]
fn sample_clamps_to_available() {
    let items: Vec<i32> = (0..5).collect();
    let sampled = super::sample_random(&items, 20);
    assert_eq!(sampled.len(), 5, "sample clamps to available items");
}

#[test]
fn count_relation_returns_zero_for_empty() {
    let store = test_store();
    let count =
        super::count_relation(&store, "facts").expect("count should succeed on empty store");
    assert_eq!(count, 0, "empty relation has zero rows");
}

#[test]
fn count_relation_after_insert() {
    let store = test_store();
    store
        .insert_fact(&make_fact("f1", "n1", "content"))
        .expect("insert");
    let count = super::count_relation(&store, "facts").expect("count should succeed");
    assert!(count >= 1, "at least one fact after insert");
}
