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

impl CellDiff {
    /// Construct a new cell diff entry.
    pub fn new(
        sheet: impl Into<String>,
        row: u32,
        col: u32,
        before: Option<String>,
        after: Option<String>,
    ) -> Self {
        Self {
            sheet: sheet.into(),
            row,
            col,
            before,
            after,
        }
    }
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

impl SlideDiff {
    /// Construct a new slide diff entry.
    pub fn new(slide_index: usize, before: Option<String>, after: Option<String>) -> Self {
        Self {
            slide_index,
            before,
            after,
        }
    }
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

    const MINIMAL_XLSX: &[u8] = b"PK\x03\x04";

    #[test]
    fn truncated_zip_bytes_return_error_for_workbooks() {
        let result = diff_workbooks(MINIMAL_XLSX, MINIMAL_XLSX);
        assert!(result.is_err(), "incomplete ZIP should not parse as XLSX");
    }

    #[test]
    fn truncated_zip_bytes_return_error_for_presentations() {
        let result = diff_presentations(MINIMAL_XLSX, MINIMAL_XLSX);
        assert!(result.is_err(), "incomplete ZIP should not parse as PPTX");
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn diff_detects_changes_on_correct_sheet_with_non_alphabetical_names() {
        let before = serde_json::json!({
            "sheets": [
                {
                    "name": "Zebra",
                    "columns": [{ "header": "A" }],
                    "rows": [["old"]]
                },
                {
                    "name": "Apple",
                    "columns": [{ "header": "B" }],
                    "rows": [["stable"]]
                }
            ]
        });
        let after = serde_json::json!({
            "sheets": [
                {
                    "name": "Zebra",
                    "columns": [{ "header": "A" }],
                    "rows": [["new"]]
                },
                {
                    "name": "Apple",
                    "columns": [{ "header": "B" }],
                    "rows": [["stable"]]
                }
            ]
        });

        let bytes_a = poiesis_sheet::render_xlsx(&before).expect("render a");
        let bytes_b = poiesis_sheet::render_xlsx(&after).expect("render b");

        let diffs = diff_workbooks(&bytes_a, &bytes_b).expect("diff must succeed");
        assert_eq!(diffs.len(), 1, "expected exactly one diff");
        let diff = diffs.first().expect("one diff");
        assert_eq!(diff.sheet, "Zebra", "diff must be on Zebra sheet");
        assert_eq!(diff.row, 1);
        assert_eq!(diff.col, 0);
        assert_eq!(diff.before.as_deref(), Some("old"));
        assert_eq!(diff.after.as_deref(), Some("new"));
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn diff_detects_sheet_present_only_in_workbook_b() {
        // WHY: diff_workbooks previously iterated only workbook_a sheets,
        // silently dropping entire sheets added in workbook_b.
        let before = serde_json::json!({
            "sheets": [
                {
                    "name": "Existing",
                    "columns": [],
                    "rows": []
                }
            ]
        });
        let after = serde_json::json!({
            "sheets": [
                {
                    "name": "Existing",
                    "columns": [],
                    "rows": []
                },
                {
                    "name": "Added",
                    "columns": [],
                    "rows": [["x"], ["y"]]
                }
            ]
        });

        let bytes_a = poiesis_sheet::render_xlsx(&before).expect("render a");
        let bytes_b = poiesis_sheet::render_xlsx(&after).expect("render b");

        let diffs = diff_workbooks(&bytes_a, &bytes_b).expect("diff must succeed");
        let added: Vec<_> = diffs.into_iter().filter(|d| d.sheet == "Added").collect();

        assert_eq!(added.len(), 2, "expected two inserted cells in Added sheet");
        for diff in &added {
            assert_eq!(diff.before, None, "added cells have no before value");
            assert!(diff.after.is_some(), "added cells have an after value");
        }

        let values: std::collections::HashSet<_> = added
            .iter()
            .map(|d| d.after.as_deref().expect("after value").to_string())
            .collect();
        assert!(values.contains("x"));
        assert!(values.contains("y"));
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn diff_workbooks_no_false_diff_for_entity_content() {
        let data = serde_json::json!({
            "sheets": [
                {
                    "name": "Sheet1",
                    "columns": [{ "header": "Label" }],
                    "rows": [[r#"A & B < C > D "E" 'F' ’"#]]
                }
            ]
        });

        let bytes_a = poiesis_sheet::render_xlsx(&data).expect("render a");
        let bytes_b = poiesis_sheet::render_xlsx(&data).expect("render b");

        let diffs = diff_workbooks(&bytes_a, &bytes_b).expect("diff must succeed");
        assert!(
            diffs.is_empty(),
            "identical workbooks must produce no diffs, got: {diffs:?}"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn diff_presentations_no_false_diff_for_entity_content() {
        let data = serde_json::json!({
            "slides": [
                {
                    "title": r#"A & B < C > D "E" 'F' ’"#,
                    "content": []
                }
            ]
        });

        let bytes_a = poiesis_slides::render_pptx(&data).expect("render a");
        let bytes_b = poiesis_slides::render_pptx(&data).expect("render b");

        let diffs = diff_presentations(&bytes_a, &bytes_b).expect("diff must succeed");
        assert!(
            diffs.is_empty(),
            "identical presentations must produce no diffs, got: {diffs:?}"
        );
    }
}
