//! Error types for poiesis-inspect.

use snafu::Snafu;

/// Result type for poiesis-inspect operations.
pub type Result<T> = std::result::Result<T, InspectError>;

/// Error type for poiesis-inspect operations.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum InspectError {
    /// Failed to parse ZIP archive (XLSX/PPTX format).
    #[snafu(display("failed to parse ZIP archive: {source}"))]
    ZipError {
        /// Source error from the zip crate.
        source: zip::result::ZipError,
    },

    /// Failed to extract text from PDF.
    #[snafu(display("failed to extract PDF text: {detail}"))]
    PdfExtractionError {
        /// Details about the PDF extraction error.
        detail: String,
    },

    /// Invalid file format (not a valid PDF, XLSX, or PPTX).
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
