//! Melete-specific errors.

use snafu::Snafu;

/// Errors from distillation operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
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
}

/// Convenience alias for `Result` with melete's [`Error`] type.
pub type Result<T> = std::result::Result<T, Error>;
