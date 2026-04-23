//! Error types for the JSON-first XLSX API.

use snafu::Snafu;

/// Errors produced by [`render_xlsx`](crate::render_xlsx) and
/// [`inspect_xlsx`](crate::inspect_xlsx).
#[derive(Debug, Snafu)]
#[non_exhaustive]
pub enum Error {
    /// The input JSON does not conform to the expected workbook schema.
    #[snafu(display("invalid schema: {detail}"))]
    InvalidSchema {
        /// Human-readable description of the schema violation.
        detail: String,
    },

    /// `rust_xlsxwriter` returned an error while writing the workbook.
    #[snafu(display("XLSX write error: {message}"))]
    XlsxWrite {
        /// Human-readable error description.
        message: String,
    },

    /// `calamine` returned an error while reading the workbook.
    #[snafu(display("XLSX read error: {message}"))]
    XlsxRead {
        /// Human-readable error description.
        message: String,
    },
}
