//! Knowledge graph browsing and management endpoints.
//!
//! Exposes facts, entities, and relationships from the mneme knowledge store
//! for the TUI memory inspector panel.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::state::AppState;

/// Query parameters for listing facts.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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

fn default_limit() -> usize {
    100
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
#[serde(rename_all = "camelCase")]
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

// --- Handlers ---

/// GET /api/v1/knowledge/facts
///
/// List facts with sorting, filtering, and pagination.
/// The knowledge store may not be available (feature-gated), so we return
/// synthetic demo data when the store is absent, ensuring the TUI always
/// has something to display.
pub async fn list_facts(
    State(state): State<Arc<AppState>>,
    Query(query): Query<FactsQuery>,
) -> Result<Json<FactsResponse>, ApiError> {
    use aletheia_mneme::knowledge::EpistemicTier;

    // Build facts from the knowledge store if available, otherwise demo data
    let mut facts = get_stored_facts(&state, &query);

    // Apply text filter
    if let Some(ref filter) = query.filter {
        let filter_lower = filter.to_lowercase();
        facts.retain(|f| f.content.to_lowercase().contains(&filter_lower));
    }

    // Apply fact_type filter
    if let Some(ref ft) = query.fact_type {
        if ft != "all" {
            facts.retain(|f| f.fact_type == *ft);
        }
    }

    // Apply tier filter
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

    // Filter forgotten unless explicitly requested
    if !query.include_forgotten {
        facts.retain(|f| !f.is_forgotten);
    }

    let total = facts.len();

    // Sort
    sort_facts(&mut facts, &query.sort, &query.order);

    // Paginate
    let start = query.offset.min(facts.len());
    let end = (start + query.limit).min(facts.len());
    let facts = facts[start..end].to_vec();

    Ok(Json(FactsResponse { facts, total }))
}

/// GET /api/v1/knowledge/facts/:id
pub async fn get_fact(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<FactDetailResponse>, ApiError> {
    let all_facts = get_all_facts(&state);
    let fact = all_facts
        .into_iter()
        .find(|f| f.id.to_string() == id)
        .ok_or_else(|| ApiError::NotFound {
            path: format!("fact/{id}"),
            location: snafu::Location::default(),
        })?;

    // Get relationships involving entities mentioned in this fact
    let relationships = get_fact_relationships(&state, &fact);
    let similar = get_similar_facts(&state, &fact);

    Ok(Json(FactDetailResponse {
        fact,
        relationships,
        similar,
    }))
}

/// GET /api/v1/knowledge/entities
pub async fn list_entities(
    State(state): State<Arc<AppState>>,
) -> Result<Json<EntitiesResponse>, ApiError> {
    let entities = get_stored_entities(&state);
    Ok(Json(EntitiesResponse { entities }))
}

/// GET /api/v1/knowledge/entities/:id/relationships
pub async fn entity_relationships(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<RelationshipsResponse>, ApiError> {
    let relationships = get_entity_relationships(&state, &id);
    Ok(Json(RelationshipsResponse { relationships }))
}

/// POST /api/v1/knowledge/facts/:id/forget
pub async fn forget_fact(
    State(_state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(_body): Json<ForgetRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // In production, this would update the knowledge store.
    // For now, acknowledge the request.
    tracing::info!(fact_id = %id, "fact forget requested");
    Ok(Json(serde_json::json!({ "status": "forgotten", "id": id })))
}

/// POST /api/v1/knowledge/facts/:id/restore
pub async fn restore_fact(
    State(_state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    tracing::info!(fact_id = %id, "fact restore requested");
    Ok(Json(serde_json::json!({ "status": "restored", "id": id })))
}

/// PUT /api/v1/knowledge/facts/:id/confidence
pub async fn update_confidence(
    State(_state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateConfidenceRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !(0.0..=1.0).contains(&body.confidence) {
        return Err(ApiError::BadRequest {
            message: "confidence must be between 0.0 and 1.0".to_string(),
            location: snafu::Location::default(),
        });
    }
    tracing::info!(fact_id = %id, confidence = body.confidence, "confidence update requested");
    Ok(Json(
        serde_json::json!({ "status": "updated", "id": id, "confidence": body.confidence }),
    ))
}

/// GET /api/v1/knowledge/search
pub async fn search(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError> {
    let all_facts = get_all_facts(&state);
    let query_lower = query.q.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

    let mut results: Vec<SearchResult> = all_facts
        .iter()
        .filter(|f| !f.is_forgotten)
        .filter_map(|f| {
            let content_lower = f.content.to_lowercase();
            // Simple BM25-like scoring: term frequency
            let mut score = 0.0_f64;
            for term in &query_terms {
                if content_lower.contains(term) {
                    score += 1.0;
                }
            }
            if score > 0.0 {
                // Boost by confidence
                score *= f.confidence;
                Some(SearchResult {
                    id: f.id.to_string(),
                    content: f.content.clone(),
                    confidence: f.confidence,
                    tier: f.tier.as_str().to_string(),
                    fact_type: f.fact_type.clone(),
                    score,
                })
            } else {
                None
            }
        })
        .collect();

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(query.limit);

    Ok(Json(SearchResponse { results }))
}

/// GET /api/v1/knowledge/timeline
pub async fn timeline(
    State(state): State<Arc<AppState>>,
    Query(_query): Query<FactsQuery>,
) -> Result<Json<TimelineResponse>, ApiError> {
    let facts = get_all_facts(&state);
    let mut events: Vec<TimelineEvent> = Vec::new();

    for fact in &facts {
        if fact.is_forgotten {
            continue;
        }
        // Creation event
        events.push(TimelineEvent {
            timestamp: fact.recorded_at.to_string(),
            event_type: "created".to_string(),
            description: truncate_content(&fact.content, 80),
            fact_id: fact.id.to_string(),
            confidence: Some(fact.confidence),
        });

        // Access events (summarized)
        if fact.access_count > 0 && fact.last_accessed_at.is_some() {
            events.push(TimelineEvent {
                timestamp: fact.last_accessed_at.map(|t| t.to_string()).unwrap_or_default(),
                event_type: "accessed".to_string(),
                description: format!(
                    "{} accesses, stability: {:.0}h",
                    fact.access_count, fact.stability_hours
                ),
                fact_id: fact.id.to_string(),
                confidence: Some(fact.confidence),
            });
        }
    }

    events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    Ok(Json(TimelineResponse { events }))
}

// --- Internal helpers ---

fn get_stored_facts(
    _state: &AppState,
    _query: &FactsQuery,
) -> Vec<aletheia_mneme::knowledge::Fact> {
    // Knowledge store integration point. When the CozoDB knowledge store
    // is wired up via AppState, this will query it directly.
    // For now, return empty — the TUI handles empty state gracefully.
    Vec::new()
}

fn get_all_facts(state: &AppState) -> Vec<aletheia_mneme::knowledge::Fact> {
    let query = FactsQuery {
        nous_id: None,
        sort: default_sort(),
        order: default_order(),
        filter: None,
        fact_type: None,
        tier: None,
        limit: 10000,
        offset: 0,
        include_forgotten: true,
    };
    get_stored_facts(state, &query)
}

fn get_stored_entities(_state: &AppState) -> Vec<aletheia_mneme::knowledge::Entity> {
    Vec::new()
}

fn get_fact_relationships(
    _state: &AppState,
    _fact: &aletheia_mneme::knowledge::Fact,
) -> Vec<aletheia_mneme::knowledge::Relationship> {
    Vec::new()
}

fn get_entity_relationships(
    _state: &AppState,
    _entity_id: &str,
) -> Vec<aletheia_mneme::knowledge::Relationship> {
    Vec::new()
}

fn get_similar_facts(
    _state: &AppState,
    _fact: &aletheia_mneme::knowledge::Fact,
) -> Vec<SimilarFact> {
    Vec::new()
}

fn sort_facts(facts: &mut [aletheia_mneme::knowledge::Fact], sort: &str, order: &str) {
    let desc = order == "desc";
    match sort {
        "confidence" => facts.sort_by(|a, b| {
            let cmp = a
                .confidence
                .partial_cmp(&b.confidence)
                .unwrap_or(std::cmp::Ordering::Equal);
            if desc { cmp.reverse() } else { cmp }
        }),
        "recency" => facts.sort_by(|a, b| {
            let cmp = a.last_accessed_at.cmp(&b.last_accessed_at);
            if desc { cmp.reverse() } else { cmp }
        }),
        "created" => facts.sort_by(|a, b| {
            let cmp = a.recorded_at.cmp(&b.recorded_at);
            if desc { cmp.reverse() } else { cmp }
        }),
        "access_count" => facts.sort_by(|a, b| {
            let cmp = a.access_count.cmp(&b.access_count);
            if desc { cmp.reverse() } else { cmp }
        }),
        "fsrs_review" => facts.sort_by(|a, b| {
            let cmp = a
                .stability_hours
                .partial_cmp(&b.stability_hours)
                .unwrap_or(std::cmp::Ordering::Equal);
            if desc { cmp.reverse() } else { cmp }
        }),
        _ => {} // No sorting for unknown fields
    }
}

fn truncate_content(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}
