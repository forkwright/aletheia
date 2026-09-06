//! Workspace file-browser DTOs mirroring Pylon's wire shapes
//! (`crates/pylon/src/handlers/workspace_dto.rs`). Skene has no dependency on
//! pylon, so these are independent structs kept in sync by the contract
//! tests in `super::tests`.

use serde::{Deserialize, Serialize};

/// A single file or directory entry in the workspace browser.
#[derive(Debug, Clone, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's FileEntry; self-documenting by name"
)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

/// A single normalized git-status entry for the workspace browser.
#[derive(Debug, Clone, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's GitStatusEntry; self-documenting by name"
)]
pub struct GitStatusEntry {
    pub path: String,
    pub status: String,
}

/// Search result row for the workspace search box.
///
/// WHY: named `WorkspaceSearchResult` rather than `SearchResult` to stay
/// distinct from the knowledge-domain search result row of the same shape
/// name.
#[derive(Debug, Clone, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's SearchResult; self-documenting by name"
)]
pub struct WorkspaceSearchResult {
    pub path: String,
    pub line: usize,
    pub snippet: String,
}

/// Request body for writing file content back to the workspace vault.
#[derive(Debug, Clone, Serialize)]
pub struct WriteContentRequest {
    /// Workspace-relative file path to write.
    pub path: String,
    /// New file content, UTF-8 text.
    pub content: String,
    /// Optional optimistic-concurrency guard: the last-known mtime in
    /// milliseconds since the Unix epoch. When present and the on-disk mtime
    /// differs, the write is rejected with 409 so a concurrent edit is not
    /// clobbered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub if_match_mtime_ms: Option<i64>,
}

/// Response returned after a successful workspace file write.
#[derive(Debug, Clone, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's WriteContentResponse; self-documenting by name"
)]
pub struct WriteContentResponse {
    pub path: String,
    pub size: u64,
    pub mtime_ms: i64,
}

/// Response returned after dispatching a workspace file to the default app.
#[derive(Debug, Clone, Deserialize)]
#[expect(
    missing_docs,
    reason = "fields mirror pylon's OpenResponse; self-documenting by name"
)]
pub struct OpenFileResponse {
    pub ok: bool,
    pub path: String,
}
