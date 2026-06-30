#![deny(missing_docs)]
//! poiesis-inspect: text extraction from PDF, XLSX, and PPTX documents.
//!
//! Provides functions to read and extract text content from common document
//! formats, allowing agents to inspect their own generated outputs.

mod error;
mod pdf;
mod pptx;
mod xlsx;

pub use error::{InspectError, Result};

use tracing::instrument;

/// Summary of extracted text and metadata from a PDF document.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfSummary {
    /// Number of pages in the PDF.
    pub pages: usize,
    /// Whether `pages` was produced by a successful lopdf parse.
    ///
    /// When `false`, `pages` is set to `1` because lopdf could not count
    /// the pages and no other value is safe to report.
    pub page_count_reliable: bool,
    /// Extracted text snippets from each page.
    pub text_snippets: Vec<String>,
    /// Whether `text_snippets` was truncated to the 100-line storage cap.
    pub truncated: bool,
    /// Total number of non-empty lines in the extracted text.
    ///
    /// When `truncated` is `false`, this equals `text_snippets.len()`.
    pub total_lines: usize,
}

impl PdfSummary {
    pub(crate) fn new(
        pages: usize,
        page_count_reliable: bool,
        text_snippets: Vec<String>,
        truncated: bool,
        total_lines: usize,
    ) -> Self {
        Self {
            pages,
            page_count_reliable,
            text_snippets,
            truncated,
            total_lines,
        }
    }
}

/// Summary of extracted text and metadata from an XLSX workbook.
///
/// This mirrors the structure from sibling crates if they expose it;
/// otherwise it is defined locally here.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct WorkbookSummary {
    /// Sheet names and their extracted text content, in workbook order.
    pub sheets: indexmap::IndexMap<String, String>,
}

/// Summary of extracted text and metadata from a PPTX presentation.
///
/// This mirrors the structure from sibling crates if they expose it;
/// otherwise it is defined locally here.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct PresentationSummary {
    /// Slide text content (indexed by slide number).
    pub slides: Vec<String>,
}

/// Extract text from a PDF document.
///
/// # Errors
///
/// Returns an error if the input bytes cannot be parsed as a valid PDF or if
/// text extraction fails.
#[instrument(skip_all, fields(bytes = bytes.len()))]
pub fn inspect_pdf(bytes: &[u8]) -> Result<PdfSummary> {
    pdf::inspect_pdf_impl(bytes)
}

/// Extract text from an XLSX workbook.
///
/// # Errors
///
/// Returns an error if the input bytes cannot be parsed as a valid XLSX.
#[instrument(skip_all, fields(bytes = bytes.len()))]
pub fn inspect_xlsx(bytes: &[u8]) -> Result<WorkbookSummary> {
    xlsx::inspect_xlsx_impl(bytes)
}

