//! Melete-specific errors.

use snafu::Snafu;

/// Errors from distillation operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu variant fields are self-documenting via display format"
)]
pub enum Error {
    /// LLM call failed during distillation.
    #[snafu(display("LLM call failed during distillation: {source}"))]
    LlmCall {
        source: aletheia_hermeneus::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Distillation produced an empty summary.
    #[snafu(display("distillation produced empty summary"))]
    EmptySummary {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Session has no messages to distill.
    #[snafu(display("session has no messages to distill"))]
    NoMessages {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// LLM provider panicked during distillation. (#2216)
    #[snafu(display("LLM call panicked during distillation: {message}"))]
    LlmPanic {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// I/O error during consolidation lock operations.
    #[snafu(display("consolidation lock I/O: {context}"))]
    DreamLockIo {
        context: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Consolidation lock is held by another active process.
    #[snafu(display("consolidation lock held by PID {pid}"))]
    DreamLockHeld {
        pid: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Transcript source failed during auto-dream consolidation.
    #[snafu(display("transcript source error: {context}"))]
    DreamTranscriptSource {
        context: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Consolidation target failed during fact merge.
    #[snafu(display("consolidation target error: {context}"))]
    DreamConsolidationTarget {
        context: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for `Result` with melete's [`Error`] type.
pub(crate) type Result<T> = std::result::Result<T, Error>;
