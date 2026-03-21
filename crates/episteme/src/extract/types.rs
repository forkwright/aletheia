use serde::{Deserialize, Serialize};

use super::refinement;

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
    /// Whether this fact is a correction of prior information.
    ///
    /// Detected by heuristic patterns (e.g. "actually, it's X not Y").
    /// Corrections get a +0.2 confidence boost (capped at 1.0) and
    /// skip the SUPPLEMENTS path in conflict detection.
    #[serde(default)]
    pub is_correction: bool,
    /// Classified fact type for FSRS decay tuning.
    #[serde(default)]
    pub fact_type: Option<String>,
}

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
    /// Maximum facts to extract per conversation segment.
    pub max_facts: usize,
    /// Whether extraction is active.
    pub enabled: bool,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            model: "claude-haiku-4-5-20251001".to_owned(),
            min_message_length: 50,
            max_entities: 20,
            max_relationships: 30,
            max_facts: 50,
            enabled: true,
        }
    }
}

/// A lightweight conversation message for the extraction pipeline.
///
/// Decoupled from mneme's full [`aletheia_graphe::types::Message`] to keep
/// the extraction engine independent of the session store.
#[derive(Debug, Clone)]
pub struct ConversationMessage {
    /// Message role (e.g. "user", "assistant").
    pub role: String,
    /// Message text content.
    pub content: String,
}

/// The system prompt and user message for an extraction LLM call.
#[derive(Debug, Clone)]
pub struct ExtractionPrompt {
    /// System prompt with JSON schema and extraction rules.
    pub system: String,
    /// Concatenated conversation text for the user message.
    pub user_message: String,
}

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
