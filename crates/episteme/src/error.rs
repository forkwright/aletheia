//! Episteme-specific errors.

use snafu::Snafu;

/// Errors from episteme knowledge operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, location, path, detail) are self-documenting via display format"
)]
// kanon:ignore RUST/non-exhaustive-enum -- WHY: #[non_exhaustive] is already present; linter false-positive when an intervening #[expect] separates the attribute from the enum keyword
pub enum Error {
    /// JSON serialization/deserialization error within stored data.
    #[snafu(display("stored data JSON error: {source}"))]
    StoredJson {
        source: serde_json::Error,
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

    /// Persisted embedding metadata does not match the configured provider.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display(
        "embedding metadata drift detected: stored model '{stored_model}' dim {stored_dim}, configured model '{configured_model}' dim {configured_dim}; run `aletheia memory reembed` to rebuild embeddings before using recall"
    ))]
    EmbeddingDrift {
        stored_model: String,
        stored_dim: usize,
        configured_model: String,
        configured_dim: usize,
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

/// Result alias using episteme's [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;
