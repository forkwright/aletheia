//! Graphe-specific errors.

use snafu::Snafu;

/// Errors from graphe store operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, location, path, detail) are self-documenting via display format"
)]
pub enum Error {
    /// Session not found.
    #[snafu(display("session not found: {id}"))]
    SessionNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session creation failed.
    #[snafu(display("failed to create session for nous {nous_id}"))]
    SessionCreate {
        nous_id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Storage backend error (fjall LSM-tree).
    #[snafu(display("storage error: {message}"))]
    Storage {
        message: String,
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

    /// Blackboard TTL could not be represented as an expiration timestamp.
    #[snafu(display("blackboard TTL overflow: {ttl_secs} seconds: {source}"))]
    TtlOverflow {
        ttl_secs: i64,
        source: jiff::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Filesystem I/O error (archive, backup, or store open).
    #[snafu(display("I/O error at {}: {source}", path.display()))]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
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

    /// Query rewrite failed while running enhanced recall.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("query rewrite failed: {message}"))]
    QueryRewrite {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Enhanced search could not complete any rewritten query variant.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("enhanced search failed for every query variant: {message}"))]
    EnhancedSearch {
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

    /// Fact rejected by admission control policy.
    #[snafu(display("admission rejected: {reason}"))]
    AdmissionRejected {
        /// Human-readable reason from the admission policy.
        reason: String,
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

    /// Attempted to operate on a fact that does not exist.
    #[snafu(display("fact not found: {id}"))]
    FactNotFound {
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Embedding vector dimension does not match the store's configured dimension.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("embedding dimension mismatch: expected {expected}, got {actual}"))]
    EmbeddingDimensionMismatch {
        expected: usize,
        actual: usize,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Knowledge-domain identifier validation failed.
    #[snafu(display("invalid identifier: {source}"))]
    InvalidId {
        source: eidos::id::IdValidationError,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

impl Error {
    /// Whether this error represents a UNIQUE constraint violation
    /// (duplicate session key).
    ///
    /// The fjall backend emits [`Error::Storage`] with a message prefix
    /// `"UNIQUE constraint failed"` when the `(nous_id, session_key)`
    /// index already contains an entry.
    #[must_use]
    pub fn is_unique_constraint_violation(&self) -> bool {
        matches!(
            self,
            Self::Storage { message, .. }
                if message.starts_with("UNIQUE constraint failed")
        )
    }
}

/// Result alias using graphe's [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_constraint_violation_detected() {
        let err = Error::Storage {
            message: "UNIQUE constraint failed: session (syn, main) already exists".to_owned(),
            location: snafu::location!(),
        };
        assert!(err.is_unique_constraint_violation());
    }

    #[test]
    fn non_unique_storage_error_not_detected() {
        let err = Error::Storage {
            message: "disk full".to_owned(),
            location: snafu::location!(),
        };
        assert!(!err.is_unique_constraint_violation());
    }

    #[test]
    fn non_storage_error_not_detected() {
        let err = Error::SessionNotFound {
            id: "test".to_owned(),
            location: snafu::location!(),
        };
        assert!(!err.is_unique_constraint_violation());
    }
}
