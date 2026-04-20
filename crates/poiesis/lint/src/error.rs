// Error types for poiesis-lint.

use snafu::Snafu;

/// Errors that can occur during lint operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum LintError {
    /// Failed to read the input file.
    #[snafu(display("failed to read file {path:?}: {source}"))]
    ReadFile {
        /// Path that could not be read.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// Failed to write fixed content back to the file.
    #[snafu(display("failed to write file {path:?}: {source}"))]
    WriteFile {
        /// Path that could not be written.
        path: String,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// Failed to serialize findings to JSON.
    #[snafu(display("failed to serialize findings: {source}"))]
    Serialize {
        /// Underlying serde error.
        source: serde_json::Error,
    },
}
