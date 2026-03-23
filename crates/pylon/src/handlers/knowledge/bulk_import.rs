//! Bulk fact import handler.

use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::state::KnowledgeState;

/// Hard cap on facts per import request.
const MAX_IMPORT_BATCH_SIZE: usize = 1000;

/// Request body for bulk fact import.
#[derive(Debug, Deserialize)]
pub struct BulkImportRequest {
    pub facts: Vec<aletheia_mneme::knowledge::Fact>,
}

/// Summary response for bulk fact import.
#[derive(Debug, Serialize)]
pub struct BulkImportResponse {
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<ImportFactError>,
}

/// Per-fact error detail.
#[derive(Debug, Serialize)]
pub struct ImportFactError {
    pub index: usize,
    pub id: String,
    pub message: String,
}

/// POST /api/v1/knowledge/facts/import
///
/// Bulk-import facts into the knowledge store. Accepts up to 1000 facts per
/// request. Each fact is validated independently; valid facts are inserted
/// even if others fail.
#[utoipa::path(
    post,
    path = "/api/v1/knowledge/facts/import",
    request_body(
        content = serde_json::Value,
        description = "JSON object with a `facts` array of Fact objects (max 1000)",
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
pub async fn import_facts(
    State(state): State<KnowledgeState>,
    Json(body): Json<BulkImportRequest>,
) -> Result<Json<BulkImportResponse>, ApiError> {
    if body.facts.len() > MAX_IMPORT_BATCH_SIZE {
        return Err(ApiError::BadRequest {
            message: format!(
                "batch size {} exceeds maximum of {MAX_IMPORT_BATCH_SIZE}",
                body.facts.len()
            ),
            location: snafu::location!(),
        });
    }

    #[cfg(feature = "knowledge-store")]
    if let Some(ref store) = state.knowledge_store {
        let store = std::sync::Arc::clone(store);
        let facts = body.facts;
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
#[expect(clippy::unwrap_used, reason = "test assertions")]
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
    fn max_batch_size_is_1000() {
        assert_eq!(MAX_IMPORT_BATCH_SIZE, 1000);
    }
}
