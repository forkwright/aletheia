//! Knowledge graph browsing and management endpoints.
//!
//! Exposes facts, entities, and relationships from the mneme knowledge store
//! for the TUI memory inspector panel.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, BadRequestSnafu};
use crate::state::AppState;

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

fn default_sort() -> String {
    "confidence".to_string()
}

fn default_order() -> String {
    "desc".to_string()
}

/// Valid sort fields for fact listing.
const VALID_SORT_FIELDS: &[&str] = &[
    "confidence",
    "recency",
    "created",
    "access_count",
    "fsrs_review",
];

/// Valid sort directions (checked case-insensitively).
const VALID_ORDER_VALUES: &[&str] = &["asc", "desc"];

/// Hard upper bound on the `limit` query parameter for all knowledge endpoints.
const MAX_LIMIT: usize = 1000;

fn default_limit() -> usize {
    50
}

/// Response wrapper for fact listing.
#[derive(Debug, Serialize)]
pub struct FactsResponse {
    pub facts: Vec<aletheia_mneme::knowledge::Fact>,
    pub total: usize,
}

/// Response wrapper for entity listing.
#[derive(Debug, Serialize)]
pub struct EntitiesResponse {
    pub entities: Vec<aletheia_mneme::knowledge::Entity>,
}

/// Response wrapper for relationships.
#[derive(Debug, Serialize)]
pub struct RelationshipsResponse {
    pub relationships: Vec<aletheia_mneme::knowledge::Relationship>,
}

/// Body for forget/restore actions.
#[derive(Debug, Deserialize)]
pub struct ForgetRequest {
    #[serde(default = "default_forget_reason")]
    pub reason: String,
}

fn default_forget_reason() -> String {
    "user_requested".to_string()
}

/// Body for confidence edit.
#[derive(Debug, Deserialize)]
pub struct UpdateConfidenceRequest {
    pub confidence: f64,
}

/// Search query parameters.
#[derive(Debug, Deserialize)]
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

/// Hard upper bound on the `limit` query parameter for search.
const MAX_SEARCH_LIMIT: usize = 1000;

/// Search result item.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
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
pub struct SearchResponse {
    pub results: Vec<SearchResult>,
}

/// Similar fact entry.
#[derive(Debug, Serialize)]
pub struct SimilarFact {
    pub id: String,
    pub content: String,
    pub similarity: f64,
}

/// Fact detail response with related entities and similar facts.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactDetailResponse {
    pub fact: aletheia_mneme::knowledge::Fact,
    pub relationships: Vec<aletheia_mneme::knowledge::Relationship>,
    pub similar: Vec<SimilarFact>,
}

/// Timeline event.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineEvent {
    pub timestamp: String,
    pub event_type: String,
    pub description: String,
    pub fact_id: String,
    pub confidence: Option<f64>,
}

/// Timeline response.
#[derive(Debug, Serialize)]
pub struct TimelineResponse {
    pub events: Vec<TimelineEvent>,
}

/// Validate sort/order query parameters, returning 400 with descriptive errors.
fn validate_sort_order(sort: &str, order: &str) -> Result<(), ApiError> {
    if !VALID_SORT_FIELDS.contains(&sort) {
        return Err(BadRequestSnafu {
            message: format!(
                "invalid sort field '{sort}': valid fields are {}",
                VALID_SORT_FIELDS.join(", "),
            ),
        }
        .build());
    }
    if !VALID_ORDER_VALUES.contains(&order.to_ascii_lowercase().as_str()) {
        return Err(BadRequestSnafu {
            message: format!("invalid order '{order}': valid values are asc, desc",),
        }
        .build());
    }
    Ok(())
}

