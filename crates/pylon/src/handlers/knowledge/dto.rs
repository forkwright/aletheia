// WHY: wire DTO
//! Knowledge endpoint request and response wire shapes.

use serde::{Deserialize, Serialize};

/// Query parameters for listing facts.
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Deserialize)]
pub struct EntitiesQuery {
    /// Maximum results to return (default: 100, max: 1000).
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Offset for pagination.
    #[serde(default)]
    pub offset: usize,
}

/// Response wrapper for entity listing.
#[derive(Debug, Serialize)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct EntitiesResponse {
    pub entities: Vec<mneme::knowledge::Entity>,
    pub total: usize,
}

/// Response wrapper for relationships.
#[derive(Debug, Serialize)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct RelationshipsResponse {
    pub relationships: Vec<mneme::knowledge::Relationship>,
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
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
#[expect(
    missing_docs,
    reason = "response struct fields are self-documenting by name"
)]
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
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
