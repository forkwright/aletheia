//! Programmatic knowledge ingestion handler.

use axum::Json;
use axum::extract::State;
use tracing::instrument;

use symbolon::types::Role;

use crate::error::{ApiError, BadRequestSnafu, ValidationFailedSnafu};
use crate::extract::{Claims, require_nous_access, require_role};
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

    let IngestRequest {
        content,
        format,
        nous_id,
    } = body;

    if content.is_empty() {
        return Err(BadRequestSnafu {
            message: "content must not be empty".to_owned(),
        }
        .build());
    }

    let target_nous_id = resolve_ingest_target(&claims, &nous_id)?;
    let format = parse_ingest_format(&format)?;
    let config = mneme::ingest::IngestConfig::default();
    let target_for_ingest = target_nous_id.clone();
    let facts = tokio::task::spawn_blocking(move || {
        mneme::ingest::ingest_content(&content, format, &config, &target_for_ingest)
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
    super::require_facts_nous_access(&claims, facts.iter())?;
    super::require_facts_match_target(facts.iter(), &target_nous_id)?;

    #[cfg(not(feature = "knowledge-store"))]
    let _ = &facts;

    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let store = std::sync::Arc::clone(store);
        let (result, fact_events) =
            tokio::task::spawn_blocking(move || insert_facts(&store, &facts))
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
            operator = %claims.sub,
            target_nous_id = %target_nous_id,
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

fn resolve_ingest_target(claims: &Claims, requested_nous_id: &str) -> Result<String, ApiError> {
    let requested_nous_id = requested_nous_id.trim();
    if requested_nous_id.is_empty() {
        if let Some(scoped_nous_id) = &claims.nous_id {
            return Ok(scoped_nous_id.clone());
        }
        return Err(ValidationFailedSnafu {
            errors: vec![crate::error::FieldError {
                field: "nous_id".to_owned(),
                code: "required".to_owned(),
                message: "must not be empty".to_owned(),
            }],
        }
        .build());
    }

    require_nous_access(claims, requested_nous_id)?;
    Ok(requested_nous_id.to_owned())
}

/// Insert `facts` into `store`, returning the response summary plus the
/// `(fact_id, nous_id, content_preview)` tuples to publish as `fact.created`
/// events after the blocking write completes.
///
/// WHY: event publication is async; it must happen on the runtime, not inside
/// the `spawn_blocking` task. This helper keeps the blocking write self-contained
/// and the async [`ingest`] handler under the line limit.
#[cfg(feature = "knowledge-store")]
fn insert_facts(
    store: &mneme::knowledge_store::KnowledgeStore,
    facts: &[mneme::knowledge::Fact],
) -> (IngestResponse, Vec<(String, String, String)>) {
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
