//! Error types for krites v2.

use snafu::Snafu;

/// Errors from the krites v2 engine.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    /// Datalog parse error.
    #[snafu(display("parse error at {span}: {message}"))]
    Parse {
        /// Human-readable error description.
        message: String,
        /// Source location span.
        span: String,
        #[snafu(implicit)]
        /// Captured source location.
        location: snafu::Location,
    },

    /// Query planning failed.
    #[snafu(display("plan error: {message}"))]
    Plan {
        /// What went wrong during planning.
        message: String,
        #[snafu(implicit)]
        /// Captured source location.
        location: snafu::Location,
    },

    /// Evaluation error during query execution.
    #[snafu(display("eval error: {message}"))]
    Eval {
        /// What went wrong during evaluation.
        message: String,
        #[snafu(implicit)]
        /// Captured source location.
        location: snafu::Location,
    },

    /// Storage backend error.
    #[snafu(display("storage error: {message}"))]
    Storage {
        /// What went wrong in the storage layer.
        message: String,
        #[snafu(implicit)]
        /// Captured source location.
        location: snafu::Location,
    },

    /// Schema violation (type mismatch, missing column, etc.).
    #[snafu(display("schema error: {message}"))]
    Schema {
        /// What violated the schema.
        message: String,
        #[snafu(implicit)]
        /// Captured source location.
        location: snafu::Location,
    },

    /// Index operation failed.
    #[snafu(display("index error on {index_name}: {message}"))]
    Index {
        /// Name of the index that errored.
        index_name: String,
        /// What went wrong.
        message: String,
        #[snafu(implicit)]
        /// Captured source location.
        location: snafu::Location,
    },

    /// Fixed rule (graph algorithm) error.
    #[snafu(display("algorithm error in {algorithm}: {message}"))]
    Algorithm {
        /// Name of the algorithm.
        algorithm: String,
        /// What went wrong.
        message: String,
        #[snafu(implicit)]
        /// Captured source location.
        location: snafu::Location,
    },

    /// Transaction error (conflict, timeout, etc.).
    #[snafu(display("transaction error: {message}"))]
    Transaction {
        /// What went wrong.
        message: String,
        #[snafu(implicit)]
        /// Captured source location.
        location: snafu::Location,
    },
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;
