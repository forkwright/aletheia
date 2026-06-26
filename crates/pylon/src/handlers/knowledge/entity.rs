//! Entity detail, merge, delete, and review endpoints.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use symbolon::types::Role;

use crate::error::ApiError;
use crate::extract::{Claims, require_role};
use crate::state::KnowledgeState;

#[cfg(feature = "knowledge-store")]
use super::FlagSeverity;
use super::{EntityMemory, FlagRequest, MergeRequest};

fn require_unscoped_entity_write(claims: &Claims) -> Result<(), ApiError> {
    if claims.nous_id.is_some() {
        return Err(ApiError::forbidden(
            "scoped tokens cannot mutate aggregate knowledge entities",
        ));
    }
    Ok(())
}

#[cfg(feature = "knowledge-store")]
use mneme::engine::DataValue;
#[cfg(feature = "knowledge-store")]
use mneme::id::EntityId;
#[cfg(feature = "knowledge-store")]
use std::collections::BTreeMap;

#[cfg(feature = "knowledge-store")]
fn parse_entity_id(id: &str) -> Result<EntityId, ApiError> {
    EntityId::new(id).map_err(|e| ApiError::BadRequest {
        message: format!("invalid entity id: {e}"),
        location: snafu::location!(),
    })
}

#[cfg(feature = "knowledge-store")]
fn get_entity_from_store(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    id: &str,
) -> Result<mneme::knowledge::Entity, ApiError> {
    let entity_id = parse_entity_id(id)?;
    let entities = store.list_entities().map_err(|e| ApiError::Internal {
        message: e.to_string(),
        location: snafu::location!(),
    })?;
    entities
        .into_iter()
        .find(|entity| entity.id == entity_id)
        .ok_or_else(|| ApiError::NotFound {
            path: format!("entity/{id}"),
            location: snafu::location!(),
        })
}

#[cfg(feature = "knowledge-store")]
fn list_entity_relationship_links(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    id: &str,
) -> Result<Vec<(String, String)>, ApiError> {
    let script = r"
        ?[src, dst] :=
            *relationships{src, dst, relation, weight, created_at},
            src = $entity_id
        ?[src, dst] :=
            *relationships{src, dst, relation, weight, created_at},
            dst = $entity_id
    ";
    let mut params = BTreeMap::new();
    params.insert("entity_id".to_owned(), DataValue::Str(id.to_owned().into()));
    let rows = store
        .run_query(script, params)
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
            location: snafu::location!(),
        })?;

    let mut links = Vec::with_capacity(rows.row_count());
    for row in 0..rows.row_count() {
        let src = rows.get_string(row, "src").unwrap_or_default();
        let dst = rows.get_string(row, "dst").unwrap_or_default();
        if !src.is_empty() && !dst.is_empty() {
            links.push((src, dst));
        }
    }
    Ok(links)
}

#[cfg(feature = "knowledge-store")]
fn list_entity_fact_links(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    id: &str,
) -> Result<Vec<String>, ApiError> {
    let script = r"
        ?[fact_id] :=
            *fact_entities{fact_id, entity_id},
            entity_id = $entity_id
    ";
    let mut params = BTreeMap::new();
    params.insert("entity_id".to_owned(), DataValue::Str(id.to_owned().into()));
    let rows = store
        .run_query(script, params)
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
            location: snafu::location!(),
        })?;

    let mut fact_ids = Vec::with_capacity(rows.row_count());
    for row in 0..rows.row_count() {
        if let Some(fact_id) = rows.get_string(row, "fact_id")
            && !fact_id.is_empty()
        {
            fact_ids.push(fact_id);
        }
    }
    Ok(fact_ids)
}

#[cfg(feature = "knowledge-store")]
fn list_entity_pending_merge_links(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    id: &str,
) -> Result<Vec<(String, String)>, ApiError> {
    let script = r"
        ?[entity_a, entity_b] :=
            *pending_merges{
                entity_a,
                entity_b,
                name_a,
                name_b,
                name_similarity,
                embed_similarity,
                type_match,
                alias_overlap,
                merge_score,
                created_at
            },
            entity_a = $entity_id
        ?[entity_a, entity_b] :=
            *pending_merges{
                entity_a,
                entity_b,
                name_a,
                name_b,
                name_similarity,
                embed_similarity,
                type_match,
                alias_overlap,
                merge_score,
                created_at
            },
            entity_b = $entity_id
    ";
    let mut params = BTreeMap::new();
    params.insert("entity_id".to_owned(), DataValue::Str(id.to_owned().into()));
    let rows = store
        .run_query(script, params)
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
            location: snafu::location!(),
        })?;

    let mut pairs = Vec::with_capacity(rows.row_count());
    for row in 0..rows.row_count() {
        let entity_a = rows.get_string(row, "entity_a").unwrap_or_default();
        let entity_b = rows.get_string(row, "entity_b").unwrap_or_default();
        if !entity_a.is_empty() && !entity_b.is_empty() {
            pairs.push((entity_a, entity_b));
        }
    }
    Ok(pairs)
}

