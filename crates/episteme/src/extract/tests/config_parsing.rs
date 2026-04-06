//! Tests for extraction config and basic response parsing.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use super::super::utils;
use super::super::*;

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
