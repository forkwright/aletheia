//! Acceptance tests for extraction: parsing and persistence.
//! Acceptance criteria tests for extraction pipeline.
#![expect(clippy::expect_used, reason = "test assertions")]
use super::super::utils;
use super::super::*;

// --- Acceptance criteria tests (prompt 99) ---

#[test]
fn parse_empty_extraction() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let json = r#"{"entities": [], "relationships": [], "facts": []}"#;
    let extraction = engine
        .parse_response(json)
        .expect("empty arrays JSON should parse to empty extraction");
    assert!(extraction.entities.is_empty(), "entities should be empty");
    assert!(
        extraction.relationships.is_empty(),
        "relationships should be empty"
    );
    assert!(extraction.facts.is_empty(), "facts should be empty");
}

#[test]
fn parse_missing_fields_errors() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    // Missing required fields: serde_json requires all fields on Extraction.
    // "facts" key with only "content" is wrong shape (ExtractedFact needs subject/predicate/object).
    let json = r#"{"facts": [{"content": "test"}]}"#;
    let result = engine.parse_response(json);
    assert!(result.is_err(), "missing required fields should error");
}

#[test]
fn parse_missing_entities_and_relationships_errors() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    // Extraction requires all three fields: entities, relationships, facts.
    let json = r#"{"facts": []}"#;
    let result = engine.parse_response(json);
    assert!(
        result.is_err(),
        "missing entities/relationships fields should error"
    );
}

#[test]
fn parse_confidence_preserves_out_of_range() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    // NOTE: The parser does NOT clamp confidence values: it stores them as-is.
    // This is a documentation test: values outside [0,1] parse without error
    // but are semantically invalid. Validation should happen at the persist layer.
    let json = r#"{
        "entities": [],
        "relationships": [
            {"source": "Alice", "relation": "knows", "target": "Bob", "confidence": 1.5}
        ],
        "facts": [
            {"subject": "Alice", "predicate": "uses", "object": "Rust", "confidence": -0.3}
        ]
    }"#;
    let extraction = engine
        .parse_response(json)
        .expect("out-of-range confidence values should parse without error");
    assert!(
        (extraction.relationships[0].confidence - 1.5).abs() < f64::EPSILON,
        "confidence > 1.0 is not clamped at parse time"
    );
    assert!(
        (extraction.facts[0].confidence - (-0.3)).abs() < f64::EPSILON,
        "confidence < 0.0 is not clamped at parse time"
    );
}

#[test]
fn parse_handles_all_entity_types() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let json = r#"{
        "entities": [
            {"name": "Alice", "entity_type": "person", "description": "Engineer"},
            {"name": "Aletheia", "entity_type": "project", "description": "AI memory"},
            {"name": "Memory", "entity_type": "concept", "description": "Cognitive function"},
            {"name": "Rust", "entity_type": "tool", "description": "Language"},
            {"name": "Athens", "entity_type": "location", "description": "City"},
            {"name": "Acme", "entity_type": "unknown_type", "description": "Unrecognized type passes through"}
        ],
        "relationships": [],
        "facts": []
    }"#;
    let extraction = engine
        .parse_response(json)
        .expect("all entity types including unknown should parse");
    assert_eq!(
        extraction.entities.len(),
        6,
        "should parse all 6 entities including unknown type"
    );
    // entity_type is a free-form string: no validation at parse time
    assert_eq!(
        extraction.entities[5].entity_type, "unknown_type",
        "unknown entity type should pass through unchanged"
    );
}

#[test]
fn parse_handles_multiple_facts() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let json = r#"{
        "entities": [],
        "relationships": [],
        "facts": [
            {"subject": "Alice", "predicate": "uses", "object": "Rust", "confidence": 0.9},
            {"subject": "Alice", "predicate": "works on", "object": "Aletheia", "confidence": 0.95},
            {"subject": "Aletheia", "predicate": "stores", "object": "knowledge", "confidence": 0.8},
            {"subject": "Rust", "predicate": "is", "object": "a programming language", "confidence": 1.0},
            {"subject": "Alice", "predicate": "lives in", "object": "Athens", "confidence": 0.7}
        ]
    }"#;
    let extraction = engine
        .parse_response(json)
        .expect("multiple facts should parse successfully");
    assert_eq!(extraction.facts.len(), 5, "should parse all 5 facts");
}

#[test]
fn parse_does_not_deduplicate_entities() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    // NOTE: The parser does NOT deduplicate entities: it returns them as-is.
    // Deduplication is the responsibility of the persist layer.
    let json = r#"{
        "entities": [
            {"name": "Alice", "entity_type": "person", "description": "Engineer"},
            {"name": "Alice", "entity_type": "person", "description": "Developer"}
        ],
        "relationships": [],
        "facts": []
    }"#;
    let extraction = engine
        .parse_response(json)
        .expect("duplicate entities in JSON should parse without deduplication");
    assert_eq!(
        extraction.entities.len(),
        2,
        "parser returns duplicates — dedup is a persist-layer concern"
    );
}

