// WHY: wire DTO
//! Knowledge endpoint request and response wire shapes.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Query parameters for listing facts.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FactsQuery {
    /// Filter by nous agent ID.
    #[serde(default)]
    pub nous_id: Option<String>,
    /// Sort field: confidence, recency, created, `access_count`, `fsrs_review`.
    #[serde(default = "default_sort")]
    pub sort: String,
    /// Sort direction: asc or desc.
    #[serde(default = "default_order")]
    pub order: String,
    /// Text filter.
    #[serde(default)]
    pub filter: Option<String>,
    /// Fact type filter (knowledge, preference, skill, observation, etc.).
    #[serde(default)]
    pub fact_type: Option<String>,
    /// Epistemic tier filter (verified, inferred, assumed).
    #[serde(default)]
    pub tier: Option<String>,
    /// Maximum results to return.
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination.
    #[serde(default)]
    pub offset: usize,
    /// Include forgotten facts.
    #[serde(default)]
    pub include_forgotten: bool,
}

pub(crate) fn default_sort() -> String {
    "confidence".to_string()
}

pub(crate) fn default_order() -> String {
    "desc".to_string()
}

pub(crate) fn default_entity_sort() -> String {
    "page_rank".to_string()
}

pub(crate) fn default_entity_order() -> String {
    "desc".to_string()
}

pub(crate) fn default_limit() -> usize {
    100
}

/// Response wrapper for fact listing.
#[derive(Debug, Serialize)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct FactsResponse {
    pub facts: Vec<mneme::knowledge::Fact>,
    pub total: usize,
}

/// Query parameters for listing entities.
#[derive(Debug, Deserialize, ToSchema)]
pub struct EntitiesQuery {
    /// Maximum results to return (default: 100, max: 1000).
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination.
    #[serde(default)]
    pub offset: usize,
    /// Search text filter.
    #[serde(default)]
    pub q: Option<String>,
    /// Sort field.
    #[serde(default = "default_entity_sort")]
    pub sort: String,
    /// Sort order.
    #[serde(default = "default_entity_order")]
    pub order: String,
    /// Entity type filter.
    #[serde(default)]
    pub entity_type: Vec<String>,
    /// Minimum confidence threshold.
    #[serde(default)]
    pub min_confidence: Option<f64>,
    /// Agent filter.
    #[serde(default)]
    pub agent: Vec<String>,
}

/// Response wrapper for entity listing.
#[derive(Debug, Serialize, ToSchema)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct EntitiesResponse {
    pub entities: Vec<EntityListItem>,
    pub total: usize,
}

/// Entity row returned by the list endpoint.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct EntityListItem {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub confidence: f64,
    pub page_rank: f64,
    pub memory_count: u32,
    pub relationship_count: u32,
}

/// Response wrapper for relationships.
#[derive(Debug, Serialize, ToSchema)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct RelationshipsResponse {
    pub relationships: Vec<EntityRelationship>,
}

/// Direction of a relationship relative to the current entity.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub enum RelationshipDirection {
    /// The relationship points away from the viewed entity.
    Outgoing,
    /// The relationship points toward the viewed entity.
    Incoming,
}

/// Entity relationship row returned by the detail view.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct EntityRelationship {
    pub id: String,
    pub entity_id: String,
    pub entity_name: String,
    pub relationship_type: String,
    pub direction: RelationshipDirection,
    pub confidence: f64,
}

/// Memory record linked to an entity.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct EntityMemory {
    pub id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub confidence: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Request body for entity merge operations.
#[derive(Debug, Deserialize, ToSchema)]
pub struct MergeRequest {
    /// Canonical entity ID to keep.
    #[serde(alias = "primary_id")]
    pub canonical_id: String,
    /// Entity ID to merge and remove.
    #[serde(alias = "secondary_id")]
    pub merged_id: String,
}

/// Entity flagging severity.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum FlagSeverity {
    /// Low-priority review.
    Low,
    /// Medium-priority review.
    Medium,
    /// High-priority review.
    High,
}

/// Request body for entity review flags.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FlagRequest {
    /// Human-readable reason for the flag.
    pub reason: String,
    /// Review severity.
    pub severity: FlagSeverity,
}

/// Body for forget/restore actions.
#[derive(Debug, Deserialize)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct ForgetRequest {
    #[serde(default = "default_forget_reason")]
    pub reason: String,
}

pub(crate) fn default_forget_reason() -> String {
    "user_requested".to_string()
}

/// Body for confidence edit.
#[derive(Debug, Deserialize)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct UpdateConfidenceRequest {
    pub confidence: f64,
}