/// Extract text from a PPTX presentation.
///
/// # Errors
///
/// Returns an error if the input bytes cannot be parsed as a valid PPTX.
#[instrument(skip_all, fields(bytes = bytes.len()))]
pub fn inspect_pptx(bytes: &[u8]) -> Result<PresentationSummary> {
    pptx::inspect_pptx_impl(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMED_ENTITY_TEXT: &str = r"A &amp; B &lt; C &gt; D &apos;Q&apos; &quot;R&quot; &#x2019;";
    const DECODED_ENTITY_TEXT: &str = "A & B < C > D 'Q' \"R\" \u{2019}";

    #[test]
    fn inspect_pdf_rejects_invalid_bytes() {
        let malformed = b"not a pdf";
        let result = inspect_pdf(malformed);
        assert!(result.is_err());
    }

    #[test]
    fn inspect_xlsx_rejects_invalid_bytes() {
        let invalid = b"not xlsx";
        let result = inspect_xlsx(invalid);
        assert!(result.is_err());
    }

    #[test]
    fn inspect_pptx_rejects_invalid_bytes() {
        let invalid = b"not pptx";
        let result = inspect_pptx(invalid);
        assert!(result.is_err());
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
    fn inspect_xlsx_unescapes_xml_character_entities() {
        let bytes = xlsx_with_shared_string(NAMED_ENTITY_TEXT);
        let summary = inspect_xlsx(&bytes).expect("inspect must succeed");
        let sheet_text = summary.sheets.get("Sheet1").expect("Sheet1 text");

        assert!(
            sheet_text.contains(DECODED_ENTITY_TEXT),
            "sheet text must contain decoded XML entities, got: {sheet_text}"
        );
        assert!(
            !sheet_text.contains("&amp;"),
            "sheet text must not expose raw XML entities, got: {sheet_text}"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pptx_unescapes_xml_character_entities() {
        let bytes = pptx_with_slide_text(NAMED_ENTITY_TEXT);
        let summary = inspect_pptx(&bytes).expect("inspect must succeed");

        assert_eq!(summary.slides, vec![DECODED_ENTITY_TEXT.to_string()]);
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_xlsx_resolves_shared_strings_and_sheet_order() {
        let data = serde_json::json!({
            "sheets": [
                {
                    "name": "Zebra",
                    "columns": [{ "header": "Animal" }],
                    "rows": [["Zebra"]]
                },
                {
                    "name": "Apple",
                    "columns": [{ "header": "Fruit" }],
                    "rows": [["Apple"]]
                },
                {
                    "name": "Mango",
                    "columns": [{ "header": "Tropical" }],
                    "rows": [["Mango"]]
                }
            ]
        });

        let bytes = poiesis_sheet::render_xlsx(&data).expect("render must succeed");
        let summary = inspect_xlsx(&bytes).expect("inspect must succeed");

        let names: Vec<&str> = summary.sheets.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            vec!["Zebra", "Apple", "Mango"],
            "sheet order must match workbook order"
        );

        for (name, text) in &summary.sheets {
            assert!(
                text.contains(name),
                "sheet '{name}' text must contain its shared-string value, got: {text}"
            );
        }
    }

    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn nonsequential_xlsx_fixture() -> Vec<u8> {
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

        const SHARED_STRINGS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="2" uniqueCount="2">
  <si><t>AlphaValue</t></si>
  <si><t>BetaValue</t></si>
</sst>"#;

        const SHEET1: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1"><c r="A1" t="s"><v>0</v></c></row>
  </sheetData>
</worksheet>"#;

        const SHEET3: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1"><c r="A1" t="s"><v>1</v></c></row>
  </sheetData>
</worksheet>"#;

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
        zip.write_all(SHARED_STRINGS.as_bytes())
            .expect("write sharedStrings");

        zip.start_file("xl/worksheets/sheet1.xml", options)
            .expect("start sheet1");
        zip.write_all(SHEET1.as_bytes()).expect("write sheet1");

        zip.start_file("xl/worksheets/sheet3.xml", options)
            .expect("start sheet3");
        zip.write_all(SHEET3.as_bytes()).expect("write sheet3");

        zip.finish().expect("finish zip");
        cursor.into_inner()
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_xlsx_resolves_nonsequential_worksheet_paths() {
        let bytes = nonsequential_xlsx_fixture();
        let summary = inspect_xlsx(&bytes).expect("inspect must succeed");

        let names: Vec<&str> = summary.sheets.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            vec!["Alpha", "Beta"],
            "sheet order must match workbook order"
        );

        let alpha = summary.sheets.get("Alpha").expect("Alpha sheet present");
        assert!(
            alpha.contains("AlphaValue"),
            "Alpha text must contain its shared-string value, got: {alpha}"
        );

        let beta = summary.sheets.get("Beta").expect("Beta sheet present");
        assert!(
            beta.contains("BetaValue"),
            "Beta text must contain its shared-string value, got: {beta}"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pdf_counts_real_pages() {
        use poiesis_core::{Block, Document, Metadata, RichText, Span};

        let mut content = Vec::new();
        for i in 0..60 {
            content.push(Block::Paragraph(RichText {
                spans: vec![Span::Plain(format!(
                    "This is paragraph {i} with enough words to force line wrapping and page breaks. \
                     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor."
                ))],
            }));
        }
        let doc = Document {
            metadata: Metadata {
                title: "Multi-page".to_owned(),
                author: None,
                created: None,
            },
            content,
        };

        let bytes = match poiesis_doc::render_pdf_from_doc(&doc) {
            Ok(b) => b,
            Err(e) => {
                // Gracefully skip if Typst is unavailable in this environment
                eprintln!("PDF render skipped: {e}");
                return;
            }
        };
        let summary = inspect_pdf(&bytes).expect("inspect must succeed");
        assert!(
            summary.pages >= 2,
            "expected at least 2 pages for a 60-paragraph doc, got {}",
            summary.pages
        );
        assert!(
            summary.page_count_reliable,
            "page count from a renderable PDF must be marked reliable"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pdf_page_count_reliable_on_valid_pdf() {
        use poiesis_core::{Block, Document, Metadata, RichText, Span};

        let doc = Document {
            metadata: Metadata {
                title: "Single-page".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::Paragraph(RichText {
                spans: vec![Span::Plain("Hello, PDF.".to_owned())],
            })],
        };

        let bytes = match poiesis_doc::render_pdf_from_doc(&doc) {
            Ok(b) => b,
            Err(e) => {
                // Gracefully skip if Typst is unavailable in this environment
                eprintln!("PDF render skipped: {e}");
                return;
            }
        };
        let summary = inspect_pdf(&bytes).expect("inspect must succeed");
        assert_eq!(
            summary.pages, 1,
            "single-paragraph doc should report one page"
        );
        assert!(
            summary.page_count_reliable,
            "page count from a valid PDF must be marked reliable"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pdf_reports_truncation_for_long_documents() {
        use poiesis_core::{Block, Document, Metadata, RichText, Span};

        let mut content = Vec::new();
        for i in 0..150 {
            content.push(Block::Paragraph(RichText {
                spans: vec![Span::Plain(format!("Line {i}"))],
            }));
        }
        let doc = Document {
            metadata: Metadata {
                title: "Long-document".to_owned(),
                author: None,
                created: None,
            },
            content,
        };

        let bytes = match poiesis_doc::render_pdf_from_doc(&doc) {
            Ok(b) => b,
            Err(e) => {
                // Gracefully skip if Typst is unavailable in this environment
                eprintln!("PDF render skipped: {e}");
                return;
            }
        };
        let summary = inspect_pdf(&bytes).expect("inspect must succeed");
        assert!(
            summary.truncated,
            "document with more than 100 lines must be marked truncated"
        );
        assert!(
            summary.total_lines > 100,
            "total_lines must report the raw line count, got {}",
            summary.total_lines
        );
        assert_eq!(
            summary.text_snippets.len(),
            100,
            "truncated summary must contain exactly 100 snippets"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pdf_reports_no_truncation_for_short_documents() {
        use poiesis_core::{Block, Document, Metadata, RichText, Span};

        let doc = Document {
            metadata: Metadata {
                title: "Short-document".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::Paragraph(RichText {
                spans: vec![Span::Plain("One line.".to_owned())],
            })],
        };

        let bytes = match poiesis_doc::render_pdf_from_doc(&doc) {
            Ok(b) => b,
            Err(e) => {
                // Gracefully skip if Typst is unavailable in this environment
                eprintln!("PDF render skipped: {e}");
                return;
            }
        };
        let summary = inspect_pdf(&bytes).expect("inspect must succeed");
        assert!(
            !summary.truncated,
            "short document must not be marked truncated"
        );
        assert_eq!(
            summary.total_lines,
            summary.text_snippets.len(),
            "total_lines must equal snippets when nothing was truncated"
        );
    }
}
