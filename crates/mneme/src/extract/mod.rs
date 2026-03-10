//! Knowledge extraction pipeline — LLM-driven entity/relationship/fact extraction.

/// Context-dependent extraction refinement: turn classification, correction
/// detection, quality filters, and fact type classification.
pub mod refinement;

use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use tracing::instrument;

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
    /// Whether this fact corrects a previously stated fact.
    #[serde(default)]
    pub is_correction: bool,
    /// Classified fact type for FSRS decay tuning.
    #[serde(default)]
    pub fact_type: Option<String>,
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
    #[instrument(skip(config))]
    pub fn new(config: ExtractionConfig) -> Self {
        Self { config }
    }

    /// Access the extraction configuration.
    #[must_use]
    #[instrument(skip(self))]
    pub fn config(&self) -> &ExtractionConfig {
        &self.config
    }

    /// Build the system prompt and user message for knowledge extraction.
    ///
    /// When `turn_type` is `Some`, appends context-dependent extraction
    /// instructions to the base prompt.
    #[must_use]
    #[instrument(skip(self, messages), fields(msg_count = messages.len()))]
    pub fn build_prompt(&self, messages: &[ConversationMessage]) -> ExtractionPrompt {
        self.build_prompt_with_turn_type(messages, None)
    }

    /// Build the extraction prompt with optional turn-type-specific instructions.
    #[must_use]
    #[instrument(skip(self, messages), fields(msg_count = messages.len(), turn_type))]
    pub fn build_prompt_with_turn_type(
        &self,
        messages: &[ConversationMessage],
        turn_type: Option<refinement::TurnType>,
    ) -> ExtractionPrompt {
        let mut system = format!(
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

        if let Some(tt) = turn_type {
            system.push_str("\n\nContext-specific instructions:\n");
            system.push_str(tt.prompt_appendix());
        }

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
    #[instrument(skip(self, response))]
    pub fn parse_response(&self, response: &str) -> Result<Extraction, ExtractionError> {
        let trimmed = strip_code_fences(response);
        serde_json::from_str(trimmed).context(ParseResponseSnafu)
    }

    /// Run extraction end-to-end: build prompt, call provider, parse response.
    #[instrument(skip(self, provider))]
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

    /// Run extraction with context-dependent refinement.
    ///
    /// Classifies the turn, applies per-type prompt instructions, detects
    /// corrections, classifies fact types, applies quality filters, and boosts
    /// confidence where appropriate.
    #[instrument(skip(self, provider))]
    pub fn extract_refined(
        &self,
        messages: &[ConversationMessage],
        provider: &dyn ExtractionProvider,
    ) -> Result<RefinedExtraction, ExtractionError> {
        let total_len: usize = messages.iter().map(|m| m.content.len()).sum();
        if total_len < self.config.min_message_length {
            return Ok(RefinedExtraction {
                extraction: Extraction {
                    entities: vec![],
                    relationships: vec![],
                    facts: vec![],
                },
                turn_type: refinement::TurnType::Discussion,
                facts_filtered: 0,
            });
        }

        // Classify the turn from combined content
        let combined: String = messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let turn_type = refinement::classify_turn(&combined);

        // Build prompt with turn-type-specific instructions
        let prompt = self.build_prompt_with_turn_type(messages, Some(turn_type));
        let response = provider.complete(&prompt.system, &prompt.user_message)?;
        let mut extraction = self.parse_response(&response)?;

        // Detect corrections in source content
        let correction = refinement::detect_correction(&combined);

        // Post-process facts: classify type, boost confidence, apply quality filters
        let boost = turn_type.confidence_boost() + correction.confidence_boost;
        let mut filtered_count = 0;

        extraction.facts = extraction
            .facts
            .into_iter()
            .filter_map(|mut fact| {
                // Classify fact type
                let fact_content = format!("{} {} {}", fact.subject, fact.predicate, fact.object);
                let classified_type = refinement::classify_fact(&fact_content);
                fact.fact_type = Some(classified_type.as_str().to_owned());

                // Mark corrections
                if correction.is_correction {
                    fact.is_correction = true;
                }

                // Apply confidence boost
                fact.confidence = refinement::boosted_confidence(fact.confidence, boost);

                // Quality filter
                let filter = refinement::filter_fact(&fact_content, fact.confidence);
                if filter.passed {
                    Some(fact)
                } else {
                    filtered_count += 1;
                    tracing::debug!(
                        subject = %fact.subject,
                        reason = ?filter.reason,
                        "fact filtered out during extraction refinement"
                    );
                    None
                }
            })
            .collect();

        Ok(RefinedExtraction {
            extraction,
            turn_type,
            facts_filtered: filtered_count,
        })
    }

    /// Persist an extraction to the knowledge store.
    #[cfg(feature = "mneme-engine")]
    #[expect(
        clippy::too_many_lines,
        reason = "single logical operation across three entity types; splitting would obscure the parallel structure"
    )]
    #[instrument(skip(self, store))]
    #[expect(
        clippy::too_many_lines,
        reason = "sequential extraction pipeline: entities → relationships → facts"
    )]
    pub fn persist(
        &self,
        extraction: &Extraction,
        store: &crate::knowledge_store::KnowledgeStore,
        source: &str,
        nous_id: &str,
    ) -> Result<PersistResult, ExtractionError> {
        use crate::knowledge::{Entity, EpistemicTier, Fact, Relationship, far_future};

        let now = jiff::Timestamp::now();
        let mut result = PersistResult::default();

        for entity in &extraction.entities {
            let id = crate::id::EntityId::from(slugify(&entity.name));
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
                created_at: now,
                updated_at: now,
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
                src: crate::id::EntityId::from(slugify(&rel.source)),
                dst: crate::id::EntityId::from(slugify(&rel.target)),
                relation: relation_type,
                weight: rel.confidence,
                created_at: now,
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
            let id = crate::id::FactId::from(format!(
                "{}-{}-{i}",
                slugify(&fact.subject),
                slugify(&fact.predicate)
            ));
            let classified_type = fact
                .fact_type
                .as_deref()
                .map(crate::knowledge::FactType::from_str_lossy)
                .unwrap_or_else(|| crate::knowledge::FactType::classify(&content));
            let f = Fact {
                id,
                nous_id: nous_id.to_owned(),
                content,
                confidence: fact.confidence,
                tier: EpistemicTier::Inferred,
                valid_from: now,
                valid_to: far_future(),
                superseded_by: None,
                source_session_id: Some(source.to_owned()),
                recorded_at: now,
                access_count: 0,
                last_accessed_at: None,
                stability_hours: classified_type.base_stability_hours(),
                fact_type: classified_type.as_str().to_owned(),
                is_forgotten: false,
                forgotten_at: None,
                forget_reason: None,
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
// Refined extraction result
// ---------------------------------------------------------------------------

/// Result of extraction with context-dependent refinement applied.
#[derive(Debug, Clone)]
pub struct RefinedExtraction {
    /// The extraction after quality filters and confidence boosts.
    pub extraction: Extraction,
    /// The classified turn type.
    pub turn_type: refinement::TurnType,
    /// Number of facts filtered out by quality checks.
    pub facts_filtered: usize,
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

/// Slugify a string: NFC-normalize, lowercase, spaces to hyphens, keep alphanumeric and hyphens.
///
/// Unicode Normalization Form C is applied first so that visually identical strings
/// with different codepoint sequences (e.g. composed vs decomposed "café") produce the
/// same slug.
#[cfg(any(feature = "mneme-engine", test))]
fn slugify(s: &str) -> String {
    use unicode_normalization::UnicodeNormalization as _;
    let normalized: String = s.nfc().collect();
    normalized
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
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
        assert_eq!(extraction.entities.len(), 2);
        assert_eq!(extraction.entities[0].name, "Dr. Chen");
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

        let extraction = engine
            .parse_response(json)
            .expect("JSON with code fences should parse after stripping");
        assert_eq!(extraction.entities.len(), 1);
        assert_eq!(extraction.entities[0].name, "Rust");
    }

    #[test]
    fn parse_invalid_response() {
        let engine = ExtractionEngine::new(ExtractionConfig::default());
        let result = engine.parse_response("this is not json at all");
        assert!(result.is_err());
        let err = result.expect_err("non-JSON input should produce parse error");
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

        let result = engine
            .extract(&messages, &NeverCallProvider)
            .expect("short message should return empty extraction without error");
        assert!(result.entities.is_empty());
    }

    #[test]
    fn extract_calls_provider() {
        struct MockProvider;
        impl ExtractionProvider for MockProvider {
            fn complete(&self, _: &str, _: &str) -> Result<String, ExtractionError> {
                Ok(r#"{"entities":[],"relationships":[],"facts":[{"subject":"Dr. Chen","predicate":"studies","object":"neural networks","confidence":0.95}]}"#.to_owned())
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
            .expect("mock provider returns valid JSON, extraction should succeed");
        assert_eq!(result.facts.len(), 1);
        assert_eq!(result.facts[0].subject, "Dr. Chen");
    }

    #[test]
    fn slugify_works() {
        assert_eq!(slugify("Data Processor"), "data-processor");
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

    // --- Acceptance criteria tests (prompt 99) ---

    #[test]
    fn parse_empty_extraction() {
        let engine = ExtractionEngine::new(ExtractionConfig::default());
        let json = r#"{"entities": [], "relationships": [], "facts": []}"#;
        let extraction = engine
            .parse_response(json)
            .expect("empty arrays JSON should parse to empty extraction");
        assert!(extraction.entities.is_empty());
        assert!(extraction.relationships.is_empty());
        assert!(extraction.facts.is_empty());
    }

    #[test]
    fn parse_missing_fields_errors() {
        let engine = ExtractionEngine::new(ExtractionConfig::default());
        // Missing required fields — serde_json requires all fields on Extraction.
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
        // NOTE: The parser does NOT clamp confidence values — it stores them as-is.
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
        assert_eq!(extraction.entities.len(), 6);
        // entity_type is a free-form string — no validation at parse time
        assert_eq!(extraction.entities[5].entity_type, "unknown_type");
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
        assert_eq!(extraction.facts.len(), 5);
    }

    #[test]
    fn parse_does_not_deduplicate_entities() {
        let engine = ExtractionEngine::new(ExtractionConfig::default());
        // NOTE: The parser does NOT deduplicate entities — it returns them as-is.
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
        assert_eq!(strip_code_fences(input), r#"{"a":1}"#);
    }

    #[test]
    fn strip_code_fences_no_closing_fence() {
        // LLM sometimes forgets the closing fence
        let input = "```json\n{\"a\":1}";
        let result = strip_code_fences(input);
        // Should still produce parseable JSON (strips prefix, returns rest)
        assert!(result.contains(r#"{"a":1}"#));
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
        assert!(first_pos < second_pos);
        assert!(second_pos < third_pos);
    }

    #[test]
    fn extract_provider_error_propagates() {
        struct FailingProvider;
        impl ExtractionProvider for FailingProvider {
            fn complete(&self, _: &str, _: &str) -> Result<String, ExtractionError> {
                LlmCallSnafu {
                    message: "rate limited",
                }
                .fail()
            }
        }

        let engine = ExtractionEngine::new(ExtractionConfig::default());
        let messages = vec![ConversationMessage {
            role: "user".to_owned(),
            content:
                "Alice works on Aletheia, an AI memory system built in Rust for agent cognition."
                    .to_owned(),
        }];

        let result = engine.extract(&messages, &FailingProvider);
        assert!(result.is_err());
        assert!(matches!(
            result.expect_err("failing provider should return LlmCall error"),
            ExtractionError::LlmCall { .. }
        ));
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
        assert_eq!(result.entities_inserted, 2);
        assert_eq!(result.relationships_inserted, 1);
        assert_eq!(result.relationships_skipped, 0);
        assert_eq!(result.facts_inserted, 1);

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
        assert_eq!(result.relationships_inserted, 0);
        assert_eq!(result.relationships_skipped, 1);
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
        assert_eq!(result.relationships_inserted, 1);
        assert_eq!(result.relationships_skipped, 0);

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
    fn persist_accepts_unknown_type() {
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
            .expect("persist should succeed with unknown relationship type");
        assert_eq!(result.relationships_inserted, 1);
        assert_eq!(result.relationships_skipped, 0);
    }

    #[test]
    fn config_returns_same_config() {
        let config = ExtractionConfig {
            model: "test-model".to_owned(),
            min_message_length: 99,
            max_entities: 42,
            max_relationships: 7,
            enabled: false,
        };
        let engine = ExtractionEngine::new(config);
        let got = engine.config();
        assert_eq!(got.model, "test-model");
        assert_eq!(got.min_message_length, 99);
        assert_eq!(got.max_entities, 42);
        assert_eq!(got.max_relationships, 7);
        assert!(!got.enabled);
    }

    #[test]
    fn build_prompt_empty_messages() {
        let engine = ExtractionEngine::new(ExtractionConfig::default());
        let prompt = engine.build_prompt(&[]);
        assert!(
            !prompt.system.is_empty(),
            "system prompt should be non-empty even with no messages"
        );
        assert!(prompt.system.contains("entities"));
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
                .contains("Alice builds Aletheia in Rust.")
        );
        assert!(prompt.user_message.contains("user:"));
    }

    #[test]
    fn parse_response_truncated_json() {
        let engine = ExtractionEngine::new(ExtractionConfig::default());
        let truncated = r#"{"entities": [{"name": "Alice""#;
        let result = engine.parse_response(truncated);
        assert!(result.is_err(), "truncated JSON must return error");
        assert!(matches!(
            result.expect_err("truncated JSON should produce parse error"),
            ExtractionError::ParseResponse { .. }
        ));
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
        assert_eq!(extraction.entities.len(), 1);
        assert_eq!(extraction.entities[0].name, "Alice");
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
        assert_eq!(extraction.entities.len(), 3);
        assert_eq!(extraction.entities[0].name, "東京");
        assert_eq!(extraction.entities[1].name, "München");
        assert_eq!(extraction.entities[2].name, "Москва");
    }

    #[test]
    fn parse_response_empty_entities_array() {
        let engine = ExtractionEngine::new(ExtractionConfig::default());
        let json = r#"{"entities":[],"relationships":[],"facts":[]}"#;
        let extraction = engine
            .parse_response(json)
            .expect("compact empty arrays JSON should parse");
        assert!(extraction.entities.is_empty());
        assert!(extraction.relationships.is_empty());
        assert!(extraction.facts.is_empty());
    }

    #[test]
    fn extract_min_length_boundary() {
        struct EchoProvider;
        impl ExtractionProvider for EchoProvider {
            fn complete(&self, _: &str, _: &str) -> Result<String, ExtractionError> {
                Ok(r#"{"entities":[],"relationships":[],"facts":[]}"#.to_owned())
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
            .expect("extraction on above-threshold input should call provider and return result");
        assert!(
            result.entities.is_empty(),
            "11 chars > 10 threshold, provider should be called"
        );
    }

    #[test]
    fn extract_empty_messages() {
        struct PanicProvider;
        impl ExtractionProvider for PanicProvider {
            fn complete(&self, _: &str, _: &str) -> Result<String, ExtractionError> {
                panic!("should not be called for empty messages");
            }
        }

        let engine = ExtractionEngine::new(ExtractionConfig::default());
        let result = engine
            .extract(&[], &PanicProvider)
            .expect("empty messages should return empty extraction without calling provider");
        assert!(result.entities.is_empty());
        assert!(result.relationships.is_empty());
        assert!(result.facts.is_empty());
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
        let result = strip_code_fences(input);
        assert!(
            !result.starts_with("```"),
            "leading code fence should be stripped"
        );
    }

    #[test]
    fn strip_code_fences_nested() {
        let input = "```json\n```inner```\n```";
        let result = strip_code_fences(input);
        assert!(!result.is_empty());
        assert!(!result.starts_with("```json"));
    }

    #[test]
    fn slugify_empty_string() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn slugify_all_special_chars() {
        let result = slugify("!@#$%");
        assert_eq!(
            result, "",
            "all special chars should collapse to empty string"
        );
    }

    #[test]
    fn slugify_unicode_mixed() {
        let result = slugify("Hello 世界 Rust");
        assert!(result.contains("hello"));
        assert!(result.contains("rust"));
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
        let slug_composed = slugify(composed);
        let slug_decomposed = slugify(decomposed);
        assert_eq!(
            slug_composed, slug_decomposed,
            "NFC-composed and NFD-decomposed forms must produce the same slug"
        );
    }

    #[test]
    fn slugify_nfc_normalization_preserves_ascii() {
        // NFC normalization must not alter plain ASCII
        assert_eq!(slugify("hello-world"), "hello-world");
        assert_eq!(slugify("Data Processor"), "data-processor");
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
        assert_eq!(result.relationships_inserted, 0);
        assert_eq!(result.relationships_skipped, 1);
    }
}

#[cfg(test)]
mod proptests {
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
            let _ = strip_code_fences(&input);
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
            assert_eq!(extraction.entities.len(), 1);
            assert_eq!(extraction.entities[0].name, name);
        }

        #[test]
        fn slugify_never_panics(input in "\\PC{0,200}") {
            let result = slugify(&input);
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
