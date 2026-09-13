// WHY: wire DTO
//! Programmatic ingestion endpoint wire shapes.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Request body for knowledge ingestion.
// WHY(#5100): also `Serialize` so `pylon::client::GatewayClient` can build this
// request body directly instead of restating its field shape in a client-side
// literal (the request/response wire shape is defined once, here).
#[derive(Debug, Serialize, Deserialize, ToSchema)]
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
// WHY(#5100): also `Deserialize` so `pylon::client::GatewayClient::ingest()` can
// decode the response it receives, matching the pattern already used by
// `HealthResponse`/`SessionResponse` for client-consumed server DTOs.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct IngestFactError {
    /// Index of the fact in the batch.
    pub index: usize,
    /// Fact ID if available.
    pub id: Option<String>,
    /// Error message.
    pub message: String,
}

/// Response for knowledge ingestion.
// WHY(#5100): also `Deserialize` — see `IngestFactError` above.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct IngestResponse {
    /// Number of facts successfully inserted.
    pub inserted: usize,
    /// Number of facts skipped due to errors.
    pub skipped: usize,
    /// Per-fact error details.
    pub errors: Vec<IngestFactError>,
}
