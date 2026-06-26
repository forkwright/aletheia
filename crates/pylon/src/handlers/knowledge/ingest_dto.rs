// WHY: wire DTO
//! Programmatic ingestion endpoint wire shapes.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Request body for knowledge ingestion.
#[derive(Debug, Deserialize, ToSchema)]
pub struct IngestRequest {
    /// Raw content to ingest.
    pub content: String,
    /// Format: markdown, text, json, jsonl.
    #[serde(default)]
    pub format: String,
    /// Nous agent ID that will own the extracted facts. Scoped tokens may omit
    /// this field to use their token-bound agent.
    #[serde(default)]
    pub nous_id: String,
}

/// Per-fact error during ingestion.
#[derive(Debug, Serialize, ToSchema)]
pub struct IngestFactError {
    /// Index of the fact in the batch.
    pub index: usize,
    /// Fact ID if available.
    pub id: Option<String>,
    /// Error message.
    pub message: String,
}

/// Response for knowledge ingestion.
#[derive(Debug, Serialize, ToSchema)]
pub struct IngestResponse {
    /// Number of facts successfully inserted.
    pub inserted: usize,
    /// Number of facts skipped due to errors.
    pub skipped: usize,
    /// Per-fact error details.
    pub errors: Vec<IngestFactError>,
}