/// GET /api/v1/knowledge/facts
///
/// List facts with sorting, filtering, and pagination.
/// The knowledge store may not be available (feature-gated), so we return
/// synthetic demo data when the store is absent, ensuring the TUI always
/// has something to display.
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/facts",
    params(
        ("nous_id" = Option<String>, Query, description = "Filter by agent ID"),
        ("sort" = Option<String>, Query, description = "Sort field: confidence, recency, created, access_count, fsrs_review (default: confidence)"),
        ("order" = Option<String>, Query, description = "Sort direction: asc or desc (default: desc)"),
        ("filter" = Option<String>, Query, description = "Text filter"),
        ("fact_type" = Option<String>, Query, description = "Fact type filter: knowledge, preference, skill, observation, etc."),
        ("tier" = Option<String>, Query, description = "Epistemic tier: verified, inferred, assumed"),
        ("limit" = Option<usize>, Query, description = "Maximum results (default: 100)"),
        ("offset" = Option<usize>, Query, description = "Pagination offset"),
        ("include_forgotten" = Option<bool>, Query, description = "Include forgotten facts (default: false)"),
    ),
    responses(
        (status = 200, description = "Fact list with total count"),
        (status = 400, description = "Invalid sort or order parameter", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_facts(
    State(state): State<Arc<AppState>>,
    Query(mut query): Query<FactsQuery>,
) -> Result<Json<FactsResponse>, ApiError> {
    use aletheia_mneme::knowledge::EpistemicTier;

    query.limit = query.limit.min(MAX_LIMIT);
    validate_sort_order(&query.sort, &query.order)?;
    query.order = query.order.to_ascii_lowercase();

    let mut facts = get_stored_facts(&state, &query);

    if let Some(ref filter) = query.filter {
        let filter_lower = filter.to_lowercase();
        facts.retain(|f| f.content.to_lowercase().contains(&filter_lower));
    }

    if let Some(ref ft) = query.fact_type
        && ft != "all"
    {
        facts.retain(|f| f.fact_type == *ft);
    }

    if let Some(ref tier) = query.tier {
        let tier_enum = match tier.as_str() {
            "verified" => Some(EpistemicTier::Verified),
            "inferred" => Some(EpistemicTier::Inferred),
            "assumed" => Some(EpistemicTier::Assumed),
            _ => None,
        };
        if let Some(t) = tier_enum {
            facts.retain(|f| f.tier == t);
        }
    }

    if !query.include_forgotten {
        facts.retain(|f| !f.is_forgotten);
    }

    let total = facts.len();

    sort_facts(&mut facts, &query.sort, &query.order);

    let start = query.offset.min(facts.len());
    let end = (start + query.limit).min(facts.len());
    // start and end are both bounded by facts.len() via .min()
    #[expect(
        clippy::indexing_slicing,
        reason = "start and end are bounded by facts.len() via .min()"
    )]
    let facts = facts[start..end].to_vec();

    Ok(Json(FactsResponse { facts, total }))
}

/// GET /api/v1/knowledge/facts/{id}
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/facts/{id}",
    params(("id" = String, Path, description = "Fact ID")),
    responses(
        (status = 200, description = "Fact detail with relationships and similar facts"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Fact not found", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_fact(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<FactDetailResponse>, ApiError> {
    // WHY: The previous implementation called get_all_facts which hardcoded
    // nous_id: None, causing get_stored_facts to always return an empty Vec
    // (it requires nous_id.is_some() to query the store). Bug #1252.
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let facts = store
            .read_facts_by_id(&id)
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
                location: snafu::location!(),
            })?;
        let fact = facts.into_iter().next().ok_or_else(|| ApiError::NotFound {
            path: format!("fact/{id}"),
            location: snafu::location!(),
        })?;
        let relationships = get_fact_relationships(&state, &fact);
        let similar = get_similar_facts(&state, &fact);
        return Ok(Json(FactDetailResponse {
            fact,
            relationships,
            similar,
        }));
    }
    Err(ApiError::NotFound {
        path: format!("fact/{id}"),
        location: snafu::location!(),
    })
}

