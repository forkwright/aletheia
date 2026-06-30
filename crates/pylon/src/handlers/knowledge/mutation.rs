//! Knowledge fact mutation handlers: forget, restore, update confidence.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use symbolon::types::Role;

use crate::error::ApiError;
#[cfg(feature = "knowledge-store")]
use crate::extract::require_nous_access;
use crate::extract::{Claims, require_role};
use crate::state::KnowledgeState;

use super::{ForgetRequest, UpdateConfidenceRequest, UpdateSensitivityRequest};

#[cfg(feature = "knowledge-store")]
fn require_stored_fact_access(
    store: &std::sync::Arc<mneme::knowledge_store::KnowledgeStore>,
    claims: &Claims,
    fact_id: &mneme::id::FactId,
) -> Result<(), ApiError> {
    let facts = store
        .read_facts_by_id(fact_id.as_str())
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
            location: snafu::location!(),
        })?;
    let fact = facts.first().ok_or_else(|| ApiError::NotFound {
        path: format!("fact/{fact_id}"),
        location: snafu::location!(),
    })?;
    require_nous_access(claims, &fact.nous_id)
}

/// POST /api/v1/knowledge/facts/{id}/forget
#[utoipa::path(
    post,
    path = "/api/v1/knowledge/facts/{id}/forget",
    params(("id" = String, Path, description = "Fact ID")),
    request_body(
        content = Option<serde_json::Value>,
        description = "Optional forget reason: `{reason?}` (default: user_requested)",
        content_type = "application/json"
    ),
    responses(
        (status = 204, description = "Fact marked forgotten"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn forget_fact(
    State(state): State<KnowledgeState>,
    claims: Claims,
    Path(id): Path<String>,
    body: Option<Json<ForgetRequest>>,
) -> Result<StatusCode, ApiError> {
    require_role(&claims, Role::Operator)?;
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let fact_id = mneme::id::FactId::new(&id).map_err(|e| ApiError::BadRequest {
            message: format!("invalid fact id: {e}"),
            location: snafu::location!(),
        })?;
        require_stored_fact_access(store, &claims, &fact_id)?;
        let reason = body
            .map_or_else(super::dto::default_forget_reason, |Json(b)| b.reason)
            .parse::<mneme::knowledge::ForgetReason>()
            .unwrap_or(mneme::knowledge::ForgetReason::UserRequested);
        return match store.forget_fact_async(fact_id, reason).await {
            Ok(_) => {
                tracing::info!(operator = %claims.sub, fact_id = %id, "fact forgotten");
                Ok(StatusCode::NO_CONTENT)
            }
            Err(mneme::knowledge_error::Error::FactNotFound { .. }) => Err(ApiError::NotFound {
                path: format!("fact/{id}"),
                location: snafu::location!(),
            }),
            Err(e) => Err(ApiError::Internal {
                message: e.to_string(),
                location: snafu::location!(),
            }),
        };
    }
    #[cfg(not(feature = "knowledge-store"))]
    let _ = (state, body);
    tracing::info!(fact_id = %id, "fact forget requested but knowledge store not available");
    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not available".to_owned(),
        location: snafu::location!(),
    })
}

/// POST /api/v1/knowledge/facts/{id}/restore
#[utoipa::path(
    post,
    path = "/api/v1/knowledge/facts/{id}/restore",
    params(("id" = String, Path, description = "Fact ID")),
    responses(
        (status = 204, description = "Fact restored"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn restore_fact(
    State(state): State<KnowledgeState>,
    claims: Claims,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    require_role(&claims, Role::Operator)?;
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let fact_id = mneme::id::FactId::new(&id).map_err(|e| ApiError::BadRequest {
            message: format!("invalid fact id: {e}"),
            location: snafu::location!(),
        })?;
        require_stored_fact_access(store, &claims, &fact_id)?;
        return match store.unforget_fact_async(fact_id).await {
            Ok(_) => {
                tracing::info!(operator = %claims.sub, fact_id = %id, "fact restored");
                Ok(StatusCode::NO_CONTENT)
            }
            Err(mneme::knowledge_error::Error::FactNotFound { .. }) => Err(ApiError::NotFound {
                path: format!("fact/{id}"),
                location: snafu::location!(),
            }),
            Err(e) => Err(ApiError::Internal {
                message: e.to_string(),
                location: snafu::location!(),
            }),
        };
    }
    #[cfg(not(feature = "knowledge-store"))]
    let _ = state;
    tracing::info!(fact_id = %id, "fact restore requested but knowledge store not available");
    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not available".to_owned(),
        location: snafu::location!(),
    })
}

