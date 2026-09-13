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

    /// Cross-reference offsets would make two indirect objects claim the same
    /// source region. This is a malformed document, not an exhausted memory
    /// budget.
    #[snafu(display("PDF has overlapping indirect-object source spans"))]
    PdfOverlappingObjectSpans,

    /// The vendored parser panicked while handling hostile PDF bytes.
    ///
    /// This boundary remains in place even though known arithmetic panic paths
    /// are rejected explicitly, so one malformed document cannot unwind an
    /// ingestion worker or synchronous tool executor. `catch_unwind`'s payload
    /// is `Box<dyn Any + Send>`, not `std::error::Error`, so it cannot be a
    /// `#[snafu(source)]`; its recovered message is carried in `detail`
    /// instead so the panic reason is never silently discarded.
    #[snafu(display("PDF parser aborted while rejecting malformed input: {detail}"))]
    PdfParserPanicked {
        /// Message recovered from the `catch_unwind` panic payload.
        detail: String,
    },

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
