// WHY: typed memory/knowledge DTOs shared between pylon and first-party UIs.
//! Request and response types for the `/api/v1/knowledge` memory surface.

use serde::{Deserialize, Serialize};

/// Query parameters for `GET /api/v1/knowledge/facts`.
#[derive(Debug, Clone)]
pub struct FactsQuery {
    /// Filter by owning nous agent ID.
    pub nous_id: Option<String>,
    /// Sort field: `confidence`, `recency`, `created`, `access_count`, `fsrs_review`.
    pub sort: String,
    /// Sort direction: `asc` or `desc`.
    pub order: String,
    /// Text filter applied to fact content.
    pub filter: Option<String>,
    /// Fact type filter.
    pub fact_type: Option<String>,
    /// Epistemic tier filter.
    pub tier: Option<String>,
    /// Maximum facts to return.
    pub limit: u32,
    /// Pagination offset.
    pub offset: u32,
    /// Include forgotten facts.
    pub include_forgotten: bool,
}

impl Default for FactsQuery {
    fn default() -> Self {
        Self {
            nous_id: None,
            sort: "confidence".to_string(),
            order: "desc".to_string(),
            filter: None,
            fact_type: None,
            tier: None,
            limit: 100,
            offset: 0,
            include_forgotten: false,
        }
    }
}

/// Query parameters for `GET /api/v1/knowledge/entities`.
#[derive(Debug, Clone)]
pub struct EntitiesQuery {
    /// Maximum entities to return.
    pub limit: u32,
    /// Pagination offset.
    pub offset: u32,
    /// Search text filter.
    pub q: Option<String>,
    /// Sort field.
    pub sort: String,
    /// Sort direction.
    pub order: String,
    /// Entity type filters.
    pub entity_type: Vec<String>,
    /// Minimum confidence threshold.
    pub min_confidence: Option<f64>,
    /// Agent filters.
    pub agent: Vec<String>,
}

impl Default for EntitiesQuery {
    fn default() -> Self {
        Self {
            limit: 100,
            offset: 0,
            q: None,
            sort: "page_rank".to_string(),
            order: "desc".to_string(),
            entity_type: Vec::new(),
            min_confidence: None,
            agent: Vec::new(),
        }
    }
}

/// Query parameters for `GET /api/v1/knowledge/timeline`.
#[derive(Debug, Clone)]
pub struct TimelineQuery {
    /// Filter by owning nous agent ID.
    pub nous_id: Option<String>,
    /// Maximum events to return.
    pub limit: u32,
    /// Pagination offset.
    pub offset: u32,
}

impl Default for TimelineQuery {
    fn default() -> Self {
        Self {
            nous_id: None,
            limit: 100,
            offset: 0,
        }
    }
}

/// Direction of an entity relationship relative to the viewed entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelationshipDirection {
    /// The relationship points away from the viewed entity.
    Outgoing,
    /// The relationship points toward the viewed entity.
    Incoming,
}

/// Severity for an entity review flag.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FlagSeverity {
    /// Low-priority review.
    Low,
    /// Medium-priority review.
    Medium,
    /// High-priority review.
    High,
}

/// Request body for `PUT /api/v1/knowledge/facts/{id}/confidence`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfidenceRequest {
    /// New confidence value in `[0.0, 1.0]`.
    pub confidence: f64,
}

/// Request body for `PUT /api/v1/knowledge/facts/{id}/sensitivity`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSensitivityRequest {
    /// New sensitivity: `public`, `internal`, or `confidential`.
    pub sensitivity: String,
}

/// Request body for `POST /api/v1/knowledge/facts/{id}/forget`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgetRequest {
    /// Reason the fact is being forgotten.
    #[serde(default = "default_forget_reason")]
    pub reason: String,
}

fn default_forget_reason() -> String {
    "user_requested".to_string()
}

/// Request body for `POST /api/v1/knowledge/entities/{id}/flag`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagRequest {
    /// Human-readable reason for the flag.
    pub reason: String,
    /// Review severity.
    pub severity: FlagSeverity,
}

