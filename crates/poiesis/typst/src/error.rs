//! Error types for poiesis-typst.

use snafu::Snafu;

/// Errors returned by the Typst rendering pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub(crate)))]
pub enum PoiesisError {
    /// Data injection produced invalid JSON.
    #[snafu(display("failed to serialize injected data: {detail}"))]
    SerializeData {
        /// Human-readable description of the serialization failure.
        detail: String,
    },

    /// Typst compilation produced errors.
    #[snafu(display("typst compilation failed:\n{diagnostics}"))]
    Compile {
        /// Formatted diagnostics including source locations.
        diagnostics: String,
    },

    /// PDF export failed after successful compilation.
    #[snafu(display("typst PDF export failed:\n{diagnostics}"))]
    PdfExport {
        /// Formatted diagnostics from the PDF exporter.
        diagnostics: String,
    },

    /// A built-in template slug was not recognized.
    #[snafu(display("unknown template slug: {slug:?} (known: {known})"))]
    UnknownTemplate {
        /// The slug that was requested.
        slug: String,
        /// Comma-separated list of known template slugs.
        known: String,
    },
}
