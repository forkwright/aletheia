//! Knowledge graph browsing and management endpoints.
//!
//! Exposes facts, entities, and relationships from the mneme knowledge store
//! for the TUI memory inspector panel.

use axum::Json;
use axum::extract::{Path, Query, State};

use crate::error::{ApiError, BadRequestSnafu};
use crate::extract::{Claims, require_nous_access};
use crate::state::KnowledgeState;

mod dto;
pub(crate) mod entity;
#[cfg(test)]
pub(crate) use dto::default_limit;
pub use dto::{
    EntitiesQuery, EntitiesResponse, EntityListItem, EntityMemory, EntityRelationship,
    ExplainCandidate, ExplainDecision, ExplainQuery, ExplainResponse, FactDetailResponse,
    FactorScoreBreakdown, FactsQuery, FactsResponse, FlagRequest, FlagSeverity, ForgetRequest,
    GraphCheckReport, MergeRequest, RecallWeightsView, RelationshipDirection,
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

/// Valid sort fields for entity listing.
const VALID_ENTITY_SORT_FIELDS: &[&str] = &[
    "page_rank",
    "confidence",
    "memory_count",
    "relationship_count",
    "updated_at",
    "name",
];

pub(super) fn require_fact_nous_access(
    claims: &Claims,
    fact: &mneme::knowledge::Fact,
) -> Result<(), ApiError> {
    require_nous_access(claims, &fact.nous_id)
}

pub(super) fn require_facts_nous_access<'a>(
    claims: &Claims,
    facts: impl IntoIterator<Item = &'a mneme::knowledge::Fact>,
) -> Result<(), ApiError> {
    for fact in facts {
        require_fact_nous_access(claims, fact)?;
    }
    Ok(())
}

pub(super) fn require_facts_match_target<'a>(
    facts: impl IntoIterator<Item = &'a mneme::knowledge::Fact>,
    target_nous_id: &str,
) -> Result<(), ApiError> {
    for fact in facts {
        if fact.nous_id != target_nous_id {
            return Err(ApiError::BadRequest {
                message: format!(
                    "fact {} nous_id '{}' does not match target nous_id '{}'",
                    fact.id, fact.nous_id, target_nous_id
                ),
                location: snafu::location!(),
            });
        }
    }
    Ok(())
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
            message: format!("invalid order '{order}': valid values are asc, desc"),
        }
        .build());
    }
    Ok(())
}