/// Request body for `POST /api/v1/knowledge/entities/merge`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeRequest {
    /// Canonical entity ID to keep.
    #[serde(alias = "primary_id")]
    pub canonical_id: String,
    /// Entity ID to merge and remove.
    #[serde(alias = "secondary_id")]
    pub merged_id: String,
}

/// A knowledge fact as returned by `/api/v1/knowledge/facts`.
///
/// Mirrors the flattened wire shape produced by `mneme::knowledge::Fact`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Fact {
    /// Stable fact identifier.
    pub id: String,
    /// Agent that owns this fact.
    #[serde(default)]
    pub nous_id: String,
    /// Classification such as `preference`, `skill`, or `observation`.
    #[serde(default)]
    pub fact_type: String,
    /// Human-readable fact statement.
    #[serde(default)]
    pub content: String,
    /// Team-memory scope, if the server provided one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Project partition, if the server provided one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Data-sovereignty classification.
    #[serde(default)]
    pub sensitivity: String,
    /// Sharing visibility.
    #[serde(default)]
    pub visibility: String,
    /// When this fact became valid in the domain.
    #[serde(default)]
    pub valid_from: String,
    /// When this fact ceases to be valid in the domain.
    #[serde(default)]
    pub valid_to: String,
    /// System recording time.
    #[serde(default)]
    pub recorded_at: String,
    /// Confidence in `[0.0, 1.0]`.
    #[serde(default)]
    pub confidence: f64,
    /// Epistemic tier such as `verified`, `inferred`, or `assumed`.
    #[serde(default)]
    pub tier: String,
    /// Session that produced this fact, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_session_id: Option<String>,
    /// Base FSRS stability in hours.
    #[serde(default)]
    pub stability_hours: f64,
    /// ID of the fact that replaced this one, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    /// Whether the fact has been forgotten.
    #[serde(default)]
    pub is_forgotten: bool,
    /// When the fact was forgotten, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forgotten_at: Option<String>,
    /// Why the fact was forgotten, if applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forget_reason: Option<String>,
    /// Number of recalls.
    #[serde(default)]
    pub access_count: u32,
    /// Timestamp of the most recent recall, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_accessed_at: Option<String>,
}

/// Response wrapper for the fact listing endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactsResponse {
    /// Matching facts.
    #[serde(default)]
    pub facts: Vec<Fact>,
    /// Total number of matching facts (before pagination).
    #[serde(default)]
    pub total: usize,
}

/// A similar fact result inside a fact detail response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarFact {
    /// Fact ID.
    pub id: String,
    /// Fact content.
    pub content: String,
    /// Similarity score.
    pub similarity: f64,
}

/// A relationship between two entities as stored in the graph.
///
/// Used inside `FactDetailResponse`; contrast with [`EntityRelationship`],
/// which is relative to a single viewed entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    /// Source entity ID.
    pub src: String,
    /// Target entity ID.
    pub dst: String,
    /// Relationship type label.
    pub relation: String,
    /// Relationship weight/strength.
    #[serde(default)]
    pub weight: f64,
    /// When first observed.
    #[serde(default)]
    pub created_at: String,
}

/// Response wrapper for a single fact's detail view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactDetailResponse {
    /// The requested fact.
    pub fact: Fact,
    /// Graph relationships involving this fact.
    #[serde(default)]
    pub relationships: Vec<Relationship>,
    /// Similar facts.
    #[serde(default)]
    pub similar: Vec<SimilarFact>,
}

/// An entity row returned by the list endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityListItem {
    /// Entity identifier.
    pub id: String,
    /// Primary display name.
    pub name: String,
    /// Entity classification.
    #[serde(default)]
    pub entity_type: String,
    /// Known aliases.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// Creation timestamp.
    #[serde(default)]
    pub created_at: String,
    /// Last-updated timestamp.
    #[serde(default)]
    pub updated_at: String,
    /// Mean confidence of associated facts.
    #[serde(default)]
    pub confidence: f64,
    /// PageRank importance score.
    #[serde(default)]
    pub page_rank: f64,
    /// Number of associated memories.
    #[serde(default)]
    pub memory_count: u32,
    /// Number of relationships.
    #[serde(default)]
    pub relationship_count: u32,
}