#[test]
fn strip_code_fences_with_leading_whitespace() {
    let input = "  \n```json\n{\"a\":1}\n```\n  ";
    assert_eq!(
        utils::strip_code_fences(input),
        r#"{"a":1}"#,
        "leading whitespace before code fence should be ignored"
    );
}

#[test]
fn strip_code_fences_no_closing_fence() {
    // LLM sometimes forgets the closing fence
    let input = "```json\n{\"a\":1}";
    let result = utils::strip_code_fences(input);
    // Should still produce parseable JSON (strips prefix, returns rest)
    assert!(
        result.contains(r#"{"a":1}"#),
        "JSON body should be recoverable even without closing fence"
    );
}

#[test]
fn build_prompt_respects_max_entities() {
    let config = ExtractionConfig {
        max_entities: 5,
        max_relationships: 8,
        ..ExtractionConfig::default()
    };
    let engine = ExtractionEngine::new(config);
    let messages = vec![ConversationMessage {
        role: "user".to_owned(),
        content: "Alice works on Aletheia using Rust.".to_owned(),
    }];
    let prompt = engine.build_prompt(&messages);
    assert!(
        prompt.system.contains("5 entities"),
        "prompt should reference configured max_entities"
    );
    assert!(
        prompt.system.contains("8 relationships"),
        "prompt should reference configured max_relationships"
    );
}

#[test]
fn build_prompt_concatenates_messages_in_order() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let messages = vec![
        ConversationMessage {
            role: "user".to_owned(),
            content: "first message".to_owned(),
        },
        ConversationMessage {
            role: "assistant".to_owned(),
            content: "second message".to_owned(),
        },
        ConversationMessage {
            role: "user".to_owned(),
            content: "third message".to_owned(),
        },
    ];

    let prompt = engine.build_prompt(&messages);
    let first_pos = prompt
        .user_message
        .find("first message")
        .expect("first message should appear in user_message");
    let second_pos = prompt
        .user_message
        .find("second message")
        .expect("second message should appear in user_message");
    let third_pos = prompt
        .user_message
        .find("third message")
        .expect("third message should appear in user_message");
    assert!(
        first_pos < second_pos,
        "first message should appear before second in prompt"
    );
    assert!(
        second_pos < third_pos,
        "second message should appear before third in prompt"
    );
}

#[tokio::test]
async fn extract_provider_error_propagates() {
    struct FailingProvider;
    impl ExtractionProvider for FailingProvider {
        fn complete<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<String, ExtractionError>> + Send + 'a>,
        > {
            Box::pin(async {
                LlmCallSnafu {
                    message: "rate limited",
                }
                .fail()
            })
        }
    }

    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let messages = vec![ConversationMessage {
        role: "user".to_owned(),
        content: "Alice works on Aletheia, an AI memory system built in Rust for agent cognition."
            .to_owned(),
    }];

    let result = engine.extract(&messages, &FailingProvider).await;
    assert!(result.is_err(), "failing provider should return an error");
    assert!(
        matches!(
            result.expect_err("failing provider should return LlmCall error"),
            ExtractionError::LlmCall { .. }
        ),
        "error should be LlmCall variant"
    );
}

#[cfg(feature = "mneme-engine")]
#[test]
fn persist_round_trip() {
    let store = crate::knowledge_store::KnowledgeStore::open_mem()
        .expect("in-memory knowledge store should open successfully");
    let engine = ExtractionEngine::new(ExtractionConfig::default());

    let extraction = Extraction {
        entities: vec![
            ExtractedEntity {
                name: "Dr. Chen".to_owned(),
                entity_type: "person".to_owned(),
                description: "Developer of Aletheia".to_owned(),
            },
            ExtractedEntity {
                name: "Aletheia".to_owned(),
                entity_type: "project".to_owned(),
                description: "AI memory system".to_owned(),
            },
        ],
        relationships: vec![ExtractedRelationship {
            source: "Dr. Chen".to_owned(),
            relation: "works on".to_owned(),
            target: "Aletheia".to_owned(),
            confidence: 0.95,
        }],
        facts: vec![ExtractedFact {
            subject: "Aletheia".to_owned(),
            predicate: "uses".to_owned(),
            object: "CozoDB for knowledge storage".to_owned(),
            confidence: 0.9,
            is_correction: false,
            fact_type: None,
        }],
    };

    let result = engine
        .persist(&extraction, &store, "session:test:main:2026-03-02", "syn")
        .expect("persist should succeed with valid entities, relationships, and facts");
    assert_eq!(result.entities_inserted, 2, "should insert 2 entities");
    assert_eq!(
        result.relationships_inserted, 1,
        "should insert 1 relationship"
    );
    assert_eq!(
        result.relationships_skipped, 0,
        "no relationships should be skipped"
    );
    assert_eq!(result.facts_inserted, 1, "should insert 1 fact");

    // Verify entities are queryable via entity_neighborhood.
    let neighborhood = store
        .entity_neighborhood(&crate::id::EntityId::new_unchecked("dr-chen"))
        .expect("entity neighborhood query for dr-chen should succeed");
    assert!(
        !neighborhood.rows.is_empty(),
        "dr-chen entity should be reachable in the graph"
    );

    // query_facts filters: valid_from <= now AND valid_to > now
    // Use a future time that's after valid_from but before valid_to.
    // far_future() is 9999-01-01T00:00:00Z, so query before that.
    let facts = store
        .query_facts("syn", "2099-01-01T00:00:00Z", 100)
        .expect("query_facts should return results for syn nous up to year 2099");
    assert!(
        facts.iter().any(|f| f.content.contains("CozoDB")),
        "persisted fact should be retrievable"
    );
}