/// GET /api/v1/knowledge/entities/{id}
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/entities/{id}",
    params(("id" = String, Path, description = "Entity ID")),
    responses(
        (status = 200, description = "Entity detail"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Entity not found", body = crate::error::ErrorResponse),
        (status = 503, description = "Knowledge store not enabled", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_entity(
    State(state): State<KnowledgeState>,
    Path(id): Path<String>,
) -> Result<Json<mneme::knowledge::Entity>, ApiError> {
    #[cfg(not(feature = "knowledge-store"))]
    let _ = &id;
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let entity = get_entity_from_store(store, &id)?;
        return Ok(Json(entity));
    }

    #[cfg(not(feature = "knowledge-store"))]
    let _ = state;

    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not enabled on this server".to_owned(),
        location: snafu::location!(),
    })
}

/// GET /api/v1/knowledge/entities/{id}/memories
#[utoipa::path(
    get,
    path = "/api/v1/knowledge/entities/{id}/memories",
    params(("id" = String, Path, description = "Entity ID")),
    responses(
        (status = 200, description = "Entity memories", body = [EntityMemory]),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Entity not found", body = crate::error::ErrorResponse),
        (status = 503, description = "Knowledge store not enabled", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn entity_memories(
    State(state): State<KnowledgeState>,
    Path(id): Path<String>,
) -> Result<Json<Vec<EntityMemory>>, ApiError> {
    #[cfg(not(feature = "knowledge-store"))]
    let _ = &id;
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let _entity = get_entity_from_store(store, &id)?;
        let script = r"
            ?[fact_id, content, nous_id, source_session_id, confidence, recorded_at] :=
                *fact_entities{fact_id, entity_id},
                entity_id = $entity_id,
                *facts{
                    id: fact_id,
                    content,
                    nous_id,
                    confidence,
                    source_session_id,
                    recorded_at,
                    is_forgotten,
                    superseded_by
                },
                is_forgotten == false,
                is_null(superseded_by)
            :order -recorded_at
        ";
        let mut params = BTreeMap::new();
        params.insert("entity_id".to_owned(), DataValue::Str(id.clone().into()));
        let rows = store
            .run_query(script, params)
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
                location: snafu::location!(),
            })?;

        let mut memories = Vec::with_capacity(rows.row_count());
        for row in 0..rows.row_count() {
            memories.push(EntityMemory {
                id: rows.get_string(row, "fact_id").unwrap_or_default(),
                content: rows.get_string(row, "content").unwrap_or_default(),
                agent: rows.get_string(row, "nous_id"),
                session: rows.get_string(row, "source_session_id"),
                confidence: rows.get_f64(row, "confidence").unwrap_or_default(),
                created_at: rows.get_string(row, "recorded_at"),
            });
        }

        return Ok(Json(memories));
    }

    #[cfg(not(feature = "knowledge-store"))]
    let _ = state;

    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not enabled on this server".to_owned(),
        location: snafu::location!(),
    })
}

/// POST /api/v1/knowledge/entities/merge
#[utoipa::path(
    post,
    path = "/api/v1/knowledge/entities/merge",
    request_body = MergeRequest,
    responses(
        (status = 204, description = "Entities merged"),
        (status = 400, description = "Invalid entity id", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Entity not found", body = crate::error::ErrorResponse),
        (status = 503, description = "Knowledge store not enabled", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn merge_entities(
    State(state): State<KnowledgeState>,
    claims: Claims,
    Json(body): Json<MergeRequest>,
) -> Result<StatusCode, ApiError> {
    require_role(&claims, Role::Operator)?;
    require_unscoped_entity_write(&claims)?;
    #[cfg(not(feature = "knowledge-store"))]
    let _ = &body;

    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let canonical = parse_entity_id(&body.canonical_id)?;
        let merged = parse_entity_id(&body.merged_id)?;
        if canonical == merged {
            return Err(ApiError::BadRequest {
                message: "canonical_id and merged_id must be different".to_owned(),
                location: snafu::location!(),
            });
        }

        let _ = get_entity_from_store(store, canonical.as_str())?;
        let _ = get_entity_from_store(store, merged.as_str())?;

        store
            .approve_merge(&canonical, &merged)
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
                location: snafu::location!(),
            })?;
        return Ok(StatusCode::NO_CONTENT);
    }

    #[cfg(not(feature = "knowledge-store"))]
    let _ = state;

    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not enabled on this server".to_owned(),
        location: snafu::location!(),
    })
}