/// An entity as returned by `GET /api/v1/knowledge/entities/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Entity identifier.
    pub id: String,
    /// Primary display name.
    pub name: String,
    /// Entity classification.
    #[serde(default)]
    pub entity_type: String,
    /// Known aliases.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Creation timestamp.
    #[serde(default)]
    pub created_at: String,
    /// Last-updated timestamp.
    #[serde(default)]
    pub updated_at: String,
}

/// Response wrapper for the entity list endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitiesResponse {
    /// Matching entities.
    #[serde(default)]
    pub entities: Vec<EntityListItem>,
    /// Total number of matching entities (before pagination).
    #[serde(default)]
    pub total: usize,
}

/// A relationship relative to a single viewed entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRelationship {
    /// Relationship identifier.
    pub id: String,
    /// Related entity ID.
    pub entity_id: String,
    /// Related entity name.
    pub entity_name: String,
    /// Relationship type label.
    pub relationship_type: String,
    /// Direction relative to the viewed entity.
    pub direction: RelationshipDirection,
    /// Confidence in the relationship.
    #[serde(default)]
    pub confidence: f64,
}

/// Response wrapper for the entity relationships endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipsResponse {
    /// Relationships of the viewed entity.
    #[serde(default)]
    pub relationships: Vec<EntityRelationship>,
}

/// A memory record linked to an entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityMemory {
    /// Memory (fact) ID.
    pub id: String,
    /// Memory content.
    pub content: String,
    /// Source agent, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Source session, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// Confidence score.
    #[serde(default)]
    pub confidence: f64,
    /// Creation/recording timestamp, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// A single event in the knowledge activity timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineEvent {
    /// Event timestamp.
    pub timestamp: String,
    /// Event type such as `created` or `accessed`.
    pub event_type: String,
    /// Human-readable description.
    pub description: String,
    /// Associated fact ID.
    pub fact_id: String,
    /// Confidence at the time of the event, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

/// Response wrapper for the knowledge activity timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineResponse {
    /// Timeline events.
    #[serde(default)]
    pub events: Vec<TimelineEvent>,
    /// Total number of events (before pagination).
    #[serde(default)]
    pub total: usize,
}

