//! Error types for poiesis-doc.

use snafu::Snafu;

/// Errors returned by the DOCX rendering and inspection pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
#[non_exhaustive]
pub enum Error {
    /// The input JSON did not match the expected render schema.
    #[snafu(display("malformed input: {detail}"))]
    MalformedInput {
        /// Human-readable description of the schema violation.
        detail: String,
    },

    /// DOCX generation failed.
    #[snafu(display("docx build failed: {detail}"))]
    BuildDocx {
        /// Human-readable description of the build failure.
        detail: String,
    },

    /// ZIP inspection failed.
    #[snafu(display("zip read failed: {source}"))]
    ReadZip {
        /// Underlying ZIP error.
        source: zip::result::ZipError,
    },

    /// XML parsing during inspection failed.
    #[snafu(display("xml parse failed: {source}"))]
    ParseXml {
        /// Underlying quick-xml error.
        source: quick_xml::Error,
    },

    /// PDF rendering via Typst failed.
    #[snafu(display("pdf render failed: {detail}"))]
    PdfRenderFailed {
        /// Human-readable description.
        detail: String,
    },

    /// ODT rendering via the clean-room backend failed.
    #[snafu(display("odt render failed: {detail}"))]
    OdtRenderFailed {
        /// Human-readable description.
        detail: String,
    },

    /// A Pandoc-backed format could not be rendered.
    #[snafu(display(
        "{format} output requires Pandoc; install pandoc >= 3.0 or use pdf/xlsx for now"
    ))]
    PandocRequired {
        /// The requested format name (e.g. "docx").
        format: String,
    },
}
