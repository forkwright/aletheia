//! Knowledge fact mutation handlers: forget, restore, update confidence.

use axum::Json;
use axum::extract::{Path, State};

use crate::error::ApiError;
use crate::state::KnowledgeState;

use super::{ForgetRequest, UpdateConfidenceRequest};

/// POST /api/v1/knowledge/facts/{id}/forget
#[utoipa::path(
    post,
    path = "/api/v1/knowledge/facts/{id}/forget",
    params(("id" = String, Path, description = "Fact ID")),
    request_body(
        content = serde_json::Value,
        description = "Optional forget reason: `{reason?}` (default: user_requested)",
        content_type = "application/json"
    ),
    responses(
        (status = 200, description = "Fact marked forgotten"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn forget_fact(
    State(state): State<KnowledgeState>,
    Path(id): Path<String>,
    Json(body): Json<ForgetRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let fact_id = aletheia_mneme::id::FactId::new(&id).map_err(|e| ApiError::BadRequest {
            message: format!("invalid fact id: {e}"),
            location: snafu::location!(),
        })?;
        let reason = body
            .reason
            .parse::<aletheia_mneme::knowledge::ForgetReason>()
            .unwrap_or(aletheia_mneme::knowledge::ForgetReason::UserRequested);
        return match store.forget_fact_async(fact_id, reason).await {
            Ok(_) => {
                tracing::info!(fact_id = %id, "fact forgotten");
                Ok(Json(serde_json::json!({ "status": "forgotten", "id": id })))
            }
            Err(aletheia_mneme::error::Error::FactNotFound { .. }) => Err(ApiError::NotFound {
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
        (status = 200, description = "Fact restored"),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
pub async fn restore_fact(
    State(state): State<KnowledgeState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let fact_id = aletheia_mneme::id::FactId::new(&id).map_err(|e| ApiError::BadRequest {
            message: format!("invalid fact id: {e}"),
            location: snafu::location!(),
        })?;
        return match store.unforget_fact_async(fact_id).await {
            Ok(_) => {
                tracing::info!(fact_id = %id, "fact restored");
                Ok(Json(serde_json::json!({ "status": "restored", "id": id })))
            }
            Err(aletheia_mneme::error::Error::FactNotFound { .. }) => Err(ApiError::NotFound {
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
pub async fn update_confidence(
    State(state): State<KnowledgeState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateConfidenceRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if !(0.0..=1.0).contains(&body.confidence) {
        return Err(ApiError::BadRequest {
            message: "confidence must be between 0.0 and 1.0".to_string(),
            location: snafu::location!(),
        });
    }
    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let fact_id = aletheia_mneme::id::FactId::new(&id).map_err(|e| ApiError::BadRequest {
            message: format!("invalid fact id: {e}"),
            location: snafu::location!(),
        })?;
        return match store
            .update_confidence_async(fact_id, body.confidence)
            .await
        {
            Ok(_) => {
                tracing::info!(fact_id = %id, confidence = body.confidence, "fact confidence updated");
                Ok(Json(
                    serde_json::json!({ "status": "updated", "id": id, "confidence": body.confidence }),
                ))
            }
            Err(aletheia_mneme::error::Error::FactNotFound { .. }) => Err(ApiError::NotFound {
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
