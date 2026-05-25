// WHY: wire DTO
//! API error response wire shapes.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Consistent error response envelope.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    /// The error body.
    pub error: ErrorBody,
}

/// Error body returned in all error responses.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    /// Machine-readable error code (e.g. `"session_not_found"`).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
    /// Per-request correlation ID for tracing errors across logs and client reports.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Optional structured details (e.g. retry timing, validation errors).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// A single field-level validation error (#3275).
///
/// Machine-readable: consumers match on `field` + `code` without parsing
/// the English `message`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct FieldError {
    /// Request body or query parameter field name (e.g. `"nous_id"`).
    pub field: String,
    /// Stable machine-readable error code (e.g. `"required"`, `"range"`,
    /// `"format"`, `"too_long"`).
    pub code: String,
    /// Human-readable description of the error.
    pub message: String,
}
