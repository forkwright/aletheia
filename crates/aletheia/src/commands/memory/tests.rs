use std::sync::Arc;

use mneme::id::{EntityId, FactId};
use mneme::knowledge::{
    Entity, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
    Visibility, far_future,
};
use mneme::knowledge_store::KnowledgeStore;

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
        sensitivity: mneme::knowledge::FactSensitivity::Public,
        visibility: Visibility::Private,
        scope: None,
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

    let rel = mneme::knowledge::Relationship {
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

// --- export-graph tests ---

fn make_relationship(src: &str, dst: &str, relation: &str) -> mneme::knowledge::Relationship {
    mneme::knowledge::Relationship {
        src: EntityId::new(src).expect("valid id"),
        dst: EntityId::new(dst).expect("valid id"),
        relation: relation.to_owned(),
        weight: 1.0,
        created_at: jiff::Timestamp::now(),
    }
}

#[test]
fn export_dot_empty_store() {
    let mut buf: Vec<u8> = Vec::new();
    let entities: Vec<mneme::knowledge::Entity> = Vec::new();
    let relationships: Vec<mneme::knowledge::Relationship> = Vec::new();
    let sensitivities = std::collections::HashMap::new();
    super::export_dot(&mut buf, &entities, &relationships, &sensitivities)
        .expect("dot export should succeed");
    let output = String::from_utf8(buf).expect("valid utf-8");
    assert!(output.contains("digraph G"), "dot has digraph header");
    assert!(!output.contains("->"), "empty store has no edges");
}

#[test]
fn export_dot_includes_nodes_and_edges() {
    let mut buf: Vec<u8> = Vec::new();
    let entities = vec![
        make_entity("ent-a", "Alice", "person"),
        make_entity("ent-b", "Bob", "person"),
    ];
    let relationships = vec![make_relationship("ent-a", "ent-b", "knows")];
    let mut sensitivities = std::collections::HashMap::new();
    sensitivities.insert(
        "ent-a".to_owned(),
        mneme::knowledge::FactSensitivity::Public,
    );
    super::export_dot(&mut buf, &entities, &relationships, &sensitivities)
        .expect("dot export should succeed");
    let output = String::from_utf8(buf).expect("valid utf-8");
    assert!(output.contains("\"ent-a\""), "dot contains alice node");
    assert!(output.contains("\"ent-b\""), "dot contains bob node");
    assert!(
        output.contains("\"ent-a\" -> \"ent-b\" [label=\"knows\"]"),
        "dot contains edge"
    );
    assert!(
        output.contains("fillcolor=\"#90EE90\""),
        "public sensitivity is green"
    );
}

#[test]
fn export_dot_colors_by_sensitivity() {
    let mut buf: Vec<u8> = Vec::new();
    let entities = vec![
        make_entity("ent-p", "PublicEnt", "concept"),
        make_entity("ent-i", "InternalEnt", "concept"),
        make_entity("ent-c", "ConfidentialEnt", "concept"),
    ];
    let relationships: Vec<mneme::knowledge::Relationship> = Vec::new();
    let mut sensitivities = std::collections::HashMap::new();
    sensitivities.insert(
        "ent-p".to_owned(),
        mneme::knowledge::FactSensitivity::Public,
    );
    sensitivities.insert(
        "ent-i".to_owned(),
        mneme::knowledge::FactSensitivity::Internal,
    );
    sensitivities.insert(
        "ent-c".to_owned(),
        mneme::knowledge::FactSensitivity::Confidential,
    );
    super::export_dot(&mut buf, &entities, &relationships, &sensitivities)
        .expect("dot export should succeed");
    let output = String::from_utf8(buf).expect("valid utf-8");
    assert!(output.contains("fillcolor=\"#90EE90\""), "public is green");
    assert!(output.contains("fillcolor=\"#FFD700\""), "internal is gold");
    assert!(
        output.contains("fillcolor=\"#FF6B6B\""),
        "confidential is red"
    );
}

#[test]
fn export_json_empty_store() {
    let mut buf: Vec<u8> = Vec::new();
    let entities: Vec<mneme::knowledge::Entity> = Vec::new();
    let relationships: Vec<mneme::knowledge::Relationship> = Vec::new();
    super::export_json(&mut buf, &entities, &relationships).expect("json export should succeed");
    let output = String::from_utf8(buf).expect("valid utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&output).expect("valid json");
    assert_eq!(
        parsed["entities"].as_array().map_or(0, Vec::len),
        0,
        "empty store has no entities"
    );
    assert_eq!(
        parsed["relationships"].as_array().map_or(0, Vec::len),
        0,
        "empty store has no relationships"
    );
}

#[test]
fn export_json_roundtrips_entities_and_relationships() {
    let mut buf: Vec<u8> = Vec::new();
    let entities = vec![
        make_entity("ent-a", "Alice", "person"),
        make_entity("ent-b", "Bob", "person"),
    ];
    let relationships = vec![make_relationship("ent-a", "ent-b", "knows")];
    super::export_json(&mut buf, &entities, &relationships).expect("json export should succeed");
    let output = String::from_utf8(buf).expect("valid utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&output).expect("valid json");
    assert_eq!(
        parsed["entities"].as_array().map_or(0, Vec::len),
        2,
        "two entities"
    );
    assert_eq!(
        parsed["relationships"].as_array().map_or(0, Vec::len),
        1,
        "one relationship"
    );
    assert_eq!(
        parsed["relationships"][0]["src"].as_str(),
        Some("ent-a"),
        "correct src"
    );
    assert_eq!(
        parsed["relationships"][0]["dst"].as_str(),
        Some("ent-b"),
        "correct dst"
    );
}

#[test]
fn export_graphml_empty_store() {
    let mut buf: Vec<u8> = Vec::new();
    let entities: Vec<mneme::knowledge::Entity> = Vec::new();
    let relationships: Vec<mneme::knowledge::Relationship> = Vec::new();
    super::export_graphml(&mut buf, &entities, &relationships)
        .expect("graphml export should succeed");
    let output = String::from_utf8(buf).expect("valid utf-8");
    assert!(
        output.contains("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
        "has xml declaration"
    );
    assert!(output.contains("<graphml xmlns="), "has graphml root");
    assert!(output.contains("</graphml>"), "has closing graphml tag");
}

#[test]
fn export_graphml_includes_nodes_and_edges() {
    let mut buf: Vec<u8> = Vec::new();
    let entities = vec![
        make_entity("ent-a", "Alice", "person"),
        make_entity("ent-b", "Bob", "person"),
    ];
    let relationships = vec![make_relationship("ent-a", "ent-b", "knows")];
    super::export_graphml(&mut buf, &entities, &relationships)
        .expect("graphml export should succeed");
    let output = String::from_utf8(buf).expect("valid utf-8");
    assert!(output.contains(r#"<node id="ent-a">"#), "has alice node");
    assert!(output.contains(r#"<node id="ent-b">"#), "has bob node");
    assert!(
        output.contains(r#"<edge source="ent-a" target="ent-b">"#),
        "has edge"
    );
    assert!(
        output.contains("<data key=\"d2\">knows</data>"),
        "edge has relation data"
    );
}

#[test]
fn load_filtered_facts_respects_nous_filter() {
    use std::collections::BTreeMap;

    let store = test_store();
    let e1 = make_entity("ent-a", "Alice", "person");
    let e2 = make_entity("ent-b", "Bob", "person");
    store.insert_entity(&e1).expect("insert e1");
    store.insert_entity(&e2).expect("insert e2");

    let f1 = make_fact("fact-1", "nous-1", "Alice likes Rust");
    store.insert_fact(&f1).expect("insert f1");

    // Link fact to entity e1 via Datalog :put
    let mut params = BTreeMap::new();
    params.insert(
        "fact_id".to_owned(),
        mneme::engine::DataValue::Str(f1.id.as_str().into()),
    );
    params.insert(
        "entity_id".to_owned(),
        mneme::engine::DataValue::Str(e1.id.as_str().into()),
    );
    params.insert(
        "created_at".to_owned(),
        mneme::engine::DataValue::Str(
            mneme::knowledge::format_timestamp(&jiff::Timestamp::now()).into(),
        ),
    );
    store
        .run_mut_query(
            r"?[fact_id, entity_id, created_at] <- [[$fact_id, $entity_id, $created_at]]
              :put fact_entities {fact_id, entity_id => created_at}",
            params,
        )
        .expect("link fact to entity");

    let (visible, _sensitivities) = super::load_filtered_facts(&store, Some("nous-1"), None, true)
        .expect("load filtered facts should succeed");
    assert!(
        visible.contains("ent-a"),
        "entity linked to nous-1 fact is visible"
    );
    assert!(
        !visible.contains("ent-b"),
        "entity not linked to nous-1 fact is hidden"
    );
}

#[test]
fn sensitivity_dot_color_maps_correctly() {
    assert_eq!(
        super::sensitivity_dot_color(mneme::knowledge::FactSensitivity::Public),
        "#90EE90",
        "public is green"
    );
    assert_eq!(
        super::sensitivity_dot_color(mneme::knowledge::FactSensitivity::Internal),
        "#FFD700",
        "internal is gold"
    );
    assert_eq!(
        super::sensitivity_dot_color(mneme::knowledge::FactSensitivity::Confidential),
        "#FF6B6B",
        "confidential is red"
    );
}
