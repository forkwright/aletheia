use std::path::Path;
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

fn write_fjall_marker(path: &Path) {
    use std::io::Write as _;

    std::fs::create_dir_all(path).expect("create fjall marker dir");
    let mut marker = std::fs::File::create(path.join("version")).expect("create fjall marker");
    marker.write_all(b"3").expect("write fjall version marker");
}

#[test]
fn recovery_store_paths_skips_internal_keyspaces_even_if_polluted() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("data").join("knowledge.fjall");
    let shared = root.join("shared");
    let keyspaces = root.join("keyspaces");
    write_fjall_marker(&shared);
    write_fjall_marker(&keyspaces);

    let oikos = taxis::oikos::Oikos::from_root(tmp.path());
    let stores = super::recovery_store_paths(&oikos).expect("recovery stores");

    assert_eq!(stores, vec![shared]);
}

#[test]
fn recovery_store_paths_maps_legacy_root_to_shared_migration_target() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("data").join("knowledge.fjall");
    write_fjall_marker(&root);
    std::fs::create_dir_all(root.join("keyspaces")).expect("create legacy keyspaces dir");

    let oikos = taxis::oikos::Oikos::from_root(tmp.path());
    let stores = super::recovery_store_paths(&oikos).expect("recovery stores");

    assert_eq!(stores, vec![root.join("shared")]);
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
        project_id: None,
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

