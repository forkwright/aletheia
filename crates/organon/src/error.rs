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

/// Typed errors for `PlanningService` adapter implementations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum PlanningAdapterError {
    #[snafu(display("failed to access workspace: {source}"))]
    Workspace {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("failed to load project: {source}"))]
    LoadProject {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("failed to save project: {source}"))]
    SaveProject {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("failed to serialize project: {source}"))]
    Serialize {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("state transition failed: {source}"))]
    Transition {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("unknown project mode: {mode}"))]
    InvalidMode {
        mode: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("unknown transition: {name}"))]
    InvalidTransition {
        name: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("invalid {kind}: {source}"))]
    InvalidId {
        kind: String,
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("{kind} not found: {id}"))]
    NotFound {
        kind: String,
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("background task panicked: {source}"))]
    TaskJoin {
        source: tokio::task::JoinError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("filesystem error: {source}"))]
    Io {
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Typed errors for `KnowledgeSearchService` adapter implementations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum KnowledgeAdapterError {
    #[snafu(display("embedding failed: {source}"))]
    Embedding {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("search failed: {source}"))]
    Search {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("fact query failed: {source}"))]
    FactQuery {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("store mutation failed: {source}"))]
    MutateStore {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("datalog query failed: {source}"))]
    DatalogQuery {
        source: Box<dyn std::error::Error + Send + Sync>,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("invalid forget reason: {reason}"))]
    InvalidReason {
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
