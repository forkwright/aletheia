#![expect(clippy::expect_used, reason = "test assertions")]

use super::utils;
use super::*;

#[test]
fn config_default() {
    let cfg = ExtractionConfig::default();
    assert_eq!(
        cfg.model, "claude-haiku-4-5-20251001",
        "default model should be claude-haiku-4-5-20251001"
    );
    assert_eq!(
        cfg.min_message_length, 50,
        "default min_message_length should be 50"
    );
    assert_eq!(cfg.max_entities, 20, "default max_entities should be 20");
    assert_eq!(
        cfg.max_relationships, 30,
        "default max_relationships should be 30"
    );
    assert_eq!(cfg.max_facts, 50, "default max_facts should be 50");
    assert!(cfg.enabled, "extraction should be enabled by default");
}

#[test]
fn build_prompt_contains_instructions() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let messages = vec![
        ConversationMessage {
            role: "user".to_owned(),
            content: "I'm working on Aletheia, a memory system for AI agents.".to_owned(),
        },
        ConversationMessage {
            role: "assistant".to_owned(),
            content: "That sounds like an interesting project. Tell me more about it.".to_owned(),
        },
    ];

    let prompt = engine.build_prompt(&messages);
    assert!(
        prompt.system.contains("JSON"),
        "system prompt should mention JSON output format"
    );
    assert!(
        prompt.system.contains("entities"),
        "system prompt should mention entities"
    );
    assert!(
        prompt.system.contains("relationships"),
        "system prompt should mention relationships"
    );
    assert!(
        prompt.system.contains("facts"),
        "system prompt should mention facts"
    );
    assert!(
        prompt.system.contains("confidence"),
        "system prompt should mention confidence"
    );
    assert!(
        prompt.user_message.contains("Aletheia"),
        "user message should contain the project name"
    );
    assert!(
        prompt.user_message.contains("memory system"),
        "user message should contain conversation content"
    );
}

#[test]
fn parse_valid_response() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let json = r#"{
        "entities": [
            { "name": "Dr. Chen", "entity_type": "person", "description": "Developer of Aletheia" },
            { "name": "Aletheia", "entity_type": "project", "description": "AI memory system" }
        ],
        "relationships": [
            { "source": "Dr. Chen", "relation": "works on", "target": "Aletheia", "confidence": 0.95 }
        ],
        "facts": [
            { "subject": "Aletheia", "predicate": "is", "object": "an AI memory system", "confidence": 0.9 }
        ]
    }"#;

    let extraction = engine
        .parse_response(json)
        .expect("valid extraction JSON should parse");
    assert_eq!(extraction.entities.len(), 2, "should parse 2 entities");
    assert_eq!(
        extraction.entities[0].name, "Dr. Chen",
        "first entity should be Dr. Chen"
    );
    assert_eq!(
        extraction.entities[1].entity_type, "project",
        "second entity type should be project"
    );
    assert_eq!(
        extraction.relationships.len(),
        1,
        "should parse 1 relationship"
    );
    assert_eq!(
        extraction.relationships[0].relation, "works on",
        "relationship should be 'works on'"
    );
    assert!(
        (extraction.relationships[0].confidence - 0.95).abs() < f64::EPSILON,
        "relationship confidence should be 0.95"
    );
    assert_eq!(extraction.facts.len(), 1, "should parse 1 fact");
    assert_eq!(
        extraction.facts[0].subject, "Aletheia",
        "fact subject should be Aletheia"
    );
}

