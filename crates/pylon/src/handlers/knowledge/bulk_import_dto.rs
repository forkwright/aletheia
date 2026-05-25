// WHY: wire DTO
//! Bulk import endpoint wire shapes.

use serde::{Deserialize, Serialize};

/// Request body for bulk fact import.
#[derive(Debug, Deserialize)]
pub struct BulkImportRequest {
    pub facts: Vec<mneme::knowledge::Fact>,
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
