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
// kanon:ignore RUST/non-exhaustive-enum -- WHY: #[non_exhaustive] is already present; linter false-positive when an intervening #[expect] separates the attribute from the enum keyword
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

    /// Attempted to use an archived session via the normal message path without
    /// explicitly reactivating it first.
    ///
    /// Callers must call the unarchive endpoint (`POST /sessions/{id}/unarchive`)
    /// before resuming an archived session.
    #[snafu(display(
        "session '{id}' is archived; use POST /sessions/{id}/unarchive to reactivate"
    ))]
    SessionIsArchived {
        id: String,
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

    /// The store directory holds existing data but no schema manifest file —
    /// it predates the manifest feature, or the manifest was removed.
    ///
    /// WHY(#5031): an absent manifest and a corrupted one are
    /// indistinguishable from a plain file read, and a wrong guess here is
    /// exactly the "repair by discarding" defect this guard exists to
    /// prevent (fjall's own keyspace recovery has separately deleted ~600
    /// issue records when opened against an unexpected on-disk state).
    /// Refused rather than silently stamped; run `aletheia session-store
    /// stamp` (backed by
    /// [`crate::store::SessionStore::stamp_legacy_schema_manifest`]) once you
    /// have confirmed the store already matches `current`.
    #[snafu(display(
        "session store at {} has existing data but no schema manifest; run `aletheia \
         session-store stamp --path {}` once you have confirmed it matches schema version \
         {current} before opening",
        path.display(),
        path.display()
    ))]
    SchemaManifestMissing {
        path: std::path::PathBuf,
        current: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The on-disk schema manifest exists but could not be parsed as JSON.
    #[snafu(display("session store manifest at {} is corrupt: {source}", path.display()))]
    SchemaManifestCorrupt {
        path: std::path::PathBuf,
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The on-disk schema version predates what this binary understands and
    /// no migration path is registered for the gap.
    #[snafu(display(
        "session store at {} is schema version {found}, this binary requires {expected} and \
         has no migration path registered for that gap; refusing to open rather than guess",
        path.display()
    ))]
    SchemaVersionTooOld {
        path: std::path::PathBuf,
        expected: u32,
        found: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The on-disk schema version is newer than this binary understands.
    #[snafu(display(
        "session store at {} is schema version {found}, this binary only understands up to \
         {expected}; upgrade aletheia before opening this store",
        path.display()
    ))]
    SchemaVersionTooNew {
        path: std::path::PathBuf,
        expected: u32,
        found: u32,
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

    /// Embedding vector dimension does not match the store's configured dimension.
    #[cfg(feature = "mneme-engine")]
    #[snafu(display("embedding dimension mismatch: expected {expected}, got {actual}"))]
    EmbeddingDimensionMismatch {
        expected: usize,
        actual: usize,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

impl Error {
    /// Whether this error represents a UNIQUE constraint violation
    /// (duplicate session key).
    ///
    /// Graphe constructs [`Error::Storage`] with a message prefix
    /// `"UNIQUE constraint failed"` in `fjall_store.rs` when a session
    /// id or `(nous_id, session_key)` index already contains an entry.
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
