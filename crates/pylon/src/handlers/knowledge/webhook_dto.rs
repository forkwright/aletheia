// WHY: wire DTO
//! Webhook ingestion endpoint wire shapes.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

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
    pub errors: Vec<crate::handlers::knowledge::IngestFactError>,
}
