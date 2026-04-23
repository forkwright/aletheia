#![deny(missing_docs)]
//! poiesis-diff: cell-level diff for XLSX and PPTX documents.
//!
//! Provides functions to compare XLSX workbooks and PPTX presentations at the
//! cell/slide level, detecting insertions, deletions, and modifications.

mod error;
mod pptx;
mod xlsx;

pub use error::{DiffError, Result};

use tracing::instrument;

/// A cell-level difference in a workbook.
///
/// Represents a single changed, inserted, or deleted cell.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellDiff {
    /// Sheet name where the difference occurs.
    pub sheet: String,
    /// Row index (0-based).
    pub row: u32,
    /// Column index (0-based).
    pub col: u32,
    /// Content before the change (None if inserted).
    pub before: Option<String>,
    /// Content after the change (None if deleted).
    pub after: Option<String>,
}

/// A slide-level difference in a presentation.
///
/// Represents slide content changes between two PPTX presentations.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlideDiff {
    /// Slide index (0-based).
    pub slide_index: usize,
    /// Text content before the change.
    pub before: Option<String>,
    /// Text content after the change.
    pub after: Option<String>,
}

/// Compare two XLSX workbooks and return a list of cell-level differences.
///
/// # Errors
///
/// Returns an error if the input bytes cannot be parsed as valid XLSX files.
#[instrument(skip_all, fields(a_bytes = a.len(), b_bytes = b.len()))]
pub fn diff_workbooks(a: &[u8], b: &[u8]) -> Result<Vec<CellDiff>> {
    xlsx::diff_workbooks_impl(a, b)
}

/// Compare two PPTX presentations and return a list of slide-level differences.
///
/// # Errors
///
/// Returns an error if the input bytes cannot be parsed as valid PPTX files.
#[instrument(skip_all, fields(a_bytes = a.len(), b_bytes = b.len()))]
pub fn diff_presentations(a: &[u8], b: &[u8]) -> Result<Vec<SlideDiff>> {
    pptx::diff_presentations_impl(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal valid XLSX structure (just the raw bytes)
    const MINIMAL_XLSX: &[u8] = b"PK\x03\x04";

    #[test]
    fn test_diff_workbooks_accepts_bytes() {
        // Test that the function accepts byte input; real XLSX parsing
        // is tested at the integration level
        let result = diff_workbooks(MINIMAL_XLSX, MINIMAL_XLSX);
        // May error (invalid XLSX), but shouldn't panic
        let _ = result;
    }

    #[test]
    fn test_diff_presentations_accepts_bytes() {
        // Test that the function accepts byte input
        let result = diff_presentations(MINIMAL_XLSX, MINIMAL_XLSX);
        // May error (invalid PPTX), but shouldn't panic
        let _ = result;
    }
}
