//! Error types for poiesis-slides.

use snafu::Snafu;

/// Errors produced by JSON-first PPTX operations.
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum Error {
    /// The supplied JSON does not match the expected slide schema.
    #[snafu(display("invalid slide JSON: {message}"))]
    InvalidJson {
        /// Human-readable description of the schema violation.
        message: String,
    },

    /// An error occurred while reading the PPTX ZIP archive.
    #[snafu(display("ZIP read error: {message}"))]
    ZipRead {
        /// Human-readable error description.
        message: String,
    },

    /// An error occurred while parsing OOXML XML inside the PPTX archive.
    #[snafu(display("XML parse error: {message}"))]
    XmlParse {
        /// Human-readable error description.
        message: String,
    },

    /// An error occurred during PPTX generation via the hand-rolled emitter.
    #[snafu(display("PPTX render error: {source}"))]
    Render {
        /// Underlying emitter error.
        source: crate::pptx::PptxError,
    },
}