fn link_fact_to_entity(
    store: &Arc<mneme::knowledge_store::KnowledgeStore>,
    fact_id: &FactId,
    entity_id: &EntityId,
) {
    use std::collections::BTreeMap;
    let mut params = BTreeMap::new();
    params.insert(
        "fact_id".to_owned(),
        mneme::engine::DataValue::Str(fact_id.as_str().into()),
    );
    params.insert(
        "entity_id".to_owned(),
        mneme::engine::DataValue::Str(entity_id.as_str().into()),
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
}

// --- pattern queries respect nous_id scope (regression: #4268) ---
//
// Seeds two parallel sub-graphs under two distinct nous_ids and asserts that
// `find_*` filters strictly: queries with `Some(nid)` see only that nous's
// facts/entities/relationships; queries with `None` see the global graph.

fn seed_two_nous_subgraphs(store: &Arc<mneme::knowledge_store::KnowledgeStore>) {
    // Entities used by nous-1
    let alice = make_entity("ent-alice", "Alice", "person");
    let bob = make_entity("ent-bob", "Bob", "person");
    // Entities used by nous-2
    let carol = make_entity("ent-carol", "Carol", "person");
    let dan = make_entity("ent-dan", "Dan", "person");
    for e in [&alice, &bob, &carol, &dan] {
        store.insert_entity(e).expect("insert entity");
    }

    // nous-1 facts: each mentions both Alice and Bob → cooccurrence count = 2
    let f1 = make_fact("fact-n1-1", "nous-1", "Alice met Bob");
    let f2 = make_fact("fact-n1-2", "nous-1", "Alice and Bob co-author");
    store.insert_fact(&f1).expect("insert f1");
    store.insert_fact(&f2).expect("insert f2");
    link_fact_to_entity(store, &f1.id, &alice.id);
    link_fact_to_entity(store, &f1.id, &bob.id);
    link_fact_to_entity(store, &f2.id, &alice.id);
    link_fact_to_entity(store, &f2.id, &bob.id);

    // nous-2 facts: each mentions both Carol and Dan → cooccurrence count = 2
    let f3 = make_fact("fact-n2-1", "nous-2", "Carol met Dan");
    let f4 = make_fact("fact-n2-2", "nous-2", "Carol and Dan collaborate");
    store.insert_fact(&f3).expect("insert f3");
    store.insert_fact(&f4).expect("insert f4");
    link_fact_to_entity(store, &f3.id, &carol.id);
    link_fact_to_entity(store, &f3.id, &dan.id);
    link_fact_to_entity(store, &f4.id, &carol.id);
    link_fact_to_entity(store, &f4.id, &dan.id);

    // Relationships: distinct relation names per sub-graph so the chain query
    // produces a unique signature per nous.
    store
        .insert_relationship(&make_relationship("ent-alice", "ent-bob", "knows"))
        .expect("rel knows");
    store
        .insert_relationship(&make_relationship("ent-carol", "ent-dan", "follows"))
        .expect("rel follows");
}

#[test]
fn find_entity_cooccurrence_scoped_to_nous() {
    let store = test_store();
    seed_two_nous_subgraphs(&store);

    let all = super::find_entity_cooccurrence(&store, None, 50).expect("unfiltered query");
    let names: Vec<(String, String)> = all.iter().map(|(a, b, _)| (a.clone(), b.clone())).collect();
    assert!(
        names
            .iter()
            .any(|(a, b)| { (a == "Alice" && b == "Bob") || (a == "Bob" && b == "Alice") }),
        "unfiltered cooccurrence includes Alice/Bob: got {names:?}"
    );
    assert!(
        names
            .iter()
            .any(|(a, b)| { (a == "Carol" && b == "Dan") || (a == "Dan" && b == "Carol") }),
        "unfiltered cooccurrence includes Carol/Dan: got {names:?}"
    );

    let nous1 = super::find_entity_cooccurrence(&store, Some("nous-1"), 50)
        .expect("nous-1 cooccurrence query");
    let n1_names: Vec<(String, String)> = nous1
        .iter()
        .map(|(a, b, _)| (a.clone(), b.clone()))
        .collect();
    assert!(
        n1_names
            .iter()
            .all(|(a, b)| { (a != "Carol" && b != "Carol") && (a != "Dan" && b != "Dan") }),
        "nous-1 cooccurrence must exclude nous-2 entities (Carol/Dan): got {n1_names:?}"
    );
    assert!(
        n1_names
            .iter()
            .any(|(a, b)| { (a == "Alice" && b == "Bob") || (a == "Bob" && b == "Alice") }),
        "nous-1 cooccurrence still includes Alice/Bob: got {n1_names:?}"
    );

    let nous2 = super::find_entity_cooccurrence(&store, Some("nous-2"), 50)
        .expect("nous-2 cooccurrence query");
    let n2_names: Vec<(String, String)> = nous2
        .iter()
        .map(|(a, b, _)| (a.clone(), b.clone()))
        .collect();
    assert!(
        n2_names
            .iter()
            .all(|(a, b)| { (a != "Alice" && b != "Alice") && (a != "Bob" && b != "Bob") }),
        "nous-2 cooccurrence must exclude nous-1 entities (Alice/Bob): got {n2_names:?}"
    );
    assert!(
        n2_names
            .iter()
            .any(|(a, b)| { (a == "Carol" && b == "Dan") || (a == "Dan" && b == "Carol") }),
        "nous-2 cooccurrence still includes Carol/Dan: got {n2_names:?}"
    );

    let unknown =
        super::find_entity_cooccurrence(&store, Some("nous-none"), 50).expect("unknown nous query");
    assert!(
        unknown.is_empty(),
        "unknown nous_id returns empty cooccurrence: got {unknown:?}"
    );
}

#[test]
fn find_relationship_chains_scoped_to_nous() {
    let store = test_store();
    seed_two_nous_subgraphs(&store);

    let all = super::find_relationship_chains(&store, None, 50).expect("unfiltered chains");
    let all_rels: Vec<String> = all.iter().map(|(r, _)| r.clone()).collect();
    assert!(
        all_rels.contains(&"knows".to_owned()),
        "unfiltered has knows"
    );
    assert!(
        all_rels.contains(&"follows".to_owned()),
        "unfiltered has follows"
    );

    let nous1 = super::find_relationship_chains(&store, Some("nous-1"), 50).expect("nous-1 chains");
    let n1_rels: Vec<String> = nous1.iter().map(|(r, _)| r.clone()).collect();
    assert!(
        n1_rels.contains(&"knows".to_owned()),
        "nous-1 has knows: got {n1_rels:?}"
    );
    assert!(
        !n1_rels.contains(&"follows".to_owned()),
        "nous-1 must exclude follows (belongs to nous-2): got {n1_rels:?}"
    );

    let nous2 = super::find_relationship_chains(&store, Some("nous-2"), 50).expect("nous-2 chains");
    let n2_rels: Vec<String> = nous2.iter().map(|(r, _)| r.clone()).collect();
    assert!(
        n2_rels.contains(&"follows".to_owned()),
        "nous-2 has follows: got {n2_rels:?}"
    );
    assert!(
        !n2_rels.contains(&"knows".to_owned()),
        "nous-2 must exclude knows (belongs to nous-1): got {n2_rels:?}"
    );
}

#[test]
fn find_hub_entities_scoped_to_nous() {
    let store = test_store();
    seed_two_nous_subgraphs(&store);

    let all = super::find_hub_entities(&store, None, 50).expect("unfiltered hubs");
    let all_names: Vec<String> = all.iter().map(|(n, _)| n.clone()).collect();
    for name in ["Alice", "Bob", "Carol", "Dan"] {
        assert!(
            all_names.contains(&name.to_owned()),
            "unfiltered hubs include {name}: got {all_names:?}"
        );
    }

    let nous1 = super::find_hub_entities(&store, Some("nous-1"), 50).expect("nous-1 hubs");
    let n1_names: Vec<String> = nous1.iter().map(|(n, _)| n.clone()).collect();
    assert!(
        n1_names.contains(&"Alice".to_owned()),
        "nous-1 hubs include Alice: got {n1_names:?}"
    );
    assert!(
        n1_names.contains(&"Bob".to_owned()),
        "nous-1 hubs include Bob: got {n1_names:?}"
    );
    assert!(
        !n1_names.contains(&"Carol".to_owned()),
        "nous-1 hubs exclude Carol (belongs to nous-2): got {n1_names:?}"
    );
    assert!(
        !n1_names.contains(&"Dan".to_owned()),
        "nous-1 hubs exclude Dan (belongs to nous-2): got {n1_names:?}"
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
