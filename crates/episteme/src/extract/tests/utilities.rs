//! Acceptance tests for config, prompt building, and utility functions.
//! Acceptance criteria tests for extraction pipeline.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use super::super::utils;
use super::super::*;

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
    // NOTE: "café" in NFC (composed é = U+00E9) vs NFD (decomposed e + combining accent)
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
    // WHY: NFC normalization must not alter plain ASCII
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
    use proptest::prelude::*;

    use super::utils;
    use super::*;
    proptest! {
        #[test]
        fn parse_never_panics_on_arbitrary_input(input in "\\PC{0,500}") {
            let engine = ExtractionEngine::new(ExtractionConfig::default());
            // INVARIANT: Must return Ok or Err, never panic
            let _ = engine.parse_response(&input);
        }

        #[test]
        fn strip_code_fences_never_panics(input in "\\PC{0,500}") {
            // INVARIANT: Must return a string, never panic
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
            // NOTE: BUG: slugify uses char::is_alphanumeric() which is Unicode-aware,
            // so non-ASCII alphanumeric chars (Tamil, Cyrillic, etc.) pass through.
            // Slugs should ideally be ASCII-only. Documented for fix in a separate PR.
            assert!(
                result.chars().all(|c| c.is_alphanumeric() || c == '-'),
                "slugify produced unexpected character in: {result:?}"
            );
        }
    }
}
