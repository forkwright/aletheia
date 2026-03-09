//! Mneme-specific errors.

use snafu::Snafu;

/// Errors from mneme store operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    /// `SQLite` operation failed.
    #[cfg(feature = "sqlite")]
    #[snafu(display("database error: {source}"))]
    Database {
        source: rusqlite::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session not found.
    #[cfg(feature = "sqlite")]
    #[snafu(display("session not found: {id}"))]
    SessionNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session creation failed.
    #[cfg(feature = "sqlite")]
    #[snafu(display("failed to create session for nous {nous_id}"))]
    SessionCreate {
        nous_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON serialization/deserialization error within stored data.
    #[snafu(display("stored data JSON error: {source}"))]
    StoredJson {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Schema migration failed.
    #[cfg(feature = "sqlite")]
    #[snafu(display("migration to v{version} failed: {source}"))]
    Migration {
        version: u32,
        source: rusqlite::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Filesystem I/O error (archive, backup).
    #[cfg(feature = "sqlite")]
    #[snafu(display("I/O error at {path}: {source}"))]
    Io {
        path: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Agent file version is not supported.
    #[cfg(feature = "sqlite")]
    #[snafu(display("unsupported agent file version: {version}"))]
    UnsupportedVersion {
        version: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Workspace file path contains unsafe traversal.
    #[cfg(feature = "sqlite")]
    #[snafu(display("unsafe path in agent file: {path}"))]
    UnsafePath {
        path: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Backup path contains characters unsafe for SQL interpolation.
    #[cfg(feature = "sqlite")]
    #[snafu(display("invalid backup path: {path}"))]
    InvalidBackupPath {
        path: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Engine initialization failed.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("engine initialization failed: {message}"))]
    EngineInit {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Engine query failed.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("engine query failed: {message}"))]
    EngineQuery {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Query exceeded the configured timeout duration.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("query timed out after {secs:.1}s"))]
    QueryTimeout {
        secs: f64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Schema version mismatch.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("schema version mismatch: expected {expected}, found {found}"))]
    SchemaVersion {
        expected: i64,
        found: i64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Spawned blocking task failed.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("spawned task failed: {source}"))]
    Join {
        source: tokio::task::JoinError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// `DataValue` type conversion failed.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("DataValue conversion failed: {message}"))]
    Conversion {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Fact content was empty.
    #[snafu(display("fact content must not be empty"))]
    EmptyContent {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Fact content exceeded maximum length.
    #[snafu(display("fact content too long: {actual} bytes (max {max})"))]
    ContentTooLong {
        max: usize,
        actual: usize,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Confidence score was outside the valid [0.0, 1.0] range.
    #[snafu(display("confidence must be in [0.0, 1.0], got {value}"))]
    InvalidConfidence {
        value: f64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A timestamp string could not be parsed.
    #[snafu(display("invalid timestamp: {source}"))]
    InvalidTimestamp {
        source: jiff::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Entity name was empty.
    #[snafu(display("entity name must not be empty"))]
    EmptyEntityName {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Relationship weight was outside the valid [0.0, 1.0] range.
    #[snafu(display("relationship weight must be in [0.0, 1.0], got {value}"))]
    InvalidWeight {
        value: f64,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Embedding vector was empty.
    #[snafu(display("embedding vector must not be empty"))]
    EmptyEmbedding {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Embedding content was empty.
    #[snafu(display("embedding content must not be empty"))]
    EmptyEmbeddingContent {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// HNSW vector index operation failed.
    #[cfg(feature = "hnsw_rs")]
    #[snafu(display("HNSW index error: {message}"))]
    HnswIndex {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Result alias using mneme's [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;
