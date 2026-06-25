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

/// Errors produced by [`crate::render_workbook`].
#[cfg(feature = "workbook")]
#[derive(Debug, Snafu)]
#[snafu(module)]
#[non_exhaustive]
pub enum WorkbookError {
    /// A [`WorkbookCell::Cite`](crate::workbook::WorkbookCell) references a fact id not present in the resolved factbase.
    #[snafu(display("unknown fact id: {id}"))]
    UnknownFact {
        /// The fact id that was not found.
        id: String,
    },
    /// `rust_xlsxwriter` returned an error.
    #[snafu(display("XLSX write error: {message}"))]
    XlsxWrite {
        /// Human-readable error description.
        message: String,
    },
    /// A [`WorkbookCell`](poiesis_core::bodies::WorkbookCell) variant is not
    /// supported by this renderer (forward-compatibility guard for
    /// `#[non_exhaustive]` additions).
    #[snafu(display("unsupported cell kind"))]
    UnsupportedCellKind,
}

#[cfg(feature = "workbook")]
impl From<rust_xlsxwriter::XlsxError> for WorkbookError {
    fn from(e: rust_xlsxwriter::XlsxError) -> Self {
        Self::XlsxWrite {
            message: e.to_string(),
        }
    }
}
