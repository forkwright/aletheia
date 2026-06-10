//! Knowledge graph browsing and management endpoints.
//!
//! Exposes facts, entities, and relationships from the mneme knowledge store
//! for the TUI memory inspector panel.

use axum::Json;
use axum::extract::{Path, Query, State};

use crate::error::{ApiError, BadRequestSnafu};
use crate::state::KnowledgeState;

mod dto;
pub(crate) mod entity;
#[cfg(test)]
pub(crate) use dto::default_limit;
pub use dto::{
    EntitiesQuery, EntitiesResponse, EntityMemory, FactDetailResponse, FactsQuery, FactsResponse,
    FlagRequest, FlagSeverity, ForgetRequest, GraphCheckReport, MergeRequest,
    RelationshipsResponse, SearchQuery, SearchResponse, SearchResult, SimilarFact, TimelineEvent,
    TimelineQuery, TimelineResponse, UpdateConfidenceRequest, UpdateSensitivityRequest,
};
pub(crate) use dto::{default_order, default_sort};
pub use entity::{
    __path_delete_entity, __path_entity_memories, __path_flag_entity, __path_get_entity,
    __path_merge_entities,
};
pub use entity::{delete_entity, entity_memories, flag_entity, get_entity, merge_entities};

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

// MAX_SEARCH_LIMIT is now read from `config.api_limits.max_search_limit` at runtime.

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
            message: format!("invalid order '{order}': valid values are asc, desc"),
        }
        .build());
    }
    Ok(())
}

/// GET /api/v1/knowledge/facts
///
/// List facts with sorting, filtering, and pagination.
/// When the knowledge store is absent or unavailable, the endpoint returns an
/// empty fact list.
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
        ("limit" = Option<usize>, Query, description = "Maximum results (default: 100, max: 1000)"),
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
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn list_facts(
    State(state): State<KnowledgeState>,
    Query(mut query): Query<FactsQuery>,
) -> Result<Json<FactsResponse>, ApiError> {
    use mneme::knowledge::EpistemicTier;

    let max_facts_limit = state.config.read().await.api_limits.max_facts_limit;
    query.limit = query.limit.min(max_facts_limit);
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
            facts.retain(|f| f.provenance.tier == t);
        }
    }

    if !query.include_forgotten {
        facts.retain(|f| !f.lifecycle.is_forgotten);
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
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn get_fact(
    #[cfg_attr(
        not(feature = "knowledge-store"),
        expect(
            unused_variables,
            reason = "state only used when knowledge-store feature is enabled"
        )
    )]
    State(state): State<KnowledgeState>,
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
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/entities",
    params(
        ("limit" = Option<usize>, Query, description = "Maximum results (default: 100, max: 1000)"),
        ("offset" = Option<usize>, Query, description = "Pagination offset"),
    ),
    responses(
        (status = 200, description = "Entity list with total count"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_entities(
    State(state): State<KnowledgeState>,
    Query(mut query): Query<EntitiesQuery>,
) -> Result<Json<EntitiesResponse>, ApiError> {
    let max_facts_limit = state.config.read().await.api_limits.max_facts_limit;
    query.limit = query.limit.min(max_facts_limit);

    let entities = get_stored_entities(&state);
    let total = entities.len();

    let start = query.offset.min(entities.len());
    let end = (start + query.limit).min(entities.len());
    // start and end are both bounded by entities.len() via .min()
    #[expect(
        clippy::indexing_slicing,
        reason = "start and end are bounded by entities.len() via .min()"
    )]
    let entities = entities[start..end].to_vec();

    Ok(Json(EntitiesResponse { entities, total }))
}

