//! Typed knowledge/memory DTOs mirroring Pylon's wire shapes
//! (`crates/pylon/src/handlers/knowledge/dto.rs`). Skene has no dependency on
//! pylon or mneme, so these are independent structs kept in sync by the
//! contract tests in `super::tests`.

use serde::{Deserialize, Serialize};

/// Data-sovereignty classification for a fact. Mirrors
/// `mneme::knowledge::FactSensitivity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FactSensitivity {
    /// Safe for any provider, including cloud LLM providers.
    #[default]
    Public,
    /// Safe for local or self-hosted providers only.
    Internal,
    /// Embedded (in-process) providers only.
    Confidential,
}

/// Visibility level for a fact. Mirrors `mneme::knowledge::Visibility`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FactVisibility {
    /// Visible only to the originating agent / user.
    #[default]
    Private,
    /// Visible to agents within the same team or project scope.
    Shared,
    /// Visible to a defined allow-list of consumers.
    Restricted,
    /// Visible to any authorized consumer, including external integrations.
    Published,
}

/// Epistemic confidence tier. Mirrors `mneme::knowledge::EpistemicTier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EpistemicTier {
    /// Checked against ground truth.
    Verified,
    /// Produced by self-reflection or meta-cognitive review.
    Reflected,
    /// Reasoned from context.
    Inferred,
    /// Unchecked assumption.
    Assumed,
    /// Derived from agent session outcomes for training signal.
    Training,
}

impl EpistemicTier {
    /// Return the lowercase string representation of this tier.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Reflected => "reflected",
            Self::Inferred => "inferred",
            Self::Assumed => "assumed",
            Self::Training => "training",
        }
    }
}

/// A memory fact, deserialized from Pylon's flattened `Fact` wire shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's flattened Fact wire shape; self-documenting by name"
)]
pub struct Fact {
    pub id: String,
    pub nous_id: String,
    pub fact_type: String,
    pub content: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub sensitivity: FactSensitivity,
    #[serde(default)]
    pub visibility: FactVisibility,
    pub valid_from: String,
    pub valid_to: String,
    pub recorded_at: String,
    pub confidence: f64,
    pub tier: EpistemicTier,
    #[serde(default)]
    pub source_session_id: Option<String>,
    pub stability_hours: f64,
    #[serde(default)]
    pub superseded_by: Option<String>,
    #[serde(default)]
    pub is_forgotten: bool,
    #[serde(default)]
    pub forgotten_at: Option<String>,
    #[serde(default)]
    pub forget_reason: Option<String>,
    #[serde(default)]
    pub access_count: u32,
    #[serde(default)]
    pub last_accessed_at: Option<String>,
}

/// Response for `GET /api/v1/knowledge/facts`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactsResponse {
    /// Facts matching the query.
    pub facts: Vec<Fact>,
    /// Total matching facts (may exceed `facts.len()` under pagination).
    pub total: usize,
}

/// A directed edge between two entities in the knowledge graph. Mirrors
/// `eidos::knowledge::Relationship` — this is the shape returned by the
/// fact-detail endpoint, distinct from [`EntityRelationship`] (the
/// entity-relative shape returned by the per-entity relationships endpoint).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror eidos::knowledge::Relationship; self-documenting by name"
)]
pub struct Relationship {
    pub src: String,
    pub dst: String,
    pub relation: String,
    #[serde(default)]
    pub weight: f64,
    #[serde(default)]
    pub created_at: String,
}

/// A fact deemed similar to the requested fact by embedding search.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's SimilarFact; self-documenting by name"
)]
pub struct SimilarFact {
    pub id: String,
    pub content: String,
    pub similarity: f64,
}

/// Response for `GET /api/v1/knowledge/facts/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactDetailResponse {
    /// The requested fact.
    pub fact: Fact,
    /// Graph relationships touching this fact's subject entities.
    #[serde(default)]
    pub relationships: Vec<Relationship>,
    /// Facts deemed similar by embedding search.
    #[serde(default)]
    pub similar: Vec<SimilarFact>,
}

/// Entity row returned by the list endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's EntityListItem; self-documenting by name"
)]
pub struct EntityListItem {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub confidence: f64,
    pub page_rank: f64,
    pub memory_count: u32,
    pub relationship_count: u32,
}

/// Response for `GET /api/v1/knowledge/entities`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitiesResponse {
    /// Entities matching the query.
    pub entities: Vec<EntityListItem>,
    /// Total matching entities.
    pub total: usize,
}

/// Direction of a relationship relative to the entity it was fetched for.
/// Mirrors `pylon::handlers::knowledge::dto::RelationshipDirection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelationshipDirection {
    /// The relationship points away from the viewed entity.
    Outgoing,
    /// The relationship points toward the viewed entity.
    Incoming,
}

/// Entity relationship row returned by the detail view. NOTE: this is
/// relative to whichever entity the request was made for (`entity_id` on the
/// request), not a free-standing graph edge — the wire shape carries the
/// *other* side of the edge (`entity_id`/`entity_name`) plus a `direction`
/// telling the caller which side of `relationship_type` the viewed entity is
/// on. Reconstructing a global (src, dst) edge requires the viewed entity id,
/// which the caller already knows (see `koilon::update::memory::data_loading`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's EntityRelationship; self-documenting by name"
)]
pub struct EntityRelationship {
    pub id: String,
    pub entity_id: String,
    pub entity_name: String,
    pub relationship_type: String,
    pub direction: RelationshipDirection,
    pub confidence: f64,
}

/// Response for `GET /api/v1/knowledge/entities/{id}/relationships`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipsResponse {
    /// Relationships for the requested entity.
    pub relationships: Vec<EntityRelationship>,
}

/// A memory record linked to an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's EntityMemory; self-documenting by name"
)]
pub struct EntityMemory {
    pub id: String,
    pub content: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
    pub confidence: f64,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// A single knowledge timeline event.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's TimelineEvent; self-documenting by name"
)]
pub struct TimelineEvent {
    pub timestamp: String,
    pub event_type: String,
    pub description: String,
    pub fact_id: String,
    #[serde(default)]
    pub confidence: Option<f64>,
}

/// Response for `GET /api/v1/knowledge/timeline`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineResponse {
    /// Timeline events, oldest first within the requested page.
    #[serde(default)]
    pub events: Vec<TimelineEvent>,
    /// Total matching events (may exceed `events.len()` under pagination).
    #[serde(default)]
    pub total: usize,
}