/// Validate entity sort/order query parameters.
fn validate_entity_sort_order(sort: &str, order: &str) -> Result<(), ApiError> {
    if !VALID_ENTITY_SORT_FIELDS.contains(&sort) {
        return Err(BadRequestSnafu {
            message: format!(
                "invalid sort field '{sort}': valid fields are {}",
                VALID_ENTITY_SORT_FIELDS.join(", "),
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
        ("q" = Option<String>, Query, description = "Search text filter"),
        ("sort" = Option<String>, Query, description = "Sort field: page_rank, confidence, memory_count, relationship_count, updated_at, name (default: page_rank)"),
        ("order" = Option<String>, Query, description = "Sort direction: asc or desc (default: desc)"),
        ("entity_type" = Option<Vec<String>>, Query, description = "Entity type filter; repeat to include multiple types"),
        ("min_confidence" = Option<f64>, Query, description = "Minimum confidence threshold"),
        ("agent" = Option<Vec<String>>, Query, description = "Agent filter; repeat to include multiple agents"),
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
    validate_entity_sort_order(&query.sort, &query.order)?;
    query.order = query.order.to_ascii_lowercase();

    let mut entities = get_stored_entities(&state);
    let entity_stats_map: std::collections::HashMap<String, EntityStats> = {
        #[cfg(feature = "knowledge-store")]
        {
            load_entity_stats(&state, &query)
        }
        #[cfg(not(feature = "knowledge-store"))]
        {
            std::collections::HashMap::new()
        }
    };

    if !query.agent.is_empty() {
        #[cfg(feature = "knowledge-store")]
        let allowed = load_agent_entity_ids(&state, &query.agent)?;
        #[cfg(not(feature = "knowledge-store"))]
        let allowed: std::collections::HashSet<String> = std::collections::HashSet::new();
        entities.retain(|entity| allowed.contains(entity.id.as_str()));
    }

    if let Some(ref filter) = query.q {
        let filter_lower = filter.to_lowercase();
        entities.retain(|entity| {
            entity.name.to_lowercase().contains(&filter_lower)
                || entity
                    .aliases
                    .iter()
                    .any(|alias| alias.to_lowercase().contains(&filter_lower))
                || entity.entity_type.to_lowercase().contains(&filter_lower)
        });
    }

    if !query.entity_type.is_empty() {
        let entity_types: std::collections::HashSet<String> = query
            .entity_type
            .iter()
            .map(|ty| ty.to_lowercase())
            .collect();
        entities.retain(|entity| entity_types.contains(&entity.entity_type.to_lowercase()));
    }

    if let Some(min_confidence) = query.min_confidence {
        entities.retain(|entity| {
            entity_stats_map
                .get(entity.id.as_str())
                .map_or(0.0, |item: &EntityStats| item.confidence)
                >= min_confidence
        });
    }

    let total = entities.len();

    let mut entities: Vec<EntityListItem> = entities
        .into_iter()
        .map(|entity| {
            let entity_id = entity.id.as_str().to_owned();
            build_entity_list_item(entity, entity_stats_map.get(&entity_id))
        })
        .collect();

    sort_entity_items(&mut entities, &query.sort, &query.order);

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

#[derive(Debug, Clone, Default)]
struct EntityStats {
    confidence: f64,
    page_rank: f64,
    memory_count: u32,
    relationship_count: u32,
}

#[cfg(feature = "knowledge-store")]
fn load_agent_entity_ids(
    state: &KnowledgeState,
    agents: &[String],
) -> Result<std::collections::HashSet<String>, ApiError> {
    use std::collections::{BTreeMap, HashSet};

    let mut entity_ids = HashSet::new();
    let Some(store) = state.knowledge_store.as_ref() else {
        return Ok(entity_ids);
    };

    for agent in agents {
        let script = r"
            ?[entity_id] :=
                *fact_entities{fact_id, entity_id},
                *facts{id: fact_id, nous_id, is_forgotten, superseded_by},
                nous_id == $nous_id,
                is_forgotten == false,
                is_null(superseded_by)
        ";
        let mut params = BTreeMap::new();
        params.insert(
            "nous_id".to_owned(),
            mneme::engine::DataValue::Str(agent.clone().into()),
        );
        let result = store
            .run_query(script, params)
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
                location: snafu::location!(),
            })?;
        for row in 0..result.row_count() {
            if let Some(entity_id) = result.get_string(row, "entity_id") {
                entity_ids.insert(entity_id);
            }
        }
    }

    Ok(entity_ids)
}

#[cfg(feature = "knowledge-store")]
fn load_entity_stats(
    state: &KnowledgeState,
    query: &EntitiesQuery,
) -> std::collections::HashMap<String, EntityStats> {
    use std::collections::{BTreeMap, HashMap};

    let mut entity_stats: HashMap<String, EntityStats> = HashMap::new();
    let Some(store) = state.knowledge_store.as_ref() else {
        return entity_stats;
    };

    // NOTE: the entity list is already a page-sized slice, so it is safe to load
    // the broader graph counters once and filter in memory.
    if let Ok(relationships) = store.list_all_relationships() {
        for relationship in relationships {
            let src = relationship.src.as_str().to_owned();
            let dst = relationship.dst.as_str().to_owned();
            entity_stats.entry(src).or_default().relationship_count += 1;
            entity_stats.entry(dst).or_default().relationship_count += 1;
        }
    }

    let mut params = BTreeMap::new();
    let agent_filter = build_agent_filter_clause(&query.agent, &mut params);

    let fact_stats_script = format!(
        r"
            ?[entity_id, count(fact_id), mean(confidence)] :=
                *fact_entities{{fact_id, entity_id}},
                *facts{{id: fact_id, confidence, nous_id, is_forgotten, superseded_by}},
                is_forgotten == false,
                is_null(superseded_by)
                {agent_filter}
        "
    );

    if let Ok(result) = store.run_query(&fact_stats_script, params) {
        for row in 0..result.row_count() {
            let Some(entity_id) = result.get_string(row, "entity_id") else {
                continue;
            };
            let memory_count = result
                .get_i64(row, "count(fact_id)")
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(0);
            let confidence = result.get_f64(row, "mean(confidence)").unwrap_or(0.0);
            let entry = entity_stats.entry(entity_id).or_default();
            entry.memory_count = memory_count;
            entry.confidence = confidence;
        }
    }

    let pagerank_script = r"
            ?[entity_id, score] :=
                *graph_scores{{entity_id, score_type, score}},
                score_type == 'pagerank'
        "
    .to_string();

    let pagerank_params = BTreeMap::new();
    if let Ok(result) = store.run_query(&pagerank_script, pagerank_params) {
        for row in 0..result.row_count() {
            let Some(entity_id) = result.get_string(row, "entity_id") else {
                continue;
            };
            let page_rank = result.get_f64(row, "score").unwrap_or(0.0);
            entity_stats.entry(entity_id).or_default().page_rank = page_rank;
        }
    }

    entity_stats
}

#[cfg(feature = "knowledge-store")]
fn build_agent_filter_clause(
    agents: &[String],
    params: &mut std::collections::BTreeMap<String, mneme::engine::DataValue>,
) -> String {
    if agents.is_empty() {
        return String::new();
    }

    for (idx, agent) in agents.iter().enumerate() {
        params.insert(
            format!("agent_{idx}"),
            mneme::engine::DataValue::Str(agent.clone().into()),
        );
    }

    let clauses = (0..agents.len())
        .map(|idx| format!("nous_id == $agent_{idx}"))
        .collect::<Vec<_>>()
        .join(" or ");
    format!(", ({clauses})")
}

fn build_entity_list_item(
    entity: mneme::knowledge::Entity,
    stats: Option<&EntityStats>,
) -> EntityListItem {
    let stats = stats.cloned().unwrap_or_default();
    EntityListItem {
        id: entity.id.as_str().to_owned(),
        name: entity.name,
        entity_type: entity.entity_type,
        aliases: entity.aliases,
        created_at: entity.created_at.to_string(),
        updated_at: entity.updated_at.to_string(),
        confidence: stats.confidence,
        page_rank: stats.page_rank,
        memory_count: stats.memory_count,
        relationship_count: stats.relationship_count,
    }
}

fn sort_entity_items(items: &mut [EntityListItem], sort: &str, order: &str) {
    let descending = order == "desc";
    match sort {
        "page_rank" => items.sort_by(|a, b| {
            a.page_rank
                .partial_cmp(&b.page_rank)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        }),
        "confidence" => items.sort_by(|a, b| {
            a.confidence
                .partial_cmp(&b.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        }),
        "memory_count" => items.sort_by(|a, b| {
            a.memory_count
                .cmp(&b.memory_count)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        }),
        "relationship_count" => items.sort_by(|a, b| {
            a.relationship_count
                .cmp(&b.relationship_count)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        }),
        "updated_at" => items.sort_by(|a, b| {
            a.updated_at
                .cmp(&b.updated_at)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        }),
        "name" => items.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        }),
        _ => {
            // NOTE: validation rejects unknown fields before this point.
        }
    }

    if descending {
        items.reverse();
    }
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
pub use search::{__path_explain, __path_search, __path_timeline, explain, search, timeline};
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
