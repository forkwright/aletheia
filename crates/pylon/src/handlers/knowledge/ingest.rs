//! Programmatic knowledge ingestion handler.

use axum::Json;
use axum::extract::State;
use tracing::instrument;

use symbolon::types::Role;

use crate::error::{ApiError, BadRequestSnafu, ValidationFailedSnafu};
use crate::extract::{Claims, require_role};
use crate::state::KnowledgeState;

#[path = "ingest_dto.rs"]
mod ingest_dto;
pub use ingest_dto::{IngestFactError, IngestRequest, IngestResponse};

/// POST /api/v1/knowledge/ingest
///
/// Ingest raw content into the knowledge store. Content is chunked and
/// heuristic-extracted for markdown/plain text, or parsed directly for
/// JSON/JSONL.
#[utoipa::path(
    post,
    path = "/api/v1/knowledge/ingest",
    request_body(
        content = IngestRequest,
        description = "Content to ingest with format and target agent",
        content_type = "application/json"
    ),
    responses(
        (status = 200, description = "Ingestion summary"),
        (status = 400, description = "Invalid format or malformed request", body = crate::error::ErrorResponse),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorResponse),
        (status = 422, description = "Validation failed", body = crate::error::ErrorResponse),
        (status = 503, description = "Knowledge store not available", body = crate::error::ErrorResponse),
    ),
    security(("bearer_auth" = []))
)]
///
/// # Cancel safety
///
/// Cancel-safe. Axum handler; cancellation drops the future with no
/// side effects beyond not returning a response.
#[instrument(skip_all, fields(content_len = body.content.len()))]
pub async fn ingest(
    State(state): State<KnowledgeState>,
    claims: Claims,
    Json(body): Json<IngestRequest>,
) -> Result<Json<IngestResponse>, ApiError> {
    require_role(&claims, Role::Operator)?;

    if body.content.is_empty() {
        return Err(BadRequestSnafu {
            message: "content must not be empty".to_owned(),
        }
        .build());
    }

    if body.nous_id.is_empty() {
        return Err(ValidationFailedSnafu {
            errors: vec![crate::error::FieldError {
                field: "nous_id".to_owned(),
                code: "required".to_owned(),
                message: "must not be empty".to_owned(),
            }],
        }
        .build());
    }

    let format = parse_ingest_format(&body.format)?;
    let config = mneme::ingest::IngestConfig::default();
    let facts = tokio::task::spawn_blocking(move || {
        mneme::ingest::ingest_content(&body.content, format, &config, &body.nous_id)
    })
    .await
    .map_err(|e| ApiError::Internal {
        message: format!("ingest task failed: {e}"),
        location: snafu::location!(),
    })?
    .map_err(|e| ApiError::BadRequest {
        message: e.to_string(),
        location: snafu::location!(),
    })?;
    #[cfg(not(feature = "knowledge-store"))]
    let _ = &facts;

    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let store = std::sync::Arc::clone(store);
        let (result, fact_events) = tokio::task::spawn_blocking(move || {
            let mut inserted = 0usize;
            let mut skipped = 0usize;
            let mut errors = Vec::new();
            let mut fact_events: Vec<(String, String, String)> = Vec::new();

            for (index, fact) in facts.iter().enumerate() {
                match store.insert_fact(fact) {
                    Ok(()) => {
                        inserted += 1;
                        fact_events.push((
                            fact.id.as_str().to_owned(),
                            fact.nous_id.as_str().to_owned(),
                            truncate(&fact.content, 200),
                        ));
                    }
                    Err(e) => {
                        errors.push(IngestFactError {
                            index,
                            id: Some(fact.id.as_str().to_owned()),
                            message: e.to_string(),
                        });
                        skipped += 1;
                    }
                }
            }

            (
                IngestResponse {
                    inserted,
                    skipped,
                    errors,
                },
                fact_events,
            )
        })
        .await
        .map_err(|e| ApiError::Internal {
            message: format!("ingest write task failed: {e}"),
            location: snafu::location!(),
        })?;

        for (fact_id, nous_id, content_preview) in fact_events {
            state
                .event_bus
                .publish(crate::event_bus::DomainEvent::new(
                    state.event_bus.next_id(),
                    "fact.created",
                    serde_json::json!({
                        "fact_id": fact_id,
                        "nous_id": nous_id,
                        "content_preview": content_preview,
                    }),
                ))
                .await;
        }

        tracing::info!(
            inserted = result.inserted,
            skipped = result.skipped,
            "knowledge ingestion complete"
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

fn parse_ingest_format(format: &str) -> Result<mneme::ingest::IngestFormat, ApiError> {
    if format.is_empty() {
        return Ok(mneme::ingest::IngestFormat::PlainText);
    }

    mneme::ingest::parse_format(format).ok_or_else(|| {
        BadRequestSnafu {
            message: format!(
                "unsupported format '{format}': valid values are markdown, text, json, jsonl"
            ),
        }
        .build()
    })
}

/// Truncate a string to `max_len` bytes, adding an ellipsis if truncated.
#[cfg(feature = "knowledge-store")]
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_owned()
    } else {
        let mut result = String::with_capacity(max_len + 3);
        for ch in s.chars() {
            if result.len() + ch.len_utf8() > max_len.saturating_sub(1) {
                result.push('…');
                break;
            }
            result.push(ch);
        }
        result
    }
}
