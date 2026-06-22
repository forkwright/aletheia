//! Errors for the benchmark isolation and evidence API.

// WHY: Benchmark helpers live in mneme so eval/tooling crates share one
// typed error surface instead of re-inventing store-error wrappers.

use snafu::Snafu;

/// Errors that can occur while setting up or using benchmark-scoped memory.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum BenchmarkError {
    /// The in-memory knowledge store could not be opened for an isolated scope.
    #[snafu(display("failed to open isolated knowledge store: {source}"))]
    KnowledgeStore {
        /// Underlying episteme/krites store error.
        source: crate::knowledge_error::Error,
    },

    /// The in-memory session store could not be opened for an isolated scope.
    #[snafu(display("failed to open isolated session store: {source}"))]
    SessionStore {
        /// Underlying graphe store error.
        source: crate::error::Error,
    },

    /// A seed fact could not be inserted into the isolated knowledge store.
    #[snafu(display("failed to insert seed fact: {source}"))]
    InsertFact {
        /// Underlying knowledge-store error.
        source: crate::knowledge_error::Error,
    },

    /// A benchmark session could not be created in the isolated session store.
    #[snafu(display("failed to create benchmark session: {source}"))]
    CreateSession {
        /// Underlying session-store error.
        source: crate::error::Error,
    },

    /// The post-question fact-count verification query failed.
    #[snafu(display("fact query failed: {source}"))]
    QueryFacts {
        /// Underlying knowledge-store query error.
        source: crate::knowledge_error::Error,
    },

    /// A caller-supplied fact identifier was rejected by the domain newtype.
    #[snafu(display("invalid fact id {id}: {source}"))]
    InvalidFactId {
        /// Supplied identifier that failed validation.
        id: String,
        /// Underlying id validation error.
        source: crate::id::IdValidationError,
    },

    /// A storage-level invariant expected by the benchmark harness was violated.
    #[snafu(display("{message}"))]
    Storage {
        /// Human-readable description of the violation.
        message: String,
    },
}