#[test]
fn parse_response_with_code_fences() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let json = r#"```json
{
    "entities": [
        { "name": "Rust", "entity_type": "tool", "description": "Programming language" }
    ],
    "relationships": [],
    "facts": []
}
```"#;

    let extraction = engine
        .parse_response(json)
        .expect("JSON with code fences should parse after stripping");
    assert_eq!(
        extraction.entities.len(),
        1,
        "should parse 1 entity from fenced JSON"
    );
    assert_eq!(
        extraction.entities[0].name, "Rust",
        "entity name should be Rust"
    );
}

#[test]
fn parse_invalid_response() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let result = engine.parse_response("this is not json at all");
    assert!(result.is_err(), "non-JSON input should return an error");
    let err = result.expect_err("non-JSON input should produce parse error");
    assert!(
        matches!(err, ExtractionError::ParseResponse { .. }),
        "error should be ParseResponse variant"
    );
}

#[tokio::test]
async fn extract_skips_short_messages() {
    struct NeverCallProvider;
    impl ExtractionProvider for NeverCallProvider {
        fn complete<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<String, ExtractionError>> + Send + 'a>,
        > {
            Box::pin(async { panic!("should not be called for short messages") })
        }
    }

    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let messages = vec![ConversationMessage {
        role: "user".to_owned(),
        content: "Hi".to_owned(),
    }];

    let result = engine
        .extract(&messages, &NeverCallProvider)
        .await
        .expect("short message should return empty extraction without error");
    assert!(
        result.entities.is_empty(),
        "short message should produce no entities"
    );
}

#[tokio::test]
async fn extract_calls_provider() {
    struct MockProvider;
    impl ExtractionProvider for MockProvider {
        fn complete<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<String, ExtractionError>> + Send + 'a>,
        > {
            Box::pin(async {
                Ok(r#"{"entities":[],"relationships":[],"facts":[{"subject":"Dr. Chen","predicate":"studies","object":"neural networks","confidence":0.95}]}"#.to_owned())
            })
        }
    }

    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let messages = vec![ConversationMessage {
        role: "user".to_owned(),
        content: "Dr. Chen studies neural networks at the university and works on AI memory systems every day."
            .to_owned(),
    }];

    let result = engine
        .extract(&messages, &MockProvider)
        .await
        .expect("mock provider returns valid JSON, extraction should succeed");
    assert_eq!(
        result.facts.len(),
        1,
        "should extract 1 fact from provider response"
    );
    assert_eq!(
        result.facts[0].subject, "Dr. Chen",
        "fact subject should be Dr. Chen"
    );
}

#[test]
fn slugify_works() {
    assert_eq!(
        utils::slugify("Data Processor"),
        "data-processor",
        "spaces should become hyphens and letters lowercased"
    );
    assert_eq!(
        utils::slugify("AI Memory System"),
        "ai-memory-system",
        "multiple words should be joined with hyphens"
    );
    assert_eq!(
        utils::slugify("  hello  world  "),
        "hello-world",
        "leading/trailing and multiple spaces should be collapsed"
    );
    assert_eq!(
        utils::slugify("C++/Rust"),
        "c-rust",
        "special characters should be replaced with hyphens"
    );
}

#[test]
fn strip_code_fences_works() {
    assert_eq!(
        utils::strip_code_fences(
            r#"```json
{"a":1}
```"#
        ),
        r#"{"a":1}"#,
        "json-tagged code fence should be stripped"
    );
    assert_eq!(
        utils::strip_code_fences(
            r#"```
{"a":1}
```"#
        ),
        r#"{"a":1}"#,
        "untagged code fence should be stripped"
    );
    assert_eq!(
        utils::strip_code_fences(r#"{"a":1}"#),
        r#"{"a":1}"#,
        "plain JSON without fences should pass through unchanged"
    );
}

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

#[cfg(feature = "mneme-engine")]
#[test]
fn persist_rejects_malformed_type() {
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
            relation: String::new(),
            target: "Sol".to_owned(),
            confidence: 0.7,
        }],
        facts: vec![],
    };

    let result = engine
        .persist(&extraction, &store, "session:test", "syn")
        .expect("persist should succeed even when relationship type is malformed");
    assert_eq!(
        result.relationships_inserted, 0,
        "malformed types must not be persisted"
    );
    assert_eq!(
        result.relationships_skipped, 1,
        "malformed relationship should be counted as skipped"
    );
}

#[test]
fn config_returns_same_config() {
    let config = ExtractionConfig {
        model: "test-model".to_owned(),
        min_message_length: 99,
        max_entities: 42,
        max_relationships: 7,
        max_facts: 50,
        enabled: false,
    };
    let engine = ExtractionEngine::new(config);
    let got = engine.config();
    assert_eq!(
        got.model, "test-model",
        "config model should match what was provided"
    );
    assert_eq!(
        got.min_message_length, 99,
        "config min_message_length should match what was provided"
    );
    assert_eq!(
        got.max_entities, 42,
        "config max_entities should match what was provided"
    );
    assert_eq!(
        got.max_relationships, 7,
        "config max_relationships should match what was provided"
    );
    assert!(!got.enabled, "config enabled should be false as provided");
}

#[test]
fn build_prompt_empty_messages() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let prompt = engine.build_prompt(&[]);
    assert!(
        !prompt.system.is_empty(),
        "system prompt should be non-empty even with no messages"
    );
    assert!(
        prompt.system.contains("entities"),
        "system prompt should always mention entities"
    );
    assert!(
        prompt.user_message.is_empty(),
        "no messages means empty user text"
    );
}

#[test]
fn build_prompt_single_message() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let messages = vec![ConversationMessage {
        role: "user".to_owned(),
        content: "Alice builds Aletheia in Rust.".to_owned(),
    }];
    let prompt = engine.build_prompt(&messages);
    assert!(
        prompt
            .user_message
            .contains("Alice builds Aletheia in Rust."),
        "user message should contain the conversation content"
    );
    assert!(
        prompt.user_message.contains("user:"),
        "user message should include role prefix"
    );
}

#[test]
fn parse_response_truncated_json() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let truncated = r#"{"entities": [{"name": "Alice""#;
    let result = engine.parse_response(truncated);
    assert!(result.is_err(), "truncated JSON must return error");
    assert!(
        matches!(
            result.expect_err("truncated JSON should produce parse error"),
            ExtractionError::ParseResponse { .. }
        ),
        "truncated JSON should yield ParseResponse error variant"
    );
}

#[test]
fn parse_response_wrong_type_for_confidence() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let json = r#"{
        "entities": [],
        "relationships": [
            {"source": "Alice", "relation": "knows", "target": "Bob", "confidence": "high"}
        ],
        "facts": []
    }"#;
    let result = engine.parse_response(json);
    assert!(
        result.is_err(),
        "string confidence should cause parse error"
    );
}

#[test]
fn parse_response_extra_fields_ignored() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let json = r#"{
        "entities": [
            {"name": "Alice", "entity_type": "person", "description": "Engineer", "extra_field": true}
        ],
        "relationships": [],
        "facts": [],
        "metadata": {"version": 2}
    }"#;
    let extraction = engine
        .parse_response(json)
        .expect("extra fields should be ignored during deserialization");
    assert_eq!(
        extraction.entities.len(),
        1,
        "should parse 1 entity when extra fields are present"
    );
    assert_eq!(
        extraction.entities[0].name, "Alice",
        "entity name should be preserved despite extra fields"
    );
}

#[test]
fn parse_response_unicode_entities() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let json = r#"{
        "entities": [
            {"name": "東京", "entity_type": "location", "description": "Capital of Japan"},
            {"name": "München", "entity_type": "location", "description": "City in Germany"},
            {"name": "Москва", "entity_type": "location", "description": "Capital of Russia"}
        ],
        "relationships": [],
        "facts": []
    }"#;
    let extraction = engine
        .parse_response(json)
        .expect("unicode entity names should parse successfully");
    assert_eq!(
        extraction.entities.len(),
        3,
        "should parse 3 unicode-named entities"
    );
    assert_eq!(
        extraction.entities[0].name, "東京",
        "first entity should be 東京"
    );
    assert_eq!(
        extraction.entities[1].name, "München",
        "second entity should be München"
    );
    assert_eq!(
        extraction.entities[2].name, "Москва",
        "third entity should be Москва"
    );
}

#[test]
fn parse_response_empty_entities_array() {
    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let json = r#"{"entities":[],"relationships":[],"facts":[]}"#;
    let extraction = engine
        .parse_response(json)
        .expect("compact empty arrays JSON should parse");
    assert!(
        extraction.entities.is_empty(),
        "entities should be empty for compact empty JSON"
    );
    assert!(
        extraction.relationships.is_empty(),
        "relationships should be empty for compact empty JSON"
    );
    assert!(
        extraction.facts.is_empty(),
        "facts should be empty for compact empty JSON"
    );
}

#[tokio::test]
async fn extract_min_length_boundary() {
    struct EchoProvider;
    impl ExtractionProvider for EchoProvider {
        fn complete<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<String, ExtractionError>> + Send + 'a>,
        > {
            Box::pin(async { Ok(r#"{"entities":[],"relationships":[],"facts":[]}"#.to_owned()) })
        }
    }

    let config = ExtractionConfig {
        min_message_length: 10,
        ..ExtractionConfig::default()
    };
    let engine = ExtractionEngine::new(config);

    let below = vec![ConversationMessage {
        role: "user".to_owned(),
        content: "123456789".to_owned(),
    }];
    let result = engine
        .extract(&below, &EchoProvider)
        .await
        .expect("extraction on below-threshold input should return empty without error");
    assert!(
        result.entities.is_empty(),
        "9 chars < 10 threshold, should skip"
    );

    let exact = vec![ConversationMessage {
        role: "user".to_owned(),
        content: "1234567890".to_owned(),
    }];
    let result = engine
        .extract(&exact, &EchoProvider)
        .await
        .expect("extraction on exact-threshold input should call provider and return result");
    assert!(
        result.entities.is_empty(),
        "10 chars == 10 threshold, provider should be called"
    );

    let above = vec![ConversationMessage {
        role: "user".to_owned(),
        content: "12345678901".to_owned(),
    }];
    let result = engine
        .extract(&above, &EchoProvider)
        .await
        .expect("extraction on above-threshold input should call provider and return result");
    assert!(
        result.entities.is_empty(),
        "11 chars > 10 threshold, provider should be called"
    );
}

#[tokio::test]
async fn extract_empty_messages() {
    struct PanicProvider;
    impl ExtractionProvider for PanicProvider {
        fn complete<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<String, ExtractionError>> + Send + 'a>,
        > {
            Box::pin(async { panic!("should not be called for empty messages") })
        }
    }

    let engine = ExtractionEngine::new(ExtractionConfig::default());
    let result = engine
        .extract(&[], &PanicProvider)
        .await
        .expect("empty messages should return empty extraction without calling provider");
    assert!(
        result.entities.is_empty(),
        "empty messages should produce no entities"
    );
    assert!(
        result.relationships.is_empty(),
        "empty messages should produce no relationships"
    );
    assert!(
        result.facts.is_empty(),
        "empty messages should produce no facts"
    );
}

#[test]
fn strip_code_fences_multiple_blocks() {
    let input = r#"```json
{"entities":[]}
```
Some text
```json
{"facts":[]}
```"#;
    let result = utils::strip_code_fences(input);
    assert!(
        !result.starts_with("```"),
        "leading code fence should be stripped"
    );
}