#[cfg(feature = "mneme-engine")]
#[test]
fn persist_skips_relates_to() {
    let store = crate::knowledge_store::KnowledgeStore::open_mem()
        .expect("in-memory knowledge store should open successfully");
    let engine = ExtractionEngine::new(ExtractionConfig::default());

    let extraction = Extraction {
        entities: vec![
            ExtractedEntity {
                name: "Nyx".to_owned(),
                entity_type: "person".to_owned(),
                description: String::new(),
            },
            ExtractedEntity {
                name: "Sol".to_owned(),
                entity_type: "person".to_owned(),
                description: String::new(),
            },
        ],
        relationships: vec![ExtractedRelationship {
            source: "Nyx".to_owned(),
            relation: "RELATES_TO".to_owned(),
            target: "Sol".to_owned(),
            confidence: 0.8,
        }],
        facts: vec![],
    };

    let result = engine
        .persist(&extraction, &store, "session:test", "syn")
        .expect("persist should succeed even when all relationships are skipped");
    assert_eq!(
        result.relationships_inserted, 0,
        "RELATES_TO relationship should not be inserted"
    );
    assert_eq!(
        result.relationships_skipped, 1,
        "RELATES_TO relationship should be counted as skipped"
    );
}

#[cfg(feature = "mneme-engine")]
#[test]
fn persist_normalizes_relation_type() {
    let store = crate::knowledge_store::KnowledgeStore::open_mem()
        .expect("in-memory knowledge store should open successfully");
    let engine = ExtractionEngine::new(ExtractionConfig::default());

    let extraction = Extraction {
        entities: vec![
            ExtractedEntity {
                name: "Nyx".to_owned(),
                entity_type: "person".to_owned(),
                description: String::new(),
            },
            ExtractedEntity {
                name: "Helios".to_owned(),
                entity_type: "project".to_owned(),
                description: String::new(),
            },
        ],
        relationships: vec![ExtractedRelationship {
            source: "Nyx".to_owned(),
            relation: "works on".to_owned(),
            target: "Helios".to_owned(),
            confidence: 0.9,
        }],
        facts: vec![],
    };

    let result = engine
        .persist(&extraction, &store, "session:test", "syn")
        .expect("persist should succeed with normalized relation type");
    assert_eq!(
        result.relationships_inserted, 1,
        "normalized relationship should be inserted"
    );
    assert_eq!(
        result.relationships_skipped, 0,
        "no relationships should be skipped after normalization"
    );

    let neighborhood = store
        .entity_neighborhood(&crate::id::EntityId::new_unchecked("nyx"))
        .expect("entity neighborhood query for nyx should succeed");
    assert!(
        neighborhood
            .rows
            .iter()
            .any(|row| row.iter().any(|v| v.get_str() == Some("WORKS_AT"))),
        "relationship should be stored as normalized WORKS_AT"
    );
}

#[cfg(feature = "mneme-engine")]
#[test]
fn persist_accepts_novel_type() {
    let store = crate::knowledge_store::KnowledgeStore::open_mem()
        .expect("in-memory knowledge store should open successfully");
    let engine = ExtractionEngine::new(ExtractionConfig::default());

    let extraction = Extraction {
        entities: vec![
            ExtractedEntity {
                name: "Nyx".to_owned(),
                entity_type: "person".to_owned(),
                description: String::new(),
            },
            ExtractedEntity {
                name: "Sol".to_owned(),
                entity_type: "person".to_owned(),
                description: String::new(),
            },
        ],
        relationships: vec![ExtractedRelationship {
            source: "Nyx".to_owned(),
            relation: "MENTORS".to_owned(),
            target: "Sol".to_owned(),
            confidence: 0.7,
        }],
        facts: vec![],
    };

    let result = engine
        .persist(&extraction, &store, "session:test", "syn")
        .expect("persist should succeed with novel relationship type");
    assert_eq!(
        result.relationships_inserted, 1,
        "novel LLM-generated types should be persisted"
    );
    assert_eq!(
        result.relationships_skipped, 0,
        "no relationships should be skipped for novel types"
    );

    let neighborhood = store
        .entity_neighborhood(&crate::id::EntityId::new_unchecked("nyx"))
        .expect("entity neighborhood query for nyx should succeed");
    assert!(
        neighborhood
            .rows
            .iter()
            .any(|row| row.iter().any(|v| v.get_str() == Some("MENTORS"))),
        "novel relationship MENTORS should be stored as-is"
    );
}