/// PUT /api/v1/knowledge/facts/{id}/confidence
#[utoipa::path(
    put,
    path = "/api/v1/knowledge/facts/{id}/confidence",
    params(("id" = String, Path, description = "Fact ID")),
    request_body(
        content = serde_json::Value,
        description = "Confidence value: `{confidence}` (0.0–1.0)",
        content_type = "application/json"
    ),
    responses(
        (status = 200, description = "Confidence updated"),
        (status = 400, description = "Confidence out of range", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
pub async fn update_confidence(
    State(state): State<KnowledgeState>,
    claims: Claims,
    Path(id): Path<String>,
    Json(body): Json<UpdateConfidenceRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_role(&claims, Role::Operator)?;
    if !(0.0..=1.0).contains(&body.confidence) {
        return Err(ApiError::BadRequest {
            message: "confidence must be between 0.0 and 1.0".to_string(),
            location: snafu::location!(),
        });
    }
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let fact_id = mneme::id::FactId::new(&id).map_err(|e| ApiError::BadRequest {
            message: format!("invalid fact id: {e}"),
            location: snafu::location!(),
        })?;
        require_stored_fact_access(store, &claims, &fact_id)?;
        return match store
            .update_confidence_async(fact_id, body.confidence)
            .await
        {
            Ok(_) => {
                tracing::info!(operator = %claims.sub, fact_id = %id, confidence = body.confidence, "fact confidence updated");
                Ok(Json(
                    serde_json::json!({ "status": "updated", "id": id, "confidence": body.confidence }),
                ))
            }
            Err(mneme::knowledge_error::Error::FactNotFound { .. }) => Err(ApiError::NotFound {
                path: format!("fact/{id}"),
                location: snafu::location!(),
            }),
            Err(e) => Err(ApiError::Internal {
                message: e.to_string(),
                location: snafu::location!(),
            }),
        };
    }
    #[cfg(not(feature = "knowledge-store"))]
    let _ = (state, body);
    tracing::info!(fact_id = %id, "confidence update requested but knowledge store not available");
    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not available".to_owned(),
        location: snafu::location!(),
    })
}

/// PUT /api/v1/knowledge/facts/{id}/sensitivity
///
/// Set the data-sovereignty classification on a fact so the recall pipeline
/// can gate which LLM providers receive it (#3404, #3413). Valid values:
/// `public`, `internal`, `confidential`.
#[utoipa::path(
    put,
    path = "/api/v1/knowledge/facts/{id}/sensitivity",
    params(("id" = String, Path, description = "Fact ID")),
    request_body(
        content = serde_json::Value,
        description = "Sensitivity classification: `{\"sensitivity\": \"public|internal|confidential\"}`",
        content_type = "application/json"
    ),
    responses(
        (status = 200, description = "Sensitivity updated"),
        (status = 400, description = "Invalid sensitivity value", body = crate::error::ErrorResponse),
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
pub async fn update_sensitivity(
    State(state): State<KnowledgeState>,
    claims: Claims,
    Path(id): Path<String>,
    Json(body): Json<UpdateSensitivityRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    require_role(&claims, Role::Operator)?;
    let sensitivity = body
        .sensitivity
        .parse::<mneme::knowledge::FactSensitivity>()
        .map_err(|e| ApiError::BadRequest {
            message: format!("invalid sensitivity: {e}"),
            location: snafu::location!(),
        })?;
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let fact_id = mneme::id::FactId::new(&id).map_err(|e| ApiError::BadRequest {
            message: format!("invalid fact id: {e}"),
            location: snafu::location!(),
        })?;
        require_stored_fact_access(store, &claims, &fact_id)?;
        return match store.update_sensitivity_async(fact_id, sensitivity).await {
            Ok(_) => {
                tracing::info!(
                    operator = %claims.sub,
                    fact_id = %id,
                    sensitivity = sensitivity.as_str(),
                    "fact sensitivity updated"
                );
                Ok(Json(serde_json::json!({
                    "status": "updated",
                    "id": id,
                    "sensitivity": sensitivity.as_str(),
                })))
            }
            Err(mneme::knowledge_error::Error::FactNotFound { .. }) => Err(ApiError::NotFound {
                path: format!("fact/{id}"),
                location: snafu::location!(),
            }),
            Err(e) => Err(ApiError::Internal {
                message: e.to_string(),
                location: snafu::location!(),
            }),
        };
    }
    #[cfg(not(feature = "knowledge-store"))]
    let _ = (state, sensitivity);
    tracing::info!(fact_id = %id, "sensitivity update requested but knowledge store not available");
    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not available".to_owned(),
        location: snafu::location!(),
    })
}
