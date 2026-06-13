//! Regression tests for fact-entity edge population during extraction (#4675).
//!
//! Normal extraction must link each persisted fact to the subject/object
//! entities it references, so graph-aware recall, scoped dedup, and
//! consolidation operate on real `fact_entities` edges rather than a bag of
//! disconnected rows.
#![cfg_attr(
    feature = "mneme-engine",
    expect(clippy::expect_used, reason = "test assertions")
)]

#[cfg(feature = "mneme-engine")]
use super::super::*;

#[cfg(feature = "mneme-engine")]
fn entity(name: &str, entity_type: &str) -> eidos::bookkeeping::ExtractedEntity {
    eidos::bookkeeping::ExtractedEntity {
        name: name.to_owned(),
        entity_type: entity_type.to_owned(),
        description: String::new(),
    }
}

/// Co-extracted entities, relationships, and a triple must all persist AND the
/// fact must be linked to its subject and object entities.
#[cfg(feature = "mneme-engine")]
#[test]
fn extraction_links_fact_to_subject_and_object_entities() {
    let store = crate::knowledge_store::KnowledgeStore::open_mem()
        .expect("in-memory knowledge store should open successfully");
    let engine = ExtractionEngine::new(ExtractionConfig::default());

    let extraction = Extraction {
        entities: vec![entity("Alice", "person"), entity("Rust", "tool")],
        relationships: vec![ExtractedRelationship {
            source: "Alice".to_owned(),
            relation: "uses".to_owned(),
            target: "Rust".to_owned(),
            confidence: 0.9,
        }],
        facts: vec![ExtractedFact {
            subject: "Alice".to_owned(),
            predicate: "prefers".to_owned(),
            object: "Rust".to_owned(),
            confidence: 0.9,
            fact_type: None,
            is_correction: false,
        }],
    };

    let result = engine
        .persist(&extraction, &store, "session:test", "syn")
        .expect("persist should succeed");

    assert_eq!(result.entities_inserted, 2, "both entities persist");
    assert_eq!(result.relationships_inserted, 1, "relationship persists");
    assert_eq!(result.facts_inserted, 1, "fact persists");
    assert_eq!(
        result.fact_entities_inserted, 2,
        "fact must link to both its subject and object entities"
    );

    let fact_id = crate::id::FactId::new("alice-prefers-0").expect("valid fact id");
    let linked = store
        .list_entities_for_facts(&[fact_id])
        .expect("list linked entities");
    let mut names: Vec<&str> = linked.iter().map(|e| e.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec!["Alice", "Rust"],
        "both referenced entities must be reachable through fact_entities"
    );
}

/// A subject/object that does not resolve to a known entity must not create a
/// dangling edge; only the resolvable side is linked.
#[cfg(feature = "mneme-engine")]
#[test]
fn extraction_skips_unknown_entity_references() {
    let store = crate::knowledge_store::KnowledgeStore::open_mem()
        .expect("in-memory knowledge store should open successfully");
    let engine = ExtractionEngine::new(ExtractionConfig::default());

    // Only the subject entity is extracted; the object ("Postgres") is not.
    let extraction = Extraction {
        entities: vec![entity("Bob", "person")],
        relationships: vec![],
        facts: vec![ExtractedFact {
            subject: "Bob".to_owned(),
            predicate: "deployed".to_owned(),
            object: "Postgres".to_owned(),
            confidence: 0.8,
            fact_type: None,
            is_correction: false,
        }],
    };

    let result = engine
        .persist(&extraction, &store, "session:test", "syn")
        .expect("persist should succeed");

    assert_eq!(
        result.fact_entities_inserted, 1,
        "only the known subject entity is linked; the unknown object is skipped"
    );

    let fact_id = crate::id::FactId::new("bob-deployed-0").expect("valid fact id");
    let linked = store
        .list_entities_for_facts(&[fact_id])
        .expect("list linked entities");
    let names: Vec<&str> = linked.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Bob"],
        "unknown object reference must not create a dangling fact-entity edge"
    );
}
