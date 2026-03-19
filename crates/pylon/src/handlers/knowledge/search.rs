//! Knowledge search and timeline handlers.

use axum::Json;
use axum::extract::{Query, State};

use crate::error::ApiError;
use crate::state::KnowledgeState;

#[cfg(feature = "knowledge-store")]
use super::SimilarFact;
use super::{
    FactsQuery, MAX_SEARCH_LIMIT, SearchQuery, SearchResponse, SearchResult, TimelineEvent,
    TimelineResponse, default_order, default_sort,
};

/// GET /api/v1/knowledge/search
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/search",
    params(
        ("q" = String, Query, description = "Search query text"),
        ("nous_id" = Option<String>, Query, description = "Filter by agent ID"),
        ("limit" = Option<usize>, Query, description = "Maximum results (default: 20)"),
    ),
    responses(
        (status = 200, description = "Search results ranked by relevance"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn search(
    State(state): State<KnowledgeState>,
    Query(mut query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError> {
    query.limit = query.limit.min(MAX_SEARCH_LIMIT);

    // WHY: Pass the caller-supplied nous_id so get_stored_facts can query the store.
    // The previous call to get_all_facts hardcoded nous_id: None, causing the store
    // to return empty even when facts were persisted under a specific agent (Bug #1252).
    let facts_query = FactsQuery {
        nous_id: query.nous_id.clone(),
        sort: default_sort(),
        order: default_order(),
        filter: None,
        fact_type: None,
        tier: None,
        limit: 10_000,
        offset: 0,
        include_forgotten: false,
    };
    let all_facts = get_stored_facts(&state, &facts_query);
    let query_lower = query.q.to_lowercase();
    let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

    let mut results: Vec<SearchResult> = all_facts
        .iter()
        .filter(|f| !f.is_forgotten)
        .filter_map(|f| {
            let content_lower = f.content.to_lowercase();
            // NOTE: Simple BM25-like scoring: term frequency weighted by confidence.
            let mut score = 0.0_f64;
            for term in &query_terms {
                if content_lower.contains(term) {
                    score += 1.0;
                }
            }
            if score > 0.0 {
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
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/timeline",
    params(
        ("nous_id" = Option<String>, Query, description = "Filter by agent ID"),
    ),
    responses(
        (status = 200, description = "Fact activity timeline in chronological order"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn timeline(
    State(state): State<KnowledgeState>,
    Query(query): Query<FactsQuery>,
) -> Result<Json<TimelineResponse>, ApiError> {
    // WHY: Pass the caller-supplied nous_id so get_stored_facts can query the store.
    // The previous call to get_all_facts hardcoded nous_id: None, causing the store
    // to return empty even when facts were persisted under a specific agent (Bug #1252).
    let timeline_query = FactsQuery {
        nous_id: query.nous_id,
        sort: default_sort(),
        order: default_order(),
        filter: None,
        fact_type: None,
        tier: None,
        limit: 10_000,
        offset: 0,
        include_forgotten: false,
    };
    let facts = get_stored_facts(&state, &timeline_query);
    let mut events: Vec<TimelineEvent> = Vec::new();

    for fact in &facts {
        if fact.is_forgotten {
            continue;
        }
        events.push(TimelineEvent {
            timestamp: fact.recorded_at.to_string(),
            event_type: "created".to_string(),
            description: truncate_content(&fact.content, 80),
            fact_id: fact.id.to_string(),
            confidence: Some(fact.confidence),
        });

        if fact.access_count > 0 && fact.last_accessed_at.is_some() {
            events.push(TimelineEvent {
                timestamp: fact
                    .last_accessed_at
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
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

pub(super) fn get_stored_facts(
    state: &KnowledgeState,
    query: &FactsQuery,
) -> Vec<aletheia_mneme::knowledge::Fact> {
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let fetch_limit =
            i64::try_from((query.offset + query.limit).min(10_000)).unwrap_or(i64::MAX);
        let result = if let Some(nous_id) = query.nous_id.as_deref() {
            store.audit_all_facts(nous_id, fetch_limit)
        } else {
            store.list_all_facts(fetch_limit)
        };
        match result {
            Ok(facts) => return facts,
            Err(e) => {
                tracing::warn!(error = %e, "failed to query knowledge store");
            }
        }
    }
    #[cfg(not(feature = "knowledge-store"))]
    let _ = (state, query);
    Vec::new()
}

pub(super) fn get_stored_entities(
    state: &KnowledgeState,
) -> Vec<aletheia_mneme::knowledge::Entity> {
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        match store.list_entities() {
            Ok(entities) => return entities,
            Err(e) => {
                tracing::warn!(error = %e, "failed to query knowledge store entities");
            }
        }
    }
    #[cfg(not(feature = "knowledge-store"))]
    let _ = state;
    Vec::new()
}

#[cfg(feature = "knowledge-store")]
pub(super) fn get_fact_relationships(
    _state: &KnowledgeState,
    _fact: &aletheia_mneme::knowledge::Fact,
) -> Vec<aletheia_mneme::knowledge::Relationship> {
    Vec::new()
}

pub(super) fn get_entity_relationships(
    _state: &KnowledgeState,
    _entity_id: &str,
) -> Vec<aletheia_mneme::knowledge::Relationship> {
    Vec::new()
}

#[cfg(feature = "knowledge-store")]
pub(super) fn get_similar_facts(
    _state: &KnowledgeState,
    _fact: &aletheia_mneme::knowledge::Fact,
) -> Vec<SimilarFact> {
    Vec::new()
}

pub(super) fn sort_facts(facts: &mut [aletheia_mneme::knowledge::Fact], sort: &str, order: &str) {
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
        _ => {
            // NOTE: unrecognized sort field, facts retain original order
        }
    }
}

pub(crate) fn truncate_content(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", s.get(..end).unwrap_or(s))
    }
}
