//! Shared bookkeeping provider contracts and extraction DTOs.
//!
//! Bookkeeping providers handle mechanical cognition-adjacent work such as
//! extraction and classification.  Generative LLMs can implement this surface
//! as a compatibility path, but dedicated small-model providers are the target
//! runtime.

use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use snafu::Snafu;

/// Result alias for bookkeeping provider operations.
pub type BookkeepingResult<T> = Result<T, BookkeepingError>;

/// Errors returned by bookkeeping providers.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
#[non_exhaustive]
pub enum BookkeepingError {
    /// The provider failed while executing a supported operation.
    #[snafu(display("{provider} bookkeeping provider failed during {operation}: {message}"))]
    ProviderFailed {
        provider: &'static str,
        operation: &'static str,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
    /// The provider does not implement the requested capability.
    #[snafu(display("{provider} bookkeeping provider does not support {capability}"))]
    Unsupported {
        provider: &'static str,
        capability: &'static str,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Schema limits for a bookkeeping extraction request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionSchema {
    /// Maximum entities to extract per request.
    pub max_entities: usize,
    /// Maximum relationships to extract per request.
    pub max_relationships: usize,
    /// Maximum facts to extract per request.
    pub max_facts: usize,
}

impl Default for ExtractionSchema {
    fn default() -> Self {
        Self {
            max_entities: 20,
            max_relationships: 30,
            max_facts: 50,
        }
    }
}

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

impl Extraction {
    /// Return an empty extraction result.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            entities: Vec::new(),
            relationships: Vec::new(),
            facts: Vec::new(),
        }
    }
}

/// A named entity extracted from conversation text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    /// Normalized entity name.
    pub name: String,
    /// Entity category label, such as `person`, `project`, `concept`, `tool`, or `location`.
    pub entity_type: String,
    /// Brief description of the entity from context.
    pub description: String,
}

/// A directed relationship between two extracted entities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRelationship {
    /// Entity name at the source side of the relationship.
    pub source: String,
    /// Relationship verb phrase.
    pub relation: String,
    /// Entity name at the target side of the relationship.
    pub target: String,
    /// Confidence score from `0.0` to `1.0`.
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
    /// Confidence score from `0.0` to `1.0`.
    pub confidence: f64,
    /// Whether this fact is a correction of prior information.
    #[serde(default)]
    pub is_correction: bool,
    /// Classified fact type for downstream decay tuning.
    #[serde(default)]
    pub fact_type: Option<String>,
}

/// A lightweight representation of a tool call for extraction.
#[derive(Debug, Clone)]
pub struct ExtractedToolCall {
    /// Tool call ID.
    pub id: String, // kanon:ignore RUST/primitive-for-domain-id — raw provider-assigned tool call ID, not a knowledge-domain identifier
    /// Tool name.
    pub name: String,
    /// Input parameters.
    pub input: serde_json::Value,
    /// Result content, if available.
    pub result: Option<String>,
    /// Whether the tool call errored.
    pub is_error: bool,
}

impl ExtractedToolCall {
    /// Construct an extracted tool call.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        input: serde_json::Value,
        result: Option<String>,
        is_error: bool,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            input,
            result,
            is_error,
        }
    }
}

/// A lightweight conversation message for bookkeeping extraction.
#[derive(Debug, Clone)]
pub struct ConversationMessage {
    /// Message role, such as `user` or `assistant`.
    pub role: String,
    /// Message text content.
    pub content: String,
    /// Tool calls made during this turn, if any.
    pub tool_calls: Option<Vec<ExtractedToolCall>>,
    /// Reasoning or thinking blocks generated by the model, if any.
    pub reasoning: Option<String>,
}

impl ConversationMessage {
    /// Construct a plain user message from text.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_owned(),
            content: content.into(),
            tool_calls: None,
            reasoning: None,
        }
    }
}

/// Intent classes available to bookkeeping classifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Intent {
    /// A request to write or modify code.
    CodeWrite,
    /// A request to gather or synthesize research.
    Research,
    /// A request to plan or sequence work.
    Planning,
    /// A request about the agent or system behavior itself.
    Meta,
    /// A request that does not fit a known class.
    Unclassified,
}

/// Canonical entity-type classes available to bookkeeping classifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EntityType {
    /// A person.
    Person,
    /// A project.
    Project,
    /// A concept or abstract topic.
    Concept,
    /// A tool, service, model, or executable system.
    Tool,
    /// A physical or logical location.
    Location,
    /// An organization or company.
    Organization,
    /// Another entity type.
    Other,
}

impl EntityType {
    /// Return the canonical lowercase label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Person => "person",
            Self::Project => "project",
            Self::Concept => "concept",
            Self::Tool => "tool",
            Self::Location => "location",
            Self::Organization => "organization",
            Self::Other => "other",
        }
    }
}

/// Mechanical bookkeeping provider abstraction.
///
/// Implementations may use an LLM compatibility path or dedicated small models.
/// The boxed futures keep the trait object-safe for runtime provider selection.
pub trait BookkeepingProvider: Send + Sync {
    /// Extract structured knowledge from conversation messages.
    fn extract_knowledge<'a>(
        &'a self,
        messages: &'a [ConversationMessage],
        schema: &'a ExtractionSchema,
    ) -> Pin<Box<dyn Future<Output = BookkeepingResult<Extraction>> + Send + 'a>>;

    /// Extract factual triples from plain text.
    fn extract_facts<'a>(
        &'a self,
        text: &'a str,
        schema: &'a ExtractionSchema,
    ) -> Pin<Box<dyn Future<Output = BookkeepingResult<Vec<ExtractedFact>>> + Send + 'a>> {
        Box::pin(async move {
            let messages = [ConversationMessage::user(text)];
            let extraction = self.extract_knowledge(&messages, schema).await?;
            Ok(extraction.facts)
        })
    }

    /// Classify the caller's intent from the provided candidate classes.
    fn classify_intent<'a>(
        &'a self,
        _text: &'a str,
        _classes: &'a [Intent],
    ) -> Pin<Box<dyn Future<Output = BookkeepingResult<Intent>> + Send + 'a>> {
        Box::pin(async move {
            UnsupportedSnafu {
                provider: self.name(),
                capability: "classify_intent",
            }
            .fail()
        })
    }

    /// Classify the canonical type for an entity mention.
    fn classify_entity_type<'a>(
        &'a self,
        _entity: &'a str,
    ) -> Pin<Box<dyn Future<Output = BookkeepingResult<EntityType>> + Send + 'a>> {
        Box::pin(async move {
            UnsupportedSnafu {
                provider: self.name(),
                capability: "classify_entity_type",
            }
            .fail()
        })
    }

    /// Human-readable name for diagnostics and metrics.
    fn name(&self) -> &'static str;
}
