//! Type definitions for the operations pane state.

/// Which pane currently has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FocusedPane {
    #[default]
    Chat,
    Operations,
}

/// Status of a tool call in the operations pane.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OpsToolStatus {
    Running,
    Complete,
    Failed,
}

/// Tool category for grouping calls by type in the ops pane KPI display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub(crate) enum ToolCategory {
    /// File reads: read_file, glob, grep, etc.
    Read,
    /// File writes: write_file, edit_file, notebook_edit, etc.
    Write,
    /// Search operations: web_search, search, etc.
    Search,
    /// Shell execution: bash, exec, etc.
    Exec,
    /// HTTP operations: web_fetch, http, etc.
    Http,
    /// Uncategorized tools.
    Other,
}

impl std::fmt::Display for ToolCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read => write!(f, "read"),
            Self::Write => write!(f, "write"),
            Self::Search => write!(f, "search"),
            Self::Exec => write!(f, "exec"),
            Self::Http => write!(f, "http"),
            Self::Other => write!(f, "other"),
        }
    }
}

/// Auto-show behavior configuration.
///
/// Additional variants (`Always`, `Manual`) will be added when config wiring lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum OpsAutoShow {
    /// Show automatically when streaming starts, collapse when idle
    #[default]
    Auto,
}

/// A single tool call entry in the operations pane.
#[derive(Debug, Clone)]
pub(crate) struct OpsToolCall {
    pub(crate) name: String,
    pub(crate) input_json: Option<String>,
    pub(crate) output: Option<String>,
    pub(crate) status: OpsToolStatus,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) expanded: bool,
    /// Primary argument extracted from input (path, command, pattern, etc.)
    pub(crate) primary_arg: Option<String>,
    /// Error summary for failed tool calls, extracted from result text.
    pub(crate) error_message: Option<String>,
    /// Tool category for KPI grouping.
    pub(crate) category: ToolCategory,
}

/// A single thinking block in the operations pane.
#[derive(Debug, Clone)]
pub(crate) struct OpsThinkingBlock {
    pub(crate) text: String,
    pub(crate) collapsed: bool,
}

/// A file diff entry parsed from tool results.
#[derive(Debug, Clone)]
pub(crate) struct OpsDiffEntry {
    pub(crate) file_path: String,
    pub(crate) additions: Vec<String>,
    pub(crate) deletions: Vec<String>,
}
