#![deny(missing_docs)]
//! poiesis-sheet: XLSX and ODS spreadsheet rendering backends.
//!
//! Feature flags:
//! - `xlsx` (default): Excel XLSX output via `rust_xlsxwriter`.
//! - `ods` (default): `OpenDocument` Spreadsheet output via `spreadsheet-ods`.

#[cfg(feature = "xlsx")]
pub mod xlsx;

#[cfg(feature = "ods")]
pub mod ods;

#[cfg(feature = "xlsx")]
pub use xlsx::XlsxRenderer;

#[cfg(feature = "ods")]
pub use ods::OdsRenderer;