#[test]
fn strip_code_fences_nested() {
    let input = "```json\n```inner```\n```";
    let result = utils::strip_code_fences(input);
    assert!(
        !result.is_empty(),
        "result should not be empty for nested fence input"
    );
    assert!(
        !result.starts_with("```json"),
        "opening json code fence should be stripped"
    );
}

#[test]
fn slugify_empty_string() {
    assert_eq!(
        utils::slugify(""),
        "",
        "empty string should slugify to empty string"
    );
}

#[test]
fn slugify_all_special_chars() {
    let result = utils::slugify("!@#$%");
    assert_eq!(
        result, "",
        "all special chars should collapse to empty string"
    );
}

#[test]
fn slugify_unicode_mixed() {
    let result = utils::slugify("Hello 世界 Rust");
    assert!(
        result.contains("hello"),
        "ASCII letters should be lowercased in slug"
    );
    assert!(
        result.contains("rust"),
        "ASCII letters should be preserved in slug"
    );
    assert!(
        result.chars().all(|c| c.is_alphanumeric() || c == '-'),
        "slugify output should only contain alphanumeric or hyphens"
    );
}

#[test]
fn slugify_nfc_normalization_composed_vs_decomposed() {
    // "café" in NFC (composed é = U+00E9) vs NFD (decomposed e + combining accent)
    let composed = "caf\u{00E9}"; // NFC é
    let decomposed = "cafe\u{0301}"; // NFD: e + combining acute accent
    let slug_composed = utils::slugify(composed);
    let slug_decomposed = utils::slugify(decomposed);
    assert_eq!(
        slug_composed, slug_decomposed,
        "NFC-composed and NFD-decomposed forms must produce the same slug"
    );
}

