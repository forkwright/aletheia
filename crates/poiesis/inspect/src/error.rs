//! Error types for poiesis-inspect.

use snafu::Snafu;

/// Result type for poiesis-inspect operations.
pub type Result<T> = std::result::Result<T, InspectError>;

/// Error type for poiesis-inspect operations.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum InspectError {
    /// The supplied PDF is larger than the caller's accepted input budget.
    #[snafu(display("PDF input exceeds the configured byte limit"))]
    PdfInputTooLarge,

    /// A PDF exceeded one of the parser or extraction resource budgets.
    #[snafu(display("PDF inspection exceeded the configured {limit} limit"))]
    PdfLimitExceeded {
        /// Stable name of the budget that was exceeded.
        limit: &'static str,
    },

    /// The vendored parser panicked while handling hostile PDF bytes.
    ///
    /// This boundary remains in place even though known arithmetic panic paths
    /// are rejected explicitly, so one malformed document cannot unwind an
    /// ingestion worker or synchronous tool executor.
    #[snafu(display("PDF parser aborted while rejecting malformed input"))]
    PdfParserPanicked,

    /// Password-protected PDFs are deliberately unsupported by inspection.
    #[snafu(display("password-protected PDFs are not supported"))]
    EncryptedPdf,

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

impl From<poiesis_ooxml_parse::ArchiveError> for InspectError {
    fn from(err: poiesis_ooxml_parse::ArchiveError) -> Self {
        match err {
            poiesis_ooxml_parse::ArchiveError::Zip { source } => Self::ZipError { source },
            poiesis_ooxml_parse::ArchiveError::Io { source } => Self::Io { source },
        }
    }
}
