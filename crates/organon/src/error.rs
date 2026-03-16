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

/// Error from `PlanningService` adapter operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum PlanningAdapterError {
    /// Workspace create/open/save/load failure.
    #[snafu(display("workspace operation failed"))]
    Workspace {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Project state transition rejected.
    #[snafu(display("invalid project transition"))]
    InvalidTransition {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON serialization failure.
    #[snafu(display("failed to serialize project"))]
    Serialize {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Background task join failure.
    #[snafu(display("background task failed"))]
    Join {
        source: tokio::task::JoinError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Invalid input (bad mode, unknown transition name, invalid ID, entity not found).
    #[snafu(display("{message}"))]
    InvalidInput {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Error from `KnowledgeSearchService` adapter operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum KnowledgeAdapterError {
    /// Embedding generation failure.
    #[snafu(display("embedding failed"))]
    Embedding {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Hybrid search failure.
    #[snafu(display("knowledge search failed"))]
    Search {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Fact or datalog query failure.
    #[snafu(display("knowledge query failed"))]
    Query {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Fact mutation failure (insert, supersede, retract, forget, unforget).
    #[snafu(display("fact mutation failed"))]
    Mutation {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Invalid input (unparseable reason, etc.).
    #[snafu(display("{message}"))]
    InvalidInput {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
