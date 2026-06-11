//! Workspace file-browser wire shapes.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Query parameters for listing workspace files.
#[derive(Debug, Deserialize, ToSchema)]
pub struct FilesQuery {
    /// Directory path relative to the workspace root.
    #[serde(default)]
    pub path: Option<String>,
}

/// A single file or directory entry in the workspace browser.
#[derive(Debug, Serialize, ToSchema)]
pub struct FileEntry {
    /// Basename for the entry.
    pub name: String,
    /// Workspace-relative path using forward slashes.
    pub path: String,
    /// Whether this entry is a directory.
    pub is_dir: bool,
    /// File size in bytes, or `0` for directories.
    pub size: u64,
}

/// A single normalized git-status entry for the workspace browser.
#[derive(Debug, Serialize, ToSchema)]
pub struct GitStatusEntry {
    /// Workspace-relative path using forward slashes.
    pub path: String,
    /// Status code normalized for the desktop file tree (`M`, `A`, `D`, `?`).
    pub status: String,
}

/// Query parameters for reading raw file content.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ContentQuery {
    /// Workspace-relative file path.
    pub path: String,
}

/// Query parameters for reading a git diff.
#[derive(Debug, Deserialize, ToSchema)]
pub struct DiffQuery {
    /// Workspace-relative path passed to `git diff -- <path>`.
    pub path: String,
}

/// Query parameters for the workspace search endpoint.
#[derive(Debug, Deserialize, ToSchema)]
pub struct SearchQuery {
    /// Case-insensitive filename/content query.
    pub q: String,
    /// Maximum number of results to return.
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

/// Search result row for the file browser search box.
#[derive(Debug, Serialize, ToSchema)]
pub struct SearchResult {
    /// Workspace-relative path using forward slashes.
    pub path: String,
    /// 1-based line number when the match came from file contents.
    pub line: usize,
    /// Match snippet or filename preview.
    pub snippet: String,
}

pub(crate) fn default_search_limit() -> usize {
    50
}