/// Body for sovereignty sensitivity edit (#3404, #3413).
///
/// Accepted values (lowercase): `public`, `internal`, `confidential`.
#[derive(Debug, Deserialize)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct UpdateSensitivityRequest {
    pub sensitivity: String,
}

/// Search query parameters.
#[derive(Debug, Deserialize, ToSchema)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct SearchQuery {
    pub q: String,
    #[serde(default)]
    pub nous_id: Option<String>,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

fn default_search_limit() -> usize {
    20
}

/// Search result item.
#[derive(Debug, Serialize, ToSchema)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct SearchResult {
    pub id: String,
    pub content: String,
    pub confidence: f64,
    pub tier: String,
    pub fact_type: String,
    pub score: f64,
}

/// Search response wrapper.
#[derive(Debug, Serialize, ToSchema)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
}

/// Explain query parameters.
#[derive(Debug, Deserialize, ToSchema)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct ExplainQuery {
    pub q: String,
    #[serde(default)]
    pub nous_id: Option<String>,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

/// Per-factor score breakdown exposed for debugging.
#[derive(Debug, Serialize, ToSchema)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct FactorScoreBreakdown {
    pub vector_similarity: f64,
    pub decay: f64,
    pub relevance: f64,
    pub epistemic_tier: f64,
    pub access_frequency: f64,
    pub relationship_proximity: f64,
    pub graph_importance: f64,
}

/// Candidate decision reported by the explain endpoint.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExplainDecision {
    /// Included in the returned result set.
    Selected,
    /// Removed because it did not meet a hard gate.
    Dropped,
    /// Removed by a policy filter such as forgetting or visibility.
    Filtered,
}

/// Single candidate in an explain response.
#[derive(Debug, Serialize, ToSchema)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct ExplainCandidate {
    pub id: String,
    pub content: String,
    pub confidence: f64,
    pub tier: String,
    pub fact_type: String,
    pub score: f64,
    pub decision: ExplainDecision,
    pub reasons: Vec<String>,
    pub factors: FactorScoreBreakdown,
}

/// Recall weights reported by the explain endpoint.
#[derive(Debug, Serialize, ToSchema)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct RecallWeightsView {
    pub vector_similarity: f64,
    pub decay: f64,
    pub relevance: f64,
    pub epistemic_tier: f64,
    pub access_frequency: f64,
    pub relationship_proximity: f64,
    pub graph_importance: f64,
}

/// Explain response exposing the candidate set, factor scores, weights, and
/// selection/drop reasons.
#[derive(Debug, Serialize, ToSchema)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct ExplainResponse {
    pub query: String,
    pub weights: RecallWeightsView,
    pub total_candidates: usize,
    pub selected: Vec<ExplainCandidate>,
    pub dropped: Vec<ExplainCandidate>,
}

/// Similar fact entry.
#[derive(Debug, Serialize)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct SimilarFact {
    pub id: String,
    pub content: String,
    pub similarity: f64,
}

/// Fact detail response with related entities and similar facts.
#[derive(Debug, Serialize)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct FactDetailResponse {
    pub fact: mneme::knowledge::Fact,
    pub relationships: Vec<mneme::knowledge::Relationship>,
    pub similar: Vec<SimilarFact>,
}

/// Timeline event.
#[derive(Debug, Clone, Serialize)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct TimelineEvent {
    pub timestamp: String,
    pub event_type: String,
    pub description: String,
    pub fact_id: String,
    pub confidence: Option<f64>,
}

/// Query parameters for timeline listing.
#[derive(Debug, Deserialize)]
pub struct TimelineQuery {
    /// Filter by nous agent ID.
    #[serde(default)]
    pub nous_id: Option<String>,
    /// Maximum events to return (default: 100, max: 1000).
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination.
    #[serde(default)]
    pub offset: usize,
}

/// Timeline response.
#[derive(Debug, Serialize)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct TimelineResponse {
    pub events: Vec<TimelineEvent>,
    pub total: usize,
}

/// Response type for graph health check.
#[derive(Debug, serde::Serialize)]
pub struct GraphCheckReport {
    /// Total number of facts stored.
    pub fact_count: usize,
    /// Total number of entities stored.
    pub entity_count: usize,
    /// Total number of relationships stored.
    pub relationship_count: usize,
    /// Entities with no facts or relationships (potential orphans).
    pub orphaned_entity_count: usize,
    /// Edges that reference missing endpoint entities.
    pub dangling_edge_count: usize,
    /// Overall health: `"healthy"` or `"issues_found"`.
    pub status: &'static str,
}
