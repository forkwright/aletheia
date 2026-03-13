//! Error types for the organon crate.

use aletheia_koina::id::ToolName;
use snafu::Snafu;

/// Errors from tool registry operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum Error {
    /// Requested tool does not exist in the registry.
    #[snafu(display("tool not found: {name}"))]
    ToolNotFound {
        name: ToolName,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A tool with this name is already registered.
    #[snafu(display("duplicate tool: {name}"))]
    DuplicateTool {
        name: ToolName,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Tool input failed validation.
    #[snafu(display("invalid input for tool {name}: {reason}"))]
    InvalidInput {
        name: ToolName,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Tool execution returned an error.
    #[snafu(display("tool execution failed: {name}: {message}"))]
    ExecutionFailed {
        name: ToolName,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Failed to serialize an input schema to JSON.
    #[snafu(display("schema serialization failed"))]
    SchemaSerialization {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Error from store operations (`NoteStore` / `BlackboardStore` adapters).
///
/// Uses a message string so implementations can convert any underlying error
/// without introducing crate-level type dependencies on adapters.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum StoreError {
    /// A store operation failed.
    #[snafu(display("{message}"))]
    Store { message: String },
}
