//! Error types for the organon crate.

use snafu::Snafu;

use koina::id::ToolName;

/// Errors from tool registry operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (name, source, location, reason, message, path) are self-documenting via display format"
)]
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

    /// File-ref interpolation failed during tool argument expansion.
    #[snafu(display("file-ref interpolation failed: {source}"))]
    InterpError {
        source: crate::interp::InterpError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>; // kanon:ignore RUST/pub-visibility

/// Error from store operations (`NoteStore` / `BlackboardStore` adapters).
///
/// WHY: Structured variants preserve failure mode (not-found vs conflict vs I/O)
/// so callers can pattern-match on specific failures without string-parsing.
/// Decoupled from backend types to avoid circular dependencies. Closes #3286.
///
/// Variant names are prefixed with `Store` to avoid snafu context-selector
/// collisions with `PlanningAdapterError` variants in the same module.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (entity, id, context, source, message) are self-documenting via display format"
)]
#[non_exhaustive]
pub enum StoreError {
    // kanon:ignore RUST/pub-visibility
    /// The requested entity was not found.
    #[snafu(display("{entity} not found: {id}"))]
    StoreNotFound { entity: String, id: String },

    /// A conflicting entry already exists.
    #[snafu(display("{entity} conflict: {id}"))]
    StoreConflict { entity: String, id: String },

    /// An I/O error occurred during a store operation.
    #[snafu(display("store I/O error: {context}"))]
    StoreIo {
        context: String,
        source: std::io::Error,
    },

    /// Serialization or deserialization failed.
    #[snafu(display("store serialization error"))]
    StoreSerialization { source: serde_json::Error },

    /// A backend-specific error that doesn't fit other variants.
    #[snafu(display("store backend error: {message}"))]
    Backend { message: String },
}

/// Typed errors for `PlanningService` adapter implementations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (message, source, location, mode, name, kind, id) are self-documenting via display format"
)]
#[non_exhaustive]
pub enum PlanningAdapterError {
    // kanon:ignore RUST/pub-visibility
    #[snafu(display("failed to access workspace: {message}"))]
    Workspace {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("failed to load project: {message}"))]
    LoadProject {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("failed to save project: {message}"))]
    SaveProject {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("failed to serialize project: {source}"))]
    Serialize {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("state transition failed: {message}"))]
    Transition {
        message: String,
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

    #[snafu(display("invalid {kind}: {message}"))]
    InvalidId {
        kind: String,
        message: String,
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
#[expect(
    missing_docs,
    reason = "snafu error variant fields (message, location, reason) are self-documenting via display format"
)]
#[non_exhaustive]
pub enum KnowledgeAdapterError {
    // kanon:ignore RUST/pub-visibility
    #[snafu(display("embedding failed: {message}"))]
    Embedding {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("search failed: {message}"))]
    Search {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("fact query failed: {message}"))]
    FactQuery {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("store mutation failed: {message}"))]
    MutateStore {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    #[snafu(display("datalog query failed: {message}"))]
    DatalogQuery {
        message: String,
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