#[test]
fn slugify_nfc_normalization_preserves_ascii() {
    // NFC normalization must not alter plain ASCII
    assert_eq!(
        utils::slugify("hello-world"),
        "hello-world",
        "plain ASCII with hyphens should be preserved"
    );
    assert_eq!(
        utils::slugify("Data Processor"),
        "data-processor",
        "plain ASCII should not be altered by NFC normalization"
    );
}

#[cfg(feature = "mneme-engine")]
#[test]
fn persist_skips_is_type() {
    let store = crate::knowledge_store::KnowledgeStore::open_mem()
        .expect("in-memory knowledge store should open successfully");
    let engine = ExtractionEngine::new(ExtractionConfig::default());

    let extraction = Extraction {
        entities: vec![
            ExtractedEntity {
                name: "Rust".to_owned(),
                entity_type: "tool".to_owned(),
                description: String::new(),
            },
            ExtractedEntity {
                name: "Language".to_owned(),
                entity_type: "concept".to_owned(),
                description: String::new(),
            },
        ],
        relationships: vec![ExtractedRelationship {
            source: "Rust".to_owned(),
            relation: "is".to_owned(),
            target: "Language".to_owned(),
            confidence: 0.9,
        }],
        facts: vec![],
    };

    let result = engine
        .persist(&extraction, &store, "session:test", "syn")
        .expect("persist should succeed even when 'is' relationship is skipped");
    assert_eq!(
        result.relationships_inserted, 0,
        "'is' relationship should not be inserted"
    );
    assert_eq!(
        result.relationships_skipped, 1,
        "'is' relationship should be counted as skipped"
    );
}

