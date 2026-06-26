//! Webhook listener for real-time knowledge ingestion from external sources.

use axum::Json;
use axum::extract::State;
use tracing::instrument;

use symbolon::types::Role;

use crate::error::{ApiError, BadRequestSnafu};
use crate::extract::{Claims, require_nous_access, require_role};
use crate::state::KnowledgeState;

#[path = "webhook_dto.rs"]
mod webhook_dto;
pub use webhook_dto::{WebhookIngestRequest, WebhookIngestResponse};

/// POST /api/v1/knowledge/ingest/webhook
///
/// Real-time ingestion endpoint for external data sources. Each fact is
/// validated independently; valid facts are inserted even if others fail.
#[utoipa::path(
    post,
    path = "/api/v1/knowledge/ingest/webhook",
    request_body(
        content = serde_json::Value,
        description = "Batch of facts from an external source ({ nous_id, facts: [...], source? })",
        content_type = "application/json"
    ),
    responses(
        (status = 200, description = "Ingestion summary"),
        (status = 400, description = "Malformed request", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 503, description = "Knowledge store not available", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[instrument(skip_all, fields(count = body.facts.len()))]
pub async fn webhook_ingest(
    State(state): State<KnowledgeState>,
    claims: Claims,
    Json(body): Json<WebhookIngestRequest>,
) -> Result<Json<WebhookIngestResponse>, ApiError> {
    require_role(&claims, Role::Operator)?;

    let target_nous_id = body.nous_id.trim();
    if target_nous_id.is_empty() {
        return Err(BadRequestSnafu {
            message: "nous_id must not be empty".to_owned(),
        }
        .build());
    }
    require_nous_access(&claims, target_nous_id)?;
    super::require_facts_nous_access(&claims, body.facts.iter())?;
    super::require_facts_match_target(body.facts.iter(), target_nous_id)?;

    let max_batch = state.config.read().await.api_limits.max_import_batch_size;
    if body.facts.len() > max_batch {
        return Err(BadRequestSnafu {
            message: format!(
                "batch size {} exceeds maximum of {max_batch}",
                body.facts.len()
            ),
        }
        .build());
    }

    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let store = std::sync::Arc::clone(store);
        let target_nous_id = target_nous_id.to_owned();
        let facts = body.facts;
        let result = tokio::task::spawn_blocking(move || {
            let mut inserted = 0usize;
            let mut skipped = 0usize;
            let mut errors = Vec::new();

            for (index, fact) in facts.iter().enumerate() {
                match store.insert_fact(fact) {
                    Ok(()) => inserted += 1,
                    Err(e) => {
                        errors.push(crate::handlers::knowledge::ingest::IngestFactError {
                            index,
                            id: Some(fact.id.as_str().to_owned()),
                            message: e.to_string(),
                        });
                        skipped += 1;
                    }
                }
            }

            WebhookIngestResponse {
                inserted,
                skipped,
                errors,
            }
        })
        .await
        .map_err(|e| ApiError::Internal {
            message: format!("webhook ingest task failed: {e}"),
            location: snafu::location!(),
        })?;

        tracing::info!(
            operator = %claims.sub,
            target_nous_id = %target_nous_id,
            inserted = result.inserted,
            skipped = result.skipped,
            "webhook ingestion complete"
        );
        return Ok(Json(result));
    }

    #[cfg(not(feature = "knowledge-store"))]
    let _ = state;

    Err(ApiError::ServiceUnavailable {
        message: "knowledge store not available".to_owned(),
        location: snafu::location!(),
    })
}
