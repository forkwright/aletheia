//! Knowledge extraction pipeline — LLM-driven entity/relationship/fact extraction.

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from the knowledge extraction pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum ExtractionError {
    /// The LLM response could not be parsed as valid extraction JSON.
    #[snafu(display("failed to parse extraction response"))]
    ParseResponse {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The LLM provider returned an error during extraction.
    #[snafu(display("LLM extraction failed: {message}"))]
    LlmCall {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// Persisting extracted knowledge to the store failed.
    #[snafu(display("failed to persist extraction: {message}"))]
    Persist {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

// ---------------------------------------------------------------------------
// Extraction types
// ---------------------------------------------------------------------------

/// Extracted knowledge from a conversation segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Extraction {
    /// Named entities found in the conversation.
    pub entities: Vec<ExtractedEntity>,
    /// Relationships between entities.
    pub relationships: Vec<ExtractedRelationship>,
    /// Factual claims as subject-predicate-object triples.
    pub facts: Vec<ExtractedFact>,
}

/// A named entity extracted from conversation text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    /// Normalized entity name (proper noun form).
    pub name: String,
    /// Category: person, project, concept, tool, or location.
    pub entity_type: String,
    /// Brief description of the entity from context.
    pub description: String,
}

/// A directed relationship between two extracted entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRelationship {
    /// Entity name (source).
    pub source: String,
    /// Verb phrase: "works on", "depends on", "created by".
    pub relation: String,
    /// Entity name (target).
    pub target: String,
    /// 0.0–1.0.
    pub confidence: f64,
}

/// A factual claim extracted as a subject-predicate-object triple.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedFact {
    /// The entity or concept the fact is about.
    pub subject: String,
    /// The relationship verb phrase.
    pub predicate: String,
    /// The object of the claim.
    pub object: String,
    /// Confidence score (0.0–1.0).
    pub confidence: f64,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the knowledge extraction pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionConfig {
    /// LLM model to use for extraction.
    pub model: String,
    /// Minimum total message length (chars) before extraction triggers.
    pub min_message_length: usize,
    /// Maximum entities to extract per conversation segment.
    pub max_entities: usize,
    /// Maximum relationships to extract per conversation segment.
    pub max_relationships: usize,
    /// Whether extraction is active.
    pub enabled: bool,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            model: "claude-haiku-4-5-20251001".to_owned(),
            min_message_length: 50,
            max_entities: 10,
            max_relationships: 15,
            enabled: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Provider trait
// ---------------------------------------------------------------------------

/// Minimal LLM completion interface for extraction.
///
/// Keeps mneme independent of hermeneus. The nous layer bridges this trait
/// to the full `LlmProvider` + `CompletionRequest` API.
pub trait ExtractionProvider: Send + Sync {
    fn complete(&self, system: &str, user_message: &str) -> Result<String, ExtractionError>;
}

// ---------------------------------------------------------------------------
// Conversation message (local lightweight type)
// ---------------------------------------------------------------------------

/// A lightweight conversation message for the extraction pipeline.
///
/// Decoupled from mneme's full [`crate::types::Message`] to keep
/// the extraction engine independent of the session store.
#[derive(Debug, Clone)]
pub struct ConversationMessage {
    /// Message role (e.g. "user", "assistant").
    pub role: String,
    /// Message text content.
    pub content: String,
}

// ---------------------------------------------------------------------------
// Prompt output
// ---------------------------------------------------------------------------

/// The system prompt and user message for an extraction LLM call.
#[derive(Debug, Clone)]
pub struct ExtractionPrompt {
    /// System prompt with JSON schema and extraction rules.
    pub system: String,
    /// Concatenated conversation text for the user message.
    pub user_message: String,
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// Drives the extraction pipeline: prompt building, LLM calling, response parsing.
pub struct ExtractionEngine {
    config: ExtractionConfig,
}

impl ExtractionEngine {
    /// Create an extraction engine with the given configuration.
    #[must_use]
    pub fn new(config: ExtractionConfig) -> Self {
        Self { config }
    }

    /// Access the extraction configuration.
    #[must_use]
    pub fn config(&self) -> &ExtractionConfig {
        &self.config
    }

    /// Build the system prompt and user message for knowledge extraction.
    #[must_use]
    pub fn build_prompt(&self, messages: &[ConversationMessage]) -> ExtractionPrompt {
        let system = format!(
            r#"You are a knowledge extraction engine. Analyze the conversation and extract structured knowledge.

Output ONLY valid JSON matching this schema — no commentary, no markdown fences:
{{
  "entities": [
    {{ "name": "...", "entity_type": "person|project|concept|tool|location", "description": "..." }}
  ],
  "relationships": [
    {{ "source": "...", "relation": "verb phrase", "target": "...", "confidence": 0.0-1.0 }}
  ],
  "facts": [
    {{ "subject": "...", "predicate": "...", "object": "...", "confidence": 0.0-1.0 }}
  ]
}}

Rules:
- Extract entities mentioned: people, projects, concepts, tools, locations.
- Extract relationships between entities as verb phrases ("works on", "depends on", "created by").
- Extract factual claims as subject-predicate-object triples.
- Assign confidence: 1.0 for explicit statements, 0.5-0.8 for inferences, below 0.5 for weak signals.
- Normalize entity names: use proper nouns ("Alice" not "she", "Aletheia" not "the project").
- Skip greetings, small talk, and meta-conversation ("let me think about that").
- Maximum {max_entities} entities, {max_relationships} relationships.
- If the conversation contains no extractable knowledge, return empty arrays."#,
            max_entities = self.config.max_entities,
            max_relationships = self.config.max_relationships,
        );

        let mut conversation = String::new();
        for msg in messages {
            conversation.push_str(&msg.role);
            conversation.push_str(": ");
            conversation.push_str(&msg.content);
            conversation.push('\n');
        }

        ExtractionPrompt {
            system,
            user_message: conversation,
        }
    }

    /// Parse a JSON extraction response from the LLM.
    ///
    /// Strips markdown code fences if present.
    pub fn parse_response(&self, response: &str) -> Result<Extraction, ExtractionError> {
        let trimmed = strip_code_fences(response);
        serde_json::from_str(trimmed).context(ParseResponseSnafu)
    }

    /// Run extraction end-to-end: build prompt, call provider, parse response.
    pub fn extract(
        &self,
        messages: &[ConversationMessage],
        provider: &dyn ExtractionProvider,
    ) -> Result<Extraction, ExtractionError> {
        let total_len: usize = messages.iter().map(|m| m.content.len()).sum();
        if total_len < self.config.min_message_length {
            return Ok(Extraction {
                entities: vec![],
                relationships: vec![],
                facts: vec![],
            });
        }

        let prompt = self.build_prompt(messages);
        let response = provider.complete(&prompt.system, &prompt.user_message)?;
        self.parse_response(&response)
    }

    /// Persist an extraction to the knowledge store.
    #[cfg(feature = "mneme-engine")]
    pub fn persist(
        &self,
        extraction: &Extraction,
        store: &crate::knowledge_store::KnowledgeStore,
        source: &str,
        nous_id: &str,
    ) -> Result<PersistResult, ExtractionError> {
        use crate::knowledge::{Entity, EpistemicTier, Fact, Relationship};

        let now = now_iso8601();
        let mut result = PersistResult::default();

        for entity in &extraction.entities {
            let id = slugify(&entity.name);
            let aliases = if entity.description.is_empty() {
                vec![]
            } else {
                vec![entity.description.clone()]
            };
            let e = Entity {
                id,
                name: entity.name.clone(),
                entity_type: entity.entity_type.clone(),
                aliases,
                created_at: now.clone(),
                updated_at: now.clone(),
            };
            store.insert_entity(&e).map_err(|e| {
                PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            result.entities_inserted += 1;
        }

        for rel in &extraction.relationships {
            let relation_type = match crate::vocab::normalize_relation(&rel.relation) {
                crate::vocab::RelationType::Valid(canonical) => canonical.to_owned(),
                crate::vocab::RelationType::Rejected => {
                    tracing::warn!(
                        relation = %rel.relation,
                        source = %rel.source,
                        target = %rel.target,
                        "rejected relationship with banned type"
                    );
                    result.relationships_skipped += 1;
                    continue;
                }
                crate::vocab::RelationType::Unknown(normalized) => {
                    tracing::warn!(
                        relation = %normalized,
                        raw = %rel.relation,
                        source = %rel.source,
                        target = %rel.target,
                        "persisting relationship with unknown type"
                    );
                    normalized
                }
            };
            let r = Relationship {
                src: slugify(&rel.source),
                dst: slugify(&rel.target),
                relation: relation_type,
                weight: rel.confidence,
                created_at: now.clone(),
            };
            store.insert_relationship(&r).map_err(|e| {
                PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            result.relationships_inserted += 1;
        }

        for (i, fact) in extraction.facts.iter().enumerate() {
            let content = format!("{} {} {}", fact.subject, fact.predicate, fact.object);
            let id = format!(
                "{}-{}-{i}",
                slugify(&fact.subject),
                slugify(&fact.predicate)
            );
            let f = Fact {
                id,
                nous_id: nous_id.to_owned(),
                content,
                confidence: fact.confidence,
                tier: EpistemicTier::Inferred,
                valid_from: now.clone(),
                valid_to: "9999-12-31T00:00:00Z".to_owned(),
                superseded_by: None,
                source_session_id: Some(source.to_owned()),
                recorded_at: now.clone(),
            };
            store.insert_fact(&f).map_err(|e| {
                PersistSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;
            result.facts_inserted += 1;
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// Persist result
// ---------------------------------------------------------------------------

/// Counts of knowledge items persisted to the store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersistResult {
    /// Number of entities written.
    pub entities_inserted: usize,
    /// Number of relationships written.
    pub relationships_inserted: usize,
    /// Number of relationships skipped due to validation.
    pub relationships_skipped: usize,
    /// Number of facts written.
    pub facts_inserted: usize,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Strip markdown code fences from an LLM response.
fn strip_code_fences(s: &str) -> &str {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else if let Some(rest) = trimmed.strip_prefix("```") {
        rest.strip_suffix("```").unwrap_or(rest).trim()
    } else {
        trimmed
    }
}

/// Slugify a string: lowercase, spaces to hyphens, keep alphanumeric and hyphens.
#[cfg(any(feature = "mneme-engine", test))]
fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Current time as ISO 8601 string (UTC, second precision).
#[cfg(feature = "mneme-engine")]
fn now_iso8601() -> String {
    use std::time::SystemTime;

    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    // Simple epoch-day to y/m/d (civil calendar from days since 1970-01-01).
    #[expect(clippy::cast_possible_wrap, reason = "epoch days fits in i64")]
    let (y, m, d) = epoch_days_to_ymd(days as i64);
    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Convert epoch days to (year, month, day). Algorithm from Howard Hinnant.
#[cfg(feature = "mneme-engine")]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::similar_names,
    reason = "Hinnant algorithm uses known-range casts and standard doe/doy names"
)]
fn epoch_days_to_ymd(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default() {
        let cfg = ExtractionConfig::default();
        assert_eq!(cfg.model, "claude-haiku-4-5-20251001");
        assert_eq!(cfg.min_message_length, 50);
        assert_eq!(cfg.max_entities, 10);
        assert_eq!(cfg.max_relationships, 15);
        assert!(cfg.enabled);
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
                content: "That sounds like an interesting project. Tell me more about it."
                    .to_owned(),
            },
        ];

        let prompt = engine.build_prompt(&messages);
        assert!(prompt.system.contains("JSON"));
        assert!(prompt.system.contains("entities"));
        assert!(prompt.system.contains("relationships"));
        assert!(prompt.system.contains("facts"));
        assert!(prompt.system.contains("confidence"));
        assert!(prompt.user_message.contains("Aletheia"));
        assert!(prompt.user_message.contains("memory system"));
    }

    #[test]
    fn parse_valid_response() {
        let engine = ExtractionEngine::new(ExtractionConfig::default());
        let json = r#"{
            "entities": [
                { "name": "Alice", "entity_type": "person", "description": "Developer of Aletheia" },
                { "name": "Aletheia", "entity_type": "project", "description": "AI memory system" }
            ],
            "relationships": [
                { "source": "Alice", "relation": "works on", "target": "Aletheia", "confidence": 0.95 }
            ],
            "facts": [
                { "subject": "Aletheia", "predicate": "is", "object": "an AI memory system", "confidence": 0.9 }
            ]
        }"#;

        let extraction = engine.parse_response(json).unwrap();
        assert_eq!(extraction.entities.len(), 2);
        assert_eq!(extraction.entities[0].name, "Alice");
        assert_eq!(extraction.entities[1].entity_type, "project");
        assert_eq!(extraction.relationships.len(), 1);
        assert_eq!(extraction.relationships[0].relation, "works on");
        assert!((extraction.relationships[0].confidence - 0.95).abs() < f64::EPSILON);
        assert_eq!(extraction.facts.len(), 1);
        assert_eq!(extraction.facts[0].subject, "Aletheia");
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

        let extraction = engine.parse_response(json).unwrap();
        assert_eq!(extraction.entities.len(), 1);
        assert_eq!(extraction.entities[0].name, "Rust");
    }

    #[test]
    fn parse_invalid_response() {
        let engine = ExtractionEngine::new(ExtractionConfig::default());
        let result = engine.parse_response("this is not json at all");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ExtractionError::ParseResponse { .. }));
    }

    #[test]
    fn extract_skips_short_messages() {
        struct NeverCallProvider;
        impl ExtractionProvider for NeverCallProvider {
            fn complete(&self, _: &str, _: &str) -> Result<String, ExtractionError> {
                panic!("should not be called for short messages");
            }
        }

        let engine = ExtractionEngine::new(ExtractionConfig::default());
        let messages = vec![ConversationMessage {
            role: "user".to_owned(),
            content: "Hi".to_owned(),
        }];

        let result = engine.extract(&messages, &NeverCallProvider).unwrap();
        assert!(result.entities.is_empty());
    }

    #[test]
    fn extract_calls_provider() {
        struct MockProvider;
        impl ExtractionProvider for MockProvider {
            fn complete(&self, _: &str, _: &str) -> Result<String, ExtractionError> {
                Ok(r#"{"entities":[],"relationships":[],"facts":[{"subject":"Alice","predicate":"lives in","object":"Springfield","confidence":0.95}]}"#.to_owned())
            }
        }

        let engine = ExtractionEngine::new(ExtractionConfig::default());
        let messages = vec![ConversationMessage {
            role: "user".to_owned(),
            content: "Alice lives in Springfield, Oregon and works on AI memory systems every day."
                .to_owned(),
        }];

        let result = engine.extract(&messages, &MockProvider).unwrap();
        assert_eq!(result.facts.len(), 1);
        assert_eq!(result.facts[0].subject, "Alice");
    }

    #[test]
    fn slugify_works() {
        assert_eq!(slugify("Alice Johnson"), "alice-johnson");
        assert_eq!(slugify("AI Memory System"), "ai-memory-system");
        assert_eq!(slugify("  hello  world  "), "hello-world");
        assert_eq!(slugify("C++/Rust"), "c-rust");
    }

    #[test]
    fn strip_code_fences_works() {
        assert_eq!(
            strip_code_fences(
                r#"```json
{"a":1}
```"#
            ),
            r#"{"a":1}"#
        );
        assert_eq!(
            strip_code_fences(
                r#"```
{"a":1}
```"#
            ),
            r#"{"a":1}"#
        );
        assert_eq!(strip_code_fences(r#"{"a":1}"#), r#"{"a":1}"#);
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn persist_round_trip() {
        let store = crate::knowledge_store::KnowledgeStore::open_mem().unwrap();
        let engine = ExtractionEngine::new(ExtractionConfig::default());

        let extraction = Extraction {
            entities: vec![
                ExtractedEntity {
                    name: "Alice".to_owned(),
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
                source: "Alice".to_owned(),
                relation: "works on".to_owned(),
                target: "Aletheia".to_owned(),
                confidence: 0.95,
            }],
            facts: vec![ExtractedFact {
                subject: "Aletheia".to_owned(),
                predicate: "uses".to_owned(),
                object: "CozoDB for knowledge storage".to_owned(),
                confidence: 0.9,
            }],
        };

        let result = engine
            .persist(&extraction, &store, "session:test:main:2026-03-02", "syn")
            .unwrap();
        assert_eq!(result.entities_inserted, 2);
        assert_eq!(result.relationships_inserted, 1);
        assert_eq!(result.relationships_skipped, 0);
        assert_eq!(result.facts_inserted, 1);

        // Verify entities are queryable via entity_neighborhood.
        let neighborhood = store.entity_neighborhood("alice").unwrap();
        assert!(
            !neighborhood.rows.is_empty(),
            "alice entity should be reachable in the graph"
        );

        // query_facts filters: valid_from <= now AND valid_to > now
        // Use a future time that's after valid_from but before valid_to.
        let facts = store
            .query_facts("syn", "9999-01-01T00:00:00Z", 100)
            .unwrap();
        assert!(
            facts.iter().any(|f| f.content.contains("CozoDB")),
            "persisted fact should be retrievable"
        );
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn persist_skips_relates_to() {
        let store = crate::knowledge_store::KnowledgeStore::open_mem().unwrap();
        let engine = ExtractionEngine::new(ExtractionConfig::default());

        let extraction = Extraction {
            entities: vec![
                ExtractedEntity {
                    name: "Alice".to_owned(),
                    entity_type: "person".to_owned(),
                    description: String::new(),
                },
                ExtractedEntity {
                    name: "Bob".to_owned(),
                    entity_type: "person".to_owned(),
                    description: String::new(),
                },
            ],
            relationships: vec![ExtractedRelationship {
                source: "Alice".to_owned(),
                relation: "RELATES_TO".to_owned(),
                target: "Bob".to_owned(),
                confidence: 0.8,
            }],
            facts: vec![],
        };

        let result = engine
            .persist(&extraction, &store, "session:test", "syn")
            .unwrap();
        assert_eq!(result.relationships_inserted, 0);
        assert_eq!(result.relationships_skipped, 1);
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn persist_normalizes_relation_type() {
        let store = crate::knowledge_store::KnowledgeStore::open_mem().unwrap();
        let engine = ExtractionEngine::new(ExtractionConfig::default());

        let extraction = Extraction {
            entities: vec![
                ExtractedEntity {
                    name: "Alice".to_owned(),
                    entity_type: "person".to_owned(),
                    description: String::new(),
                },
                ExtractedEntity {
                    name: "Acme".to_owned(),
                    entity_type: "project".to_owned(),
                    description: String::new(),
                },
            ],
            relationships: vec![ExtractedRelationship {
                source: "Alice".to_owned(),
                relation: "works on".to_owned(),
                target: "Acme".to_owned(),
                confidence: 0.9,
            }],
            facts: vec![],
        };

        let result = engine
            .persist(&extraction, &store, "session:test", "syn")
            .unwrap();
        assert_eq!(result.relationships_inserted, 1);
        assert_eq!(result.relationships_skipped, 0);

        let neighborhood = store.entity_neighborhood("alice").unwrap();
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
    fn persist_accepts_unknown_type() {
        let store = crate::knowledge_store::KnowledgeStore::open_mem().unwrap();
        let engine = ExtractionEngine::new(ExtractionConfig::default());

        let extraction = Extraction {
            entities: vec![
                ExtractedEntity {
                    name: "Alice".to_owned(),
                    entity_type: "person".to_owned(),
                    description: String::new(),
                },
                ExtractedEntity {
                    name: "Bob".to_owned(),
                    entity_type: "person".to_owned(),
                    description: String::new(),
                },
            ],
            relationships: vec![ExtractedRelationship {
                source: "Alice".to_owned(),
                relation: "MENTORS".to_owned(),
                target: "Bob".to_owned(),
                confidence: 0.7,
            }],
            facts: vec![],
        };

        let result = engine
            .persist(&extraction, &store, "session:test", "syn")
            .unwrap();
        assert_eq!(result.relationships_inserted, 1);
        assert_eq!(result.relationships_skipped, 0);
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn persist_skips_is_type() {
        let store = crate::knowledge_store::KnowledgeStore::open_mem().unwrap();
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
            .unwrap();
        assert_eq!(result.relationships_inserted, 0);
        assert_eq!(result.relationships_skipped, 1);
    }
}