/// POST /api/v1/knowledge/entities/{id}/flag
///
/// Persist an operator review flag against the entity. The latest flag for an
/// entity overwrites any previous flag.
#[utoipa::path(
    post,
    path = "/api/v1/knowledge/entities/{id}/flag",
    params(("id" = String, Path, description = "Entity ID")),
    request_body = FlagRequest,
    responses(
        (status = 204, description = "Entity flagged"),
        (status = 400, description = "Invalid request", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 403, description = "Forbidden", body = crate::error::ErrorResponse),
        (status = 404, description = "Entity not found", body = crate::error::ErrorResponse),
        (status = 503, description = "Knowledge store not enabled", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn flag_entity(
    State(state): State<KnowledgeState>,
    claims: Claims,
    Path(id): Path<String>,
    Json(body): Json<FlagRequest>,
) -> Result<StatusCode, ApiError> {
    require_role(&claims, Role::Operator)?;
    require_unscoped_entity_write(&claims)?;

    if body.reason.trim().is_empty() {
        return Err(ApiError::BadRequest {
            message: "reason must not be empty".to_owned(),
            location: snafu::location!(),
        });
    }

    #[cfg(not(feature = "knowledge-store"))]
    let _ = (&id, &body);

    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let entity_id = parse_entity_id(&id)?;
        let _entity = get_entity_from_store(store, entity_id.as_str())?;

        let severity = match body.severity {
            FlagSeverity::Low => "low",
            FlagSeverity::Medium => "medium",
            FlagSeverity::High => "high",
        };

        store
            .flag_entity(&entity_id, body.reason.trim(), severity, &claims.sub)
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
                location: snafu::location!(),
            })?;
        return Ok(StatusCode::NO_CONTENT);
    }

    #[cfg(not(feature = "knowledge-store"))]
    let _ = state;

    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not enabled on this server".to_owned(),
        location: snafu::location!(),
    })
}

/// DELETE /api/v1/knowledge/entities/{id}
#[utoipa::path(
    delete,
    path = "/api/v1/knowledge/entities/{id}",
    params(("id" = String, Path, description = "Entity ID")),
    responses(
        (status = 204, description = "Entity deleted"),
        (status = 400, description = "Invalid entity id", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 404, description = "Entity not found", body = crate::error::ErrorResponse),
        (status = 503, description = "Knowledge store not enabled", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_entity(
    State(state): State<KnowledgeState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_role(&claims, Role::Operator)?;
    require_unscoped_entity_write(&claims)?;
    #[cfg(not(feature = "knowledge-store"))]
    let _ = &id;

    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let entity_id = parse_entity_id(&id)?;
        let _entity = get_entity_from_store(store, entity_id.as_str())?;

        let rel_links = list_entity_relationship_links(store, entity_id.as_str())?;
        for (src, dst) in rel_links {
            let mut params = BTreeMap::new();
            params.insert("src".to_owned(), DataValue::Str(src.into()));
            params.insert("dst".to_owned(), DataValue::Str(dst.into()));
            if let Err(e) = store.run_mut_query(
                r"?[src, dst] := *relationships{src, dst, relation, weight, created_at}, src = $src, dst = $dst :rm relationships {src, dst}",
                params,
            ) {
                tracing::warn!(error = %e, entity_id = %id, "failed to remove relationship during entity delete");
            }
        }

        let fact_links = list_entity_fact_links(store, entity_id.as_str())?;
        for fact_id in fact_links {
            let mut params = BTreeMap::new();
            params.insert("fact_id".to_owned(), DataValue::Str(fact_id.into()));
            params.insert(
                "entity_id".to_owned(),
                DataValue::Str(entity_id.as_str().to_owned().into()),
            );
            if let Err(e) = store.run_mut_query(
                r"?[fact_id, entity_id] := *fact_entities{fact_id, entity_id}, fact_id = $fact_id, entity_id = $entity_id :rm fact_entities {fact_id, entity_id}",
                params,
            ) {
                tracing::warn!(error = %e, entity_id = %id, "failed to remove fact_entity during entity delete");
            }
        }

        let merge_links = list_entity_pending_merge_links(store, entity_id.as_str())?;
        for (entity_a, entity_b) in merge_links {
            let mut params = BTreeMap::new();
            params.insert("entity_a".to_owned(), DataValue::Str(entity_a.into()));
            params.insert("entity_b".to_owned(), DataValue::Str(entity_b.into()));
            if let Err(e) = store.run_mut_query(
                r"?[entity_a, entity_b] := *pending_merges{entity_a, entity_b, name_a, name_b, name_similarity, embed_similarity, type_match, alias_overlap, merge_score, created_at}, entity_a = $entity_a, entity_b = $entity_b :rm pending_merges {entity_a, entity_b}",
                params,
            ) {
                tracing::warn!(error = %e, entity_id = %id, "failed to remove pending merge during entity delete");
            }
        }

        let mut params = BTreeMap::new();
        params.insert(
            "id".to_owned(),
            DataValue::Str(entity_id.as_str().to_owned().into()),
        );
        store
            .run_mut_query(
                r"?[id] := *entities{id}, id = $id :rm entities {id}",
                params,
            )
            .map_err(|e| ApiError::Internal {
                message: e.to_string(),
                location: snafu::location!(),
            })?;

        if let Err(e) = store.clear_entity_flags(&entity_id) {
            tracing::warn!(
                entity_id = %id,
                error = %e,
                "failed to clear entity flags during entity delete"
            );
        }

        return Ok(StatusCode::NO_CONTENT);
    }

    #[cfg(not(feature = "knowledge-store"))]
    let _ = state;

    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not enabled on this server".to_owned(),
        location: snafu::location!(),
    })
}