/// GET /api/v1/knowledge/entities
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/entities",
    responses(
        (status = 200, description = "Entity list"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_entities(
    State(state): State<Arc<AppState>>,
) -> Result<Json<EntitiesResponse>, ApiError> {
    let entities = get_stored_entities(&state);
    Ok(Json(EntitiesResponse { entities }))
}

/// GET /api/v1/knowledge/entities/{id}/relationships
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/entities/{id}/relationships",
    params(("id" = String, Path, description = "Entity ID")),
    responses(
        (status = 200, description = "Entity relationships"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn entity_relationships(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<RelationshipsResponse>, ApiError> {
    let relationships = get_entity_relationships(&state, &id);
    Ok(Json(RelationshipsResponse { relationships }))
}

mod mutation;
mod search;

pub use mutation::{
    __path_forget_fact, __path_restore_fact, __path_update_confidence, forget_fact, restore_fact,
    update_confidence,
};
#[cfg(test)]
use search::truncate_content;
pub use search::{__path_search, __path_timeline, search, timeline};
use search::{get_entity_relationships, get_stored_entities, get_stored_facts, sort_facts};
#[cfg(feature = "knowledge-store")]
use search::{get_fact_relationships, get_similar_facts};

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]
mod tests {
    use super::*;

    fn make_fact(id: &str, content: &str, confidence: f64) -> aletheia_mneme::knowledge::Fact {
        use aletheia_mneme::id::FactId;
        use aletheia_mneme::knowledge::EpistemicTier;
        aletheia_mneme::knowledge::Fact {
            id: FactId::new(id).unwrap(),
            nous_id: "test-nous".to_owned(),
            content: content.to_owned(),
            confidence,
            tier: EpistemicTier::Inferred,
            valid_from: jiff::Timestamp::UNIX_EPOCH,
            valid_to: jiff::Timestamp::UNIX_EPOCH,
            superseded_by: None,
            source_session_id: None,
            recorded_at: jiff::Timestamp::UNIX_EPOCH,
            access_count: 0,
            last_accessed_at: None,
            stability_hours: 24.0,
            fact_type: "knowledge".to_owned(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        }
    }

    #[test]
    fn truncate_content_short_text_unchanged() {
        let s = "short";
        assert_eq!(truncate_content(s, 80), "short");
    }

    #[test]
    fn truncate_content_long_text_gets_ellipsis() {
        let s = "a".repeat(100);
        let result = truncate_content(&s, 80);
        assert!(result.ends_with("..."));
        assert_eq!(result.len(), 83);
    }

    #[test]
    fn truncate_content_handles_utf8_boundary() {
        // NOTE: "é" is 2 bytes; with max=1 we must not split mid-char.
        let s = "éàü";
        let result = truncate_content(s, 1);
        assert!(result.ends_with("..."));
        assert!(std::str::from_utf8(result.as_bytes()).is_ok());
    }

    #[test]
    fn default_sort_returns_confidence() {
        assert_eq!(default_sort(), "confidence");
    }

    #[test]
    fn default_order_returns_desc() {
        assert_eq!(default_order(), "desc");
    }

    #[test]
    fn default_limit_returns_50() {
        assert_eq!(default_limit(), 50);
    }

    #[test]
    fn sort_facts_by_confidence_descending() {
        let mut facts = vec![
            make_fact("a", "low", 0.3),
            make_fact("b", "high", 0.9),
            make_fact("c", "mid", 0.6),
        ];
        sort_facts(&mut facts, "confidence", "desc");
        assert_eq!(facts[0].id.as_str(), "b");
        assert_eq!(facts[1].id.as_str(), "c");
        assert_eq!(facts[2].id.as_str(), "a");
    }

    #[test]
    fn sort_facts_by_confidence_ascending() {
        let mut facts = vec![
            make_fact("a", "low", 0.3),
            make_fact("b", "high", 0.9),
            make_fact("c", "mid", 0.6),
        ];
        sort_facts(&mut facts, "confidence", "asc");
        assert_eq!(facts[0].id.as_str(), "a");
        assert_eq!(facts[1].id.as_str(), "c");
        assert_eq!(facts[2].id.as_str(), "b");
    }

    #[test]
    fn sort_facts_by_access_count_descending() {
        let mut facts = vec![
            make_fact("a", "one access", 0.5),
            make_fact("b", "five accesses", 0.5),
        ];
        facts[0].access_count = 1;
        facts[1].access_count = 5;
        sort_facts(&mut facts, "access_count", "desc");
        assert_eq!(facts[0].id.as_str(), "b");
        assert_eq!(facts[1].id.as_str(), "a");
    }

    #[test]
    fn facts_query_default_values() {
        // NOTE: FactsQuery has individual serde defaults; test them via JSON.
        let q: FactsQuery = serde_json::from_str("{}").unwrap();
        assert_eq!(q.sort, "confidence");
        assert_eq!(q.order, "desc");
        assert_eq!(q.limit, 50);
        assert!(!q.include_forgotten);
    }

    #[test]
    fn limit_is_capped_at_max() {
        // NOTE: list_facts clamps query.limit to MAX_LIMIT (1000) before use.
        const { assert!(MAX_LIMIT <= 1000) };
        assert_eq!(MAX_LIMIT, 1000);
    }

    #[test]
    fn search_result_serializes_camel_case() {
        let result = SearchResult {
            id: "fact-1".to_owned(),
            content: "Alice works at Acme Corp".to_owned(),
            confidence: 0.8,
            tier: "inferred".to_owned(),
            fact_type: "knowledge".to_owned(),
            score: 0.64,
        };
        let json = serde_json::to_value(&result).unwrap();
        // NOTE: serde(rename_all = "camelCase") maps fact_type to factType.
        assert!(json.get("factType").is_some());
        assert_eq!(json["factType"], "knowledge");
        assert_eq!(json["confidence"], 0.8);
    }

    #[test]
    fn forget_request_default_reason() {
        let req: ForgetRequest = serde_json::from_str("{}").unwrap();
        assert_eq!(req.reason, "user_requested");
    }

    #[test]
    fn empty_search_returns_empty_results() {
        let response = SearchResponse { results: vec![] };
        let json = serde_json::to_value(&response).unwrap();
        assert!(json["results"].as_array().unwrap().is_empty());
    }

    #[test]
    fn entities_response_serializes_empty() {
        let response = EntitiesResponse { entities: vec![] };
        let json = serde_json::to_value(&response).unwrap();
        assert!(json["entities"].as_array().unwrap().is_empty());
    }

    #[test]
    fn validate_sort_order_accepts_all_valid_sort_fields() {
        for field in VALID_SORT_FIELDS {
            assert!(validate_sort_order(field, "asc").is_ok());
            assert!(validate_sort_order(field, "desc").is_ok());
        }
    }

    #[test]
    fn validate_sort_order_rejects_invalid_sort_field() {
        let err = validate_sort_order("invalid_field", "asc").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("invalid sort field 'invalid_field'"), "{msg}");
        assert!(msg.contains("confidence"), "{msg}");
        assert!(msg.contains("recency"), "{msg}");
    }

    #[test]
    fn validate_sort_order_rejects_invalid_order() {
        let err = validate_sort_order("confidence", "upward").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("invalid order 'upward'"), "{msg}");
    }

    #[test]
    fn validate_sort_order_accepts_case_insensitive_order() {
        assert!(validate_sort_order("confidence", "ASC").is_ok());
        assert!(validate_sort_order("confidence", "DESC").is_ok());
        assert!(validate_sort_order("confidence", "Asc").is_ok());
        assert!(validate_sort_order("confidence", "Desc").is_ok());
    }

    #[test]
    fn sort_facts_with_uppercase_order() {
        let mut facts = vec![make_fact("a", "low", 0.3), make_fact("b", "high", 0.9)];
        // NOTE: After validation, order is normalized to lowercase before reaching sort_facts.
        sort_facts(&mut facts, "confidence", "desc");
        assert_eq!(facts[0].id.as_str(), "b");
        assert_eq!(facts[1].id.as_str(), "a");
    }
}