/// GET /api/v1/knowledge/entities/{id}/relationships
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/entities/{id}/relationships",
    params(("id" = String, Path, description = "Entity ID")),
    responses(
        (status = 200, description = "Entity relationships"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 503, description = "Knowledge store not enabled", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn entity_relationships(
    State(state): State<KnowledgeState>,
    Path(id): Path<String>,
) -> Result<Json<RelationshipsResponse>, ApiError> {
    let relationships = get_entity_relationships(&state, &id)?;
    Ok(Json(RelationshipsResponse { relationships }))
}

mod bulk_import;
mod ingest;
mod mutation;
mod search;
mod webhook;

pub use bulk_import::{__path_import_facts, import_facts};
pub use ingest::{__path_ingest, IngestFactError, IngestRequest, IngestResponse, ingest};
pub use mutation::{
    __path_forget_fact, __path_restore_fact, __path_update_confidence, __path_update_sensitivity,
    forget_fact, restore_fact, update_confidence, update_sensitivity,
};
#[cfg(test)]
use search::truncate_content;
pub use search::{__path_search, __path_timeline, search, timeline};
use search::{get_entity_relationships, get_stored_entities, get_stored_facts, sort_facts};
#[cfg(feature = "knowledge-store")]
use search::{get_fact_relationships, get_similar_facts};
pub use webhook::{
    __path_webhook_ingest, WebhookIngestRequest, WebhookIngestResponse, webhook_ingest,
};

/// GET /api/v1/knowledge/check -- run graph consistency checks.
///
/// Runs server-side; avoids the fjall exclusive-lock conflict that occurs when
/// `aletheia memory check` tries to open the store while the server holds it.
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/check",
    responses(
        (status = 200, description = "Graph health report"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 503, description = "Knowledge store not enabled", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn check_graph_health(
    State(state): State<KnowledgeState>,
) -> impl axum::response::IntoResponse {
    use axum::response::IntoResponse as _;

    #[cfg(feature = "knowledge-store")]
    {
        use axum::http::StatusCode;
        if let Some(ref store) = state.knowledge_store {
            let report = build_graph_check_report(store);
            return match report {
                Ok(r) => (StatusCode::OK, Json(r)).into_response(),
                Err(e) => ApiError::Internal {
                    message: e,
                    location: snafu::location!(),
                }
                .into_response(),
            };
        }
    }

    let _ = &state;
    ApiError::ServiceUnavailable {
        message: "knowledge store not enabled on this server".to_owned(),
        location: snafu::location!(),
    }
    .into_response()
}

#[cfg(feature = "knowledge-store")]
fn build_graph_check_report(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
) -> Result<GraphCheckReport, String> {
    use std::collections::BTreeMap;

    fn count_relation(
        store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
        relation: &str,
    ) -> Result<usize, String> {
        let key_field = match relation {
            "relationships" => "src",
            "fact_entities" => "fact_id",
            _ => "id",
        };
        let script =
            format!("row[{key_field}] := *{relation}{{{key_field}}} \n?[count(k)] := row[k]");
        let result = store
            .run_query(&script, BTreeMap::new())
            .map_err(|e| format!("query failed: {e}"))?;
        let col = result.headers.first().map_or("count(k)", String::as_str);
        let count = result.get_i64(0, col).unwrap_or(0);
        Ok(usize::try_from(count).unwrap_or(0))
    }

    fn count_orphaned_entities(
        store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    ) -> Result<usize, String> {
        let script = r"
            ?[id] :=
                *entities{id},
                not *relationships{src: id},
                not *relationships{dst: id},
                not *fact_entities{entity_id: id}
        ";
        let result = store
            .run_query(script, BTreeMap::new())
            .map_err(|e| format!("orphan query failed: {e}"))?;
        Ok(result.row_count())
    }

    fn count_dangling_edges(
        store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    ) -> Result<usize, String> {
        let script = r"
            ?[src, dst, relation] :=
                *relationships{src, dst, relation},
                not *entities{id: src}

            ?[src, dst, relation] :=
                *relationships{src, dst, relation},
                not *entities{id: dst}
        ";
        let result = store
            .run_query(script, BTreeMap::new())
            .map_err(|e| format!("dangling edge query failed: {e}"))?;
        Ok(result.row_count())
    }

    let fact_count = count_relation(store, "facts")?;
    let entity_count = count_relation(store, "entities")?;
    let relationship_count = count_relation(store, "relationships")?;
    let orphaned_entity_count = count_orphaned_entities(store)?;
    let dangling_edge_count = count_dangling_edges(store)?;

    let healthy = orphaned_entity_count == 0 && dangling_edge_count == 0;

    Ok(GraphCheckReport {
        fact_count,
        entity_count,
        relationship_count,
        orphaned_entity_count,
        dangling_edge_count,
        status: if healthy { "healthy" } else { "issues_found" },
    })
}

#[cfg(test)]
mod knowledge_tests;
