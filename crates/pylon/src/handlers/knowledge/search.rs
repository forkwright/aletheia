//! Knowledge search and timeline handlers.

use axum::Json;
use axum::extract::{Query, State};

use crate::error::ApiError;
use crate::state::KnowledgeState;

#[cfg(feature = "knowledge-store")]
use super::SimilarFact;
use super::{
    EntityRelationship, FactsQuery, SearchQuery, SearchResponse, SearchResult, TimelineEvent,
    TimelineQuery, TimelineResponse, default_order, default_sort,
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
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn search(
    State(state): State<KnowledgeState>,
    Query(mut query): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError> {
    if query.q.trim().is_empty() {
        return Err(crate::error::BadRequestSnafu {
            message: "search query 'q' must not be empty",
        }
        .build());
    }

    let max_search_limit = state.config.read().await.api_limits.max_search_limit;
    if query.limit > max_search_limit {
        return Err(crate::error::BadRequestSnafu {
            message: format!("limit must not exceed {max_search_limit}"),
        }
        .build());
    }
    query.limit = query.limit.min(max_search_limit);

    // WHY(#1252): the caller-supplied nous_id must reach get_stored_facts — a
    // hardcoded nous_id: None makes the store return empty for agent-scoped facts.
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
        .filter(|f| !f.lifecycle.is_forgotten)
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
                score *= f.provenance.confidence;
                Some(SearchResult {
                    id: f.id.to_string(),
                    content: f.content.clone(),
                    confidence: f.provenance.confidence,
                    tier: f.provenance.tier.as_str().to_string(),
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
        ("limit" = Option<usize>, Query, description = "Maximum events (default: 100, max: 1000)"),
        ("offset" = Option<usize>, Query, description = "Pagination offset"),
    ),
    responses(
        (status = 200, description = "Fact activity timeline with total count"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn timeline(
    State(state): State<KnowledgeState>,
    Query(mut query): Query<TimelineQuery>,
) -> Result<Json<TimelineResponse>, ApiError> {
    let max_facts_limit = state.config.read().await.api_limits.max_facts_limit;
    query.limit = query.limit.min(max_facts_limit);

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
        if fact.lifecycle.is_forgotten {
            continue;
        }
        events.push(TimelineEvent {
            timestamp: fact.temporal.recorded_at.to_string(),
            event_type: "created".to_string(),
            description: truncate_content(&fact.content, 80),
            fact_id: fact.id.to_string(),
            confidence: Some(fact.provenance.confidence),
        });

        if fact.access.access_count > 0 && fact.access.last_accessed_at.is_some() {
            events.push(TimelineEvent {
                timestamp: fact
                    .access
                    .last_accessed_at
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
                event_type: "accessed".to_string(),
                description: format!(
                    "{} accesses, stability: {:.0}h",
                    fact.access.access_count, fact.provenance.stability_hours
                ),
                fact_id: fact.id.to_string(),
                confidence: Some(fact.provenance.confidence),
            });
        }
    }

    events.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    let total = events.len();

    let start = query.offset.min(events.len());
    let end = (start + query.limit).min(events.len());
    // start and end are both bounded by events.len() via .min()
    #[expect(
        clippy::indexing_slicing,
        reason = "start and end are bounded by events.len() via .min()"
    )]
    let events = events[start..end].to_vec();

    Ok(Json(TimelineResponse { events, total }))
}

pub(super) fn get_stored_facts(
    state: &KnowledgeState,
    query: &FactsQuery,
) -> Vec<mneme::knowledge::Fact> {
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

pub(super) fn get_stored_entities(state: &KnowledgeState) -> Vec<mneme::knowledge::Entity> {
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
    _fact: &mneme::knowledge::Fact,
) -> Vec<mneme::knowledge::Relationship> {
    Vec::new()
}

pub(super) fn get_entity_relationships(
    state: &KnowledgeState,
    entity_id: &str,
) -> Result<Vec<EntityRelationship>, ApiError> {
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        use std::collections::HashMap;

        let entities = store.list_entities().map_err(|e| ApiError::Internal {
            message: e.to_string(),
            location: snafu::location!(),
        })?;
        let entity_names: HashMap<String, String> = entities
            .into_iter()
            .map(|entity| (entity.id.as_str().to_owned(), entity.name))
            .collect();
        let relationships = store
            .list_all_relationships()
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
                location: snafu::location!(),
            })?;

        let mut views = Vec::new();
        for relationship in relationships {
            let src = relationship.src.as_str();
            let dst = relationship.dst.as_str();
            let (entity_id, direction) = if src == entity_id {
                (dst.to_owned(), super::RelationshipDirection::Outgoing)
            } else if dst == entity_id {
                (src.to_owned(), super::RelationshipDirection::Incoming)
            } else {
                continue;
            };
            let entity_name = entity_names
                .get(&entity_id)
                .cloned()
                .unwrap_or_else(|| entity_id.clone());
            views.push(EntityRelationship {
                id: format!(
                    "{src}:{dst}:{}:{}",
                    relationship.relation, relationship.created_at
                ),
                entity_id,
                entity_name,
                relationship_type: relationship.relation,
                direction,
                confidence: relationship.weight,
            });
        }
        return Ok(views);
    }

    #[cfg(not(feature = "knowledge-store"))]
    let _ = (state, entity_id);

    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not enabled on this server".to_owned(),
        location: snafu::location!(),
    })
}

#[cfg(feature = "knowledge-store")]
pub(super) fn get_similar_facts(
    _state: &KnowledgeState,
    _fact: &mneme::knowledge::Fact,
) -> Vec<SimilarFact> {
    Vec::new()
}

pub(super) fn sort_facts(facts: &mut [mneme::knowledge::Fact], sort: &str, order: &str) {
    let desc = order == "desc";
    match sort {
        "confidence" => facts.sort_by(|a, b| {
            let cmp = a
                .provenance
                .confidence
                .partial_cmp(&b.provenance.confidence)
                .unwrap_or(std::cmp::Ordering::Equal);
            if desc { cmp.reverse() } else { cmp }
        }),
        "recency" => facts.sort_by(|a, b| {
            let cmp = a.access.last_accessed_at.cmp(&b.access.last_accessed_at);
            if desc { cmp.reverse() } else { cmp }
        }),
        "created" => facts.sort_by(|a, b| {
            let cmp = a.temporal.recorded_at.cmp(&b.temporal.recorded_at);
            if desc { cmp.reverse() } else { cmp }
        }),
        "access_count" => facts.sort_by(|a, b| {
            let cmp = a.access.access_count.cmp(&b.access.access_count);
            if desc { cmp.reverse() } else { cmp }
        }),
        "fsrs_review" => facts.sort_by(|a, b| {
            let cmp = a
                .provenance
                .stability_hours
                .partial_cmp(&b.provenance.stability_hours)
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
