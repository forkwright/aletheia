//! Error types for poiesis-diff.

use snafu::Snafu;

/// Result type for poiesis-diff operations.
pub type Result<T> = std::result::Result<T, DiffError>;

/// Error type for poiesis-diff operations.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum DiffError {
    /// Failed to parse ZIP archive (XLSX/PPTX format).
    #[snafu(display("failed to parse ZIP archive: {source}"))]
    ZipError {
        /// Source error from the zip crate.
        source: zip::result::ZipError,
    },

    /// Invalid file format (not a valid XLSX or PPTX).
    #[snafu(display("invalid file format: {detail}"))]
    InvalidFormat {
        /// Details about why the file is invalid.
        detail: String,
    },

    /// IO error while reading document.
    #[snafu(display("IO error: {source}"))]
    Io {
        /// Source IO error.
        source: std::io::Error,
    },
}