/// Graph health report returned by `GET /api/v1/knowledge/check`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphCheckReport {
    /// Total number of facts stored.
    #[serde(default)]
    pub fact_count: usize,
    /// Total number of entities stored.
    #[serde(default)]
    pub entity_count: usize,
    /// Total number of relationships stored.
    #[serde(default)]
    pub relationship_count: usize,
    /// Entities with no facts or relationships.
    #[serde(default)]
    pub orphaned_entity_count: usize,
    /// Edges that reference missing endpoint entities.
    #[serde(default)]
    pub dangling_edge_count: usize,
    /// Overall status: `healthy` or `issues_found`.
    #[serde(default)]
    pub status: String,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    #[test]
    fn fact_parses_flattened_mneme_shape() {
        let json = r#"{
            "id": "fact_01",
            "nous_id": "agent-1",
            "content": "The operator prefers tabs over spaces",
            "fact_type": "preference",
            "tier": "verified",
            "confidence": 0.92,
            "sensitivity": "internal",
            "visibility": "private",
            "scope": "user",
            "project_id": "acme.corp/website",
            "valid_from": "2026-06-01T00:00:00Z",
            "valid_to": "9999-01-01T00:00:00Z",
            "recorded_at": "2026-06-01T12:00:00Z",
            "source_session_id": "session-7",
            "stability_hours": 8760.0,
            "access_count": 4,
            "last_accessed_at": "2026-06-10T08:00:00Z",
            "is_forgotten": true,
            "forgotten_at": "2026-06-11T00:00:00Z",
            "forget_reason": "outdated",
            "superseded_by": "fact_03"
        }"#;
        let fact: Fact = serde_json::from_str(json).unwrap();
        assert_eq!(fact.id, "fact_01");
        assert_eq!(fact.nous_id, "agent-1");
        assert_eq!(fact.fact_type, "preference");
        assert_eq!(fact.tier, "verified");
        assert_eq!(fact.sensitivity, "internal");
        assert_eq!(fact.visibility, "private");
        assert_eq!(fact.scope.as_deref(), Some("user"));
        assert_eq!(fact.project_id.as_deref(), Some("acme.corp/website"));
        assert!((fact.confidence - 0.92).abs() < f64::EPSILON);
        assert!((fact.stability_hours - 8760.0).abs() < f64::EPSILON);
        assert_eq!(fact.access_count, 4);
        assert!(fact.is_forgotten);
        assert_eq!(fact.superseded_by.as_deref(), Some("fact_03"));
    }

    #[test]
    fn fact_tolerates_minimal_shape() {
        let fact: Fact = serde_json::from_str(r#"{"id":"f1","content":"c"}"#).unwrap();
        assert_eq!(fact.id, "f1");
        assert_eq!(fact.content, "c");
        assert!(!fact.is_forgotten);
        assert_eq!(fact.access_count, 0);
    }

    #[test]
    fn facts_response_parses_envelope() {
        let json = r#"{"facts":[{"id":"f1","content":"c"}],"total":1}"#;
        let resp: FactsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.total, 1);
        assert_eq!(resp.facts.len(), 1);
    }

    #[test]
    fn entity_relationship_preserves_direction_and_identifier() {
        let json = r#"{
            "id": "e1:e2:depends_on:2026-01-01T00:00:00Z",
            "entity_id": "e2",
            "entity_name": "Beta",
            "relationship_type": "depends_on",
            "direction": "Outgoing",
            "confidence": 0.85
        }"#;
        let rel: EntityRelationship = serde_json::from_str(json).unwrap();
        assert_eq!(rel.id, "e1:e2:depends_on:2026-01-01T00:00:00Z");
        assert_eq!(rel.entity_id, "e2");
        assert_eq!(rel.direction, RelationshipDirection::Outgoing);
        assert!((rel.confidence - 0.85).abs() < f64::EPSILON);
    }

    #[test]
    fn entity_list_item_preserves_pagination_fields() {
        let json = r#"{
            "id": "e1",
            "name": "Alpha",
            "entity_type": "concept",
            "aliases": ["a"],
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-02-01T00:00:00Z",
            "confidence": 0.9,
            "page_rank": 0.12,
            "memory_count": 3,
            "relationship_count": 2
        }"#;
        let item: EntityListItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.name, "Alpha");
        assert!((item.page_rank - 0.12).abs() < f64::EPSILON);
        assert_eq!(item.memory_count, 3);
        assert_eq!(item.relationship_count, 2);
    }

    #[test]
    fn timeline_response_parses_pagination() {
        let json = r#"{"events":[{"timestamp":"2026-01-01T00:00:00Z","event_type":"created","description":"d","fact_id":"f1","confidence":0.9}],"total":1}"#;
        let resp: TimelineResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.total, 1);
        assert_eq!(resp.events[0].fact_id, "f1");
    }

    #[test]
    fn graph_check_report_parses() {
        let json = r#"{
            "fact_count": 10,
            "entity_count": 5,
            "relationship_count": 8,
            "orphaned_entity_count": 1,
            "dangling_edge_count": 0,
            "status": "issues_found"
        }"#;
        let report: GraphCheckReport = serde_json::from_str(json).unwrap();
        assert_eq!(report.fact_count, 10);
        assert_eq!(report.orphaned_entity_count, 1);
        assert_eq!(report.status, "issues_found");
    }
}
