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
}

/// Result alias using mneme's [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;
