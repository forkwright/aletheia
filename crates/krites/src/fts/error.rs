//! Error types for the full-text search subsystem.
use snafu::Snafu;

/// Error for FTS tokenizer configuration and indexing failures.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub(crate) enum FtsError {
    /// A tokenizer or filter configuration was invalid, or indexing encountered
    /// an unexpected value.
    #[snafu(display("tokenization failed: {message}"))]
    TokenizationFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// N-gram tokenizer was constructed with invalid gram bounds.
    #[snafu(display("invalid n-gram configuration: {message}"))]
    InvalidNgramConfig {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}
