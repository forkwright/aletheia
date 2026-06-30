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
    const NAMED_ENTITY_TEXT: &str = r"A &amp; B &lt; C &gt; D &apos;Q&apos; &quot;R&quot; &#x2019;";
    const NUMERIC_ENTITY_TEXT: &str = r"A &#38; B &#60; C &#62; D &#39;Q&#39; &#34;R&#34; &#8217;";

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

    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn xlsx_with_shared_string(encoded_text: &str) -> Vec<u8> {
        use std::io::Write;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        const WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="Sheet1" sheetId="1" r:id="rId1"/>
  </sheets>
</workbook>"#;

        const SHEET1: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1"><c r="A1" t="s"><v>0</v></c></row>
  </sheetData>
</worksheet>"#;

        let shared_strings = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1" uniqueCount="1">
  <si><t>{encoded_text}</t></si>
</sst>"#
        );

        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("xl/workbook.xml", options)
            .expect("start workbook.xml");
        zip.write_all(WORKBOOK.as_bytes())
            .expect("write workbook.xml");

        zip.start_file("xl/sharedStrings.xml", options)
            .expect("start sharedStrings.xml");
        zip.write_all(shared_strings.as_bytes())
            .expect("write sharedStrings.xml");

        zip.start_file("xl/worksheets/sheet1.xml", options)
            .expect("start sheet1.xml");
        zip.write_all(SHEET1.as_bytes()).expect("write sheet1.xml");

        zip.finish().expect("finish zip");
        cursor.into_inner()
    }

    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn pptx_with_slide_text(encoded_text: &str) -> Vec<u8> {
        use std::io::Write;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        let slide = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp><p:txBody><a:p><a:r><a:t>{encoded_text}</a:t></a:r></a:p></p:txBody></p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#
        );

        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("ppt/slides/slide1.xml", options)
            .expect("start slide1.xml");
        zip.write_all(slide.as_bytes()).expect("write slide1.xml");

        zip.finish().expect("finish zip");
        cursor.into_inner()
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn diff_workbooks_ignores_equivalent_xml_entity_encodings() {
        let named = xlsx_with_shared_string(NAMED_ENTITY_TEXT);
        let numeric = xlsx_with_shared_string(NUMERIC_ENTITY_TEXT);

        let diffs = diff_workbooks(&named, &numeric).expect("diff must succeed");

        assert!(
            diffs.is_empty(),
            "equivalent XML entity encodings must not produce diffs, got: {diffs:?}"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn diff_presentations_ignores_equivalent_xml_entity_encodings() {
        let named = pptx_with_slide_text(NAMED_ENTITY_TEXT);
        let numeric = pptx_with_slide_text(NUMERIC_ENTITY_TEXT);

        let diffs = diff_presentations(&named, &numeric).expect("diff must succeed");

        assert!(
            diffs.is_empty(),
            "equivalent XML entity encodings must not produce diffs, got: {diffs:?}"
        );
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

    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn nonsequential_xlsx_fixture(beta_value: &str) -> Vec<u8> {
        use std::io::Write;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/worksheets/sheet3.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>
</Types>"#;

        const WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>
    <sheet name="Alpha" sheetId="1" r:id="rId1"/>
    <sheet name="Beta" sheetId="3" r:id="rId2"/>
  </sheets>
</workbook>"#;

        const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet3.xml"/>
  <Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>
</Relationships>"#;

        const SHEET1: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1"><c r="A1" t="s"><v>0</v></c></row>
  </sheetData>
</worksheet>"#;

        let shared_strings = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2">
  <si><t>AlphaValue</t></si>
  <si><t>{beta_value}</t></si>
</sst>"#
        );

        let sheet3 = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1"><c r="A1" t="s"><v>1</v></c></row>
  </sheetData>
</worksheet>"#
            .to_string();

        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("[Content_Types].xml", options)
            .expect("start [Content_Types].xml");
        zip.write_all(CONTENT_TYPES.as_bytes())
            .expect("write [Content_Types].xml");

        zip.start_file("xl/workbook.xml", options)
            .expect("start workbook.xml");
        zip.write_all(WORKBOOK.as_bytes())
            .expect("write workbook.xml");

        zip.start_file("xl/_rels/workbook.xml.rels", options)
            .expect("start rels");
        zip.write_all(RELS.as_bytes()).expect("write rels");

        zip.start_file("xl/sharedStrings.xml", options)
            .expect("start sharedStrings");
        zip.write_all(shared_strings.as_bytes())
            .expect("write sharedStrings");

        zip.start_file("xl/worksheets/sheet1.xml", options)
            .expect("start sheet1");
        zip.write_all(SHEET1.as_bytes()).expect("write sheet1");

        zip.start_file("xl/worksheets/sheet3.xml", options)
            .expect("start sheet3");
        zip.write_all(sheet3.as_bytes()).expect("write sheet3");

        zip.finish().expect("finish zip");
        cursor.into_inner()
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn diff_workbooks_resolves_nonsequential_worksheet_paths_without_false_diffs() {
        let bytes = nonsequential_xlsx_fixture("BetaValue");
        let diffs = diff_workbooks(&bytes, &bytes).expect("diff must succeed");
        assert!(
            diffs.is_empty(),
            "identical non-sequential workbooks must produce zero diffs, got: {diffs:?}"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn diff_workbooks_detects_changes_on_nonsequential_sheet() {
        let bytes_a = nonsequential_xlsx_fixture("BetaValue");
        let bytes_b = nonsequential_xlsx_fixture("ChangedBeta");

        let diffs = diff_workbooks(&bytes_a, &bytes_b).expect("diff must succeed");
        assert_eq!(diffs.len(), 1, "expected exactly one diff");
        let diff = diffs.first().expect("one diff");
        assert_eq!(diff.sheet, "Beta", "diff must be on Beta sheet");
        assert_eq!(diff.row, 0);
        assert_eq!(diff.col, 0);
        assert_eq!(diff.before.as_deref(), Some("BetaValue"));
        assert_eq!(diff.after.as_deref(), Some("ChangedBeta"));
    }
}
