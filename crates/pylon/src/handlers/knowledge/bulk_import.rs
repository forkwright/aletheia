//! Bulk fact import handler.

use axum::Json;
use axum::extract::State;
use tracing::instrument;

use symbolon::types::Role;

use crate::error::{ApiError, BadRequestSnafu};
use crate::extract::{Claims, require_role};
use crate::state::KnowledgeState;

#[path = "bulk_import_dto.rs"]
mod bulk_import_dto;
// WHY: ImportFactError is re-exported for API consumers; rustc flags it unused
// because bulk_import.rs uses it internally via bulk_import_dto:: path, not via
// the re-export itself. The pub use is intentional for the public pylon surface.
// kanon:ignore RUST/allow-not-expect WHY: unused_imports fires only under some target/cfg combinations (rustc sees ImportFactError as unused via the re-export, but --all-targets clippy sees it used through the bulk_import_dto:: path), so #[expect] would be unfulfilled and itself warn; #[allow] is the correct tool for a conditionally-unused re-export.
// kanon:ignore RUST/prefer-expect-over-allow WHY: same as RUST/allow-not-expect above — kanon emits the rule under both legacy and current names depending on basanos build; suppressing both keeps gate green across versions.
#[allow(unused_imports)]
pub use bulk_import_dto::{BulkImportRequest, BulkImportResponse, ImportFactError};

/// Parse a request body as either JSON (`{ "facts": [...] }`) or JSONL.
fn parse_import_body(bytes: &[u8]) -> Result<Vec<mneme::knowledge::Fact>, ApiError> {
    // First, try as structured JSON object.
    if let Ok(req) = serde_json::from_slice::<BulkImportRequest>(bytes) {
        return Ok(req.facts);
    }

    // Fall back to JSONL: one fact per line.
    let text = std::str::from_utf8(bytes).map_err(|e| {
        BadRequestSnafu {
            message: format!("body is not valid UTF-8: {e}"),
        }
        .build()
    })?;

    let mut facts = Vec::new();
    for (line_no, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fact: mneme::knowledge::Fact = serde_json::from_str(line).map_err(|e| {
            BadRequestSnafu {
                message: format!("JSONL parse error on line {}: {e}", line_no + 1),
            }
            .build()
        })?;
        facts.push(fact);
    }

    Ok(facts)
}

/// POST /api/v1/knowledge/facts/import
///
/// Bulk-import facts into the knowledge store. The per-request limit is
/// controlled by `api_limits.max_import_batch_size` (default 1000). Each fact
/// is validated independently; valid facts are inserted even if others fail.
///
/// Supports two body formats:
/// - **JSON**: `{"facts": [{...}, {...}]}`
/// - **JSONL**: one fact object per line
#[utoipa::path(
    post,
    path = "/api/v1/knowledge/facts/import",
    request_body(
        content = serde_json::Value,
        description = "JSON object with a `facts` array, or JSONL (one fact per line). The limit is `api_limits.max_import_batch_size`; the default is 1000.",
        content_type = "application/json"
    ),
    responses(
        (status = 200, description = "Import summary with per-fact error details"),
        (status = 400, description = "Batch too large or malformed request", body = crate::error::ErrorResponse),
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
#[instrument(skip_all)]
pub async fn import_facts(
    State(state): State<KnowledgeState>,
    claims: Claims,
    body: axum::body::Bytes,
) -> Result<Json<BulkImportResponse>, ApiError> {
    require_role(&claims, Role::Operator)?;

    let facts = parse_import_body(&body)?;
    let max_batch = state.config.read().await.api_limits.max_import_batch_size;
    if facts.len() > max_batch {
        return Err(ApiError::BadRequest {
            message: format!("batch size {} exceeds maximum of {max_batch}", facts.len()),
            location: snafu::location!(),
        });
    }
    super::require_facts_nous_access(&claims, facts.iter())?;

    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let store = std::sync::Arc::clone(store);
        let result = tokio::task::spawn_blocking(move || {
            let mut imported = 0usize;
            let mut skipped = 0usize;
            let mut errors = Vec::new();

            for (index, fact) in facts.iter().enumerate() {
                match store.insert_fact(fact) {
                    Ok(()) => imported += 1,
                    Err(e) => {
                        // WHY: validation errors (empty content, bad confidence) are
                        // per-fact problems reported in the response, not request-level failures.
                        errors.push(ImportFactError {
                            index,
                            id: fact.id.as_str().to_owned(),
                            message: e.to_string(),
                        });
                        skipped += 1;
                    }
                }
            }

            BulkImportResponse {
                imported,
                skipped,
                errors,
            }
        })
        .await
        .map_err(|e| ApiError::Internal {
            message: format!("bulk import task failed: {e}"),
            location: snafu::location!(),
        })?;

        tracing::info!(
            operator = %claims.sub,
            imported = result.imported,
            skipped = result.skipped,
            "bulk fact import complete"
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

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test assertions — panics are acceptable in test context"
)]
mod tests {
    use super::*;

    #[test]
    fn bulk_import_request_deserializes_empty_facts() {
        let json = r#"{"facts": []}"#;
        let req: BulkImportRequest = serde_json::from_str(json).unwrap();
        assert!(req.facts.is_empty());
    }

    #[test]
    fn bulk_import_response_serializes() {
        let resp = BulkImportResponse {
            imported: 5,
            skipped: 1,
            errors: vec![ImportFactError {
                index: 3,
                id: "fact-bad".to_owned(),
                message: "empty content".to_owned(),
            }],
        };
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["imported"], 5);
        assert_eq!(json["skipped"], 1);
        assert_eq!(json["errors"][0]["index"], 3);
        assert_eq!(json["errors"][0]["id"], "fact-bad");
    }

    #[test]
    fn default_max_batch_size_is_1000() {
        let config = taxis::config::ApiLimitsConfig::default();
        assert_eq!(config.max_import_batch_size, 1000);
    }
}