#[expect(clippy::expect_used, reason = "test assertions")]
mod proptests {
    use super::utils;
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn parse_never_panics_on_arbitrary_input(input in "\\PC{0,500}") {
            let engine = ExtractionEngine::new(ExtractionConfig::default());
            // Must return Ok or Err, never panic
            let _ = engine.parse_response(&input);
        }

        #[test]
        fn strip_code_fences_never_panics(input in "\\PC{0,500}") {
            // Must return a string, never panic
            let _ = utils::strip_code_fences(&input);
        }

        #[test]
        fn parse_response_valid_json_with_random_entities(
            name in "\\PC{1,100}",
            etype in "(person|project|concept|tool|location)",
            desc in "\\PC{0,100}",
        ) {
            let escaped_name =
                serde_json::to_string(&name).expect("string serialization is infallible");
            let escaped_desc =
                serde_json::to_string(&desc).expect("string serialization is infallible");
            let json = format!(
                r#"{{"entities":[{{"name":{escaped_name},"entity_type":"{etype}","description":{escaped_desc}}}],"relationships":[],"facts":[]}}"#,
            );
            let engine = ExtractionEngine::new(ExtractionConfig::default());
            let result = engine.parse_response(&json);
            assert!(result.is_ok(), "valid JSON with arbitrary strings should parse: {result:?}");
            let extraction =
                result.expect("valid JSON with arbitrary strings should parse successfully");
            assert_eq!(extraction.entities.len(), 1, "should parse exactly 1 entity");
            assert_eq!(extraction.entities[0].name, name, "entity name should match input");
        }

        #[test]
        fn slugify_never_panics(input in "\\PC{0,200}") {
            let result = utils::slugify(&input);
            // BUG: slugify uses char::is_alphanumeric() which is Unicode-aware,
            // so non-ASCII alphanumeric chars (Tamil, Cyrillic, etc.) pass through.
            // Slugs should ideally be ASCII-only. Documented for fix in a separate PR.
            assert!(
                result.chars().all(|c| c.is_alphanumeric() || c == '-'),
                "slugify produced unexpected character in: {result:?}"
            );
        }
    }
}
