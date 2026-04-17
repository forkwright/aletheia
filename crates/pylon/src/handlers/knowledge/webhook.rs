//! Webhook listener for real-time knowledge ingestion from external sources.

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};
use tracing::instrument;
use utoipa::ToSchema;

use symbolon::types::Role;

use crate::error::{ApiError, BadRequestSnafu};
use crate::extract::{Claims, require_role};
use crate::state::KnowledgeState;

/// Request body for webhook ingestion.
///
/// Accepts a single fact or a batch of facts from external systems
/// (Slack, email, wiki, etc.).
#[derive(Debug, Deserialize)]
pub struct WebhookIngestRequest {
    /// Nous agent ID that will own the facts.
    pub nous_id: String,
    /// Facts to insert.
    pub facts: Vec<mneme::knowledge::Fact>,
    /// Optional source identifier for provenance.
    #[serde(default)]
    pub source: Option<String>,
}

/// Response for webhook ingestion.
#[derive(Debug, Serialize, ToSchema)]
pub struct WebhookIngestResponse {
    /// Number of facts successfully inserted.
    pub inserted: usize,
    /// Number of facts skipped due to errors.
    pub skipped: usize,
    /// Per-fact error details.
    pub errors: Vec<crate::handlers::knowledge::ingest::IngestFactError>,
}

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
