use serde::{Deserialize, Serialize};

use super::refinement;
use crate::knowledge::{CausalRelationType, EpistemicTier};
pub use eidos::bookkeeping::{
    ConversationMessage, ExtractedEntity, ExtractedFact, ExtractedRelationship, ExtractedToolCall,
    Extraction, ExtractionSchema,
};
use eidos::workspace::ProjectId;

/// Configuration for the knowledge extraction pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "extraction config bools are independent feature knobs"
)]
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
    /// Bookkeeping provider used by the extraction engine.
    #[serde(default)]
    pub provider: BookkeepingProviderKind,
    /// Whether to extract facts whose subject is a first-person self-reference.
    ///
    /// When `false`, facts with subjects like "I" or obvious assistant
    /// self-references are filtered out during `extract_refined`.
    #[serde(default = "default_true")]
    pub extract_self_facts: bool,
    /// When `true`, the extraction prompt instructs the LLM to capture only
    /// concrete events and observations, excluding self-descriptive,
    /// preference, identity, or meta-relational facts.
    #[serde(default)]
    pub events_only_prompt: bool,
    /// Default epistemic tier assigned to persisted facts.
    #[serde(default = "default_tier_inferred")]
    pub default_tier: EpistemicTier,
    /// Whether to run cohort-respecting conflict detection against the
    /// knowledge store after extraction.
    #[serde(default)]
    pub detect_conflict: bool,
    /// Current project partition applied when persisting project-scoped facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<ProjectId>,
}

const fn default_true() -> bool {
    true
}

const fn default_tier_inferred() -> EpistemicTier {
    EpistemicTier::Inferred
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            model: koina::models::task_role_default(koina::models::TaskRole::Extraction).to_owned(),
            min_message_length: 50,
            max_entities: 20,
            max_relationships: 30,
            max_facts: 50,
            enabled: true,
            provider: BookkeepingProviderKind::Llm,
            extract_self_facts: true,
            events_only_prompt: false,
            default_tier: EpistemicTier::Inferred,
            detect_conflict: false,
            project_id: None,
        }
    }
}

impl ExtractionConfig {
    /// Return the provider-neutral extraction schema for this config.
    #[must_use]
    pub fn schema(&self) -> ExtractionSchema {
        ExtractionSchema {
            max_entities: self.max_entities,
            max_relationships: self.max_relationships,
            max_facts: self.max_facts,
        }
    }
}

/// Bookkeeping provider implementation selected for extraction.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum BookkeepingProviderKind {
    /// Compatibility LLM prompt + parser path.
    #[default]
    Llm,
    /// `GLiNER` ONNX entity adapter with LLM fallback for facts and relationships.
    Gliner,
    /// NuExtract-2.0 ONNX structured JSON extraction provider.
    NuExtract,
}

/// The system prompt and user message for an extraction LLM call.
#[derive(Debug, Clone)]
pub struct ExtractionPrompt {
    /// System prompt with JSON schema and extraction rules.
    pub system: String,
    /// Concatenated conversation text for the user message.
    pub user_message: String,
}

impl ExtractionPrompt {
    /// Construct an extraction prompt.
    #[must_use]
    pub fn new(system: impl Into<String>, user_message: impl Into<String>) -> Self {
        Self {
            system: system.into(),
            user_message: user_message.into(),
        }
    }
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
    /// Causal signal detected in the session text, if any.
    ///
    /// `Some((relation_type, confidence))` when the combined message text
    /// contains causal language ("because", "therefore", "caused by", etc.).
    /// Consumers can use this to drive the crate-private `extract_causal_edges`
    /// helper with the relevant fact IDs.
    pub causal_signal: Option<(CausalRelationType, f64)>,
}

impl RefinedExtraction {
    /// Construct a refined extraction result.
    #[must_use]
    pub fn new(
        extraction: Extraction,
        turn_type: refinement::TurnType,
        facts_filtered: usize,
        causal_signal: Option<(CausalRelationType, f64)>,
    ) -> Self {
        Self {
            extraction,
            turn_type,
            facts_filtered,
            causal_signal,
        }
    }
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
    /// Number of causal edges extracted and recorded.
    pub causal_edges_inserted: usize,
    /// Number of fact-entity edges linked during persistence.
    pub fact_entities_inserted: usize,
}

impl PersistResult {
    /// Return whether no items were inserted and no relationships were skipped.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entities_inserted == 0
            && self.relationships_inserted == 0
            && self.relationships_skipped == 0
            && self.facts_inserted == 0
            && self.causal_edges_inserted == 0
    }
}
