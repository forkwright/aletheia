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
pub use pdf::PdfInspectLimits;

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
    inspect_pdf_with_limits(bytes, &PdfInspectLimits::default())
}

/// Inspect a PDF using caller-owned resource limits.
///
/// Both ingestion and the agent report tool construct this policy at their
/// boundary, before the PDF bytes are allocated or parsed.
#[instrument(skip_all, fields(bytes = bytes.len()))]
pub fn inspect_pdf_with_limits(bytes: &[u8], limits: &PdfInspectLimits) -> Result<PdfSummary> {
    pdf::inspect_pdf_impl(bytes, limits)
}

/// Extract a PDF's full text, uncapped.
///
/// WHY(#6751) this exists beside [`inspect_pdf`]: that function summarises, and caps its
/// output at 100 non-empty lines. A consumer that stores the document's content needs
/// all of it -- a capped extraction ingested as if whole records the first hundred lines
/// of a PDF as the document, and the `truncated` flag that would have said so lives on a
/// struct such a caller has no reason to keep.
///
/// # Errors
///
/// Returns an error if the input bytes cannot be parsed as a valid PDF or if text
/// extraction fails.
#[instrument(skip_all, fields(bytes = bytes.len()))]
pub fn extract_pdf_text(bytes: &[u8]) -> Result<String> {
    extract_pdf_text_with_limits(bytes, &PdfInspectLimits::default())
}

/// Extract PDF text using caller-owned resource limits.
#[instrument(skip_all, fields(bytes = bytes.len()))]
pub fn extract_pdf_text_with_limits(bytes: &[u8], limits: &PdfInspectLimits) -> Result<String> {
    pdf::extract_pdf_text_impl(bytes, limits)
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
#[expect(
    clippy::expect_used,
    reason = "test fixture construction and assertions"
)]
#[expect(
    clippy::field_reassign_with_default,
    reason = "tests override one policy limit at a time for readability"
)]
mod tests {
    use super::*;

    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn text_pdf(text: &str, pages: usize) -> Vec<u8> {
        use lopdf::content::{Content, Operation};
        use lopdf::{Document, Object, Stream, dictionary};

        let mut document = Document::with_version("1.5");
        let pages_id = document.new_object_id();
        let font_id = document.add_object(lopdf::dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier"
        });
        let resources_id = document.add_object(lopdf::dictionary! {
            "Font" => lopdf::dictionary! { "F1" => font_id }
        });
        let mut page_ids = Vec::with_capacity(pages);
        for _ in 0..pages {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 12.into()]),
                    Operation::new("Tj", vec![Object::string_literal(text)]),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id = document.add_object(Stream::new(
                lopdf::dictionary! {},
                content.encode().expect("encode content"),
            ));
            page_ids.push(document.add_object(lopdf::dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => content_id
            }));
        }
        let page_count = i64::try_from(pages).expect("test page count fits i64");
        let kids = page_ids
            .into_iter()
            .map(Object::Reference)
            .collect::<Vec<_>>();
        document.objects.insert(
            pages_id,
            Object::Dictionary(lopdf::dictionary! {
                "Type" => "Pages", "Kids" => kids, "Count" => page_count,
                "Resources" => resources_id, "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = document.add_object(lopdf::dictionary! {
            "Type" => "Catalog", "Pages" => pages_id
        });
        document.trailer.set("Root", catalog_id);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).expect("save PDF");
        bytes
    }

    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn object_stream_pdf() -> Vec<u8> {
        use lopdf::{Document, Object, SaveOptions};

        let mut document =
            Document::load_mem(&text_pdf("object stream", 1)).expect("load test PDF");
        for index in 0..8 {
            document.add_object(lopdf::dictionary! {
                "TestObject" => Object::string_literal(format!("object-{index}"))
            });
        }

        let mut bytes = Vec::new();
        document
            .save_with_options(
                &mut bytes,
                SaveOptions::builder()
                    .use_object_streams(true)
                    .use_xref_streams(true)
                    .max_objects_per_stream(1)
                    .compression_level(1)
                    .build(),
            )
            .expect("save object-stream PDF");
        bytes
    }

    const NAMED_ENTITY_TEXT: &str = r"A &amp; B &lt; C &gt; D &apos;Q&apos; &quot;R&quot; &#x2019;";
    const DECODED_ENTITY_TEXT: &str = "A & B < C > D 'Q' \"R\" \u{2019}";

    #[test]
    fn inspect_pdf_rejects_invalid_bytes() {
        let malformed = b"not a pdf";
        let result = inspect_pdf(malformed);
        assert!(result.is_err());
    }

    #[test]
    fn bounded_pdf_extraction_preserves_ordinary_text_and_page_count() {
        let bytes = text_pdf("Hello, bounded PDF!", 2);
        let summary = inspect_pdf(&bytes).expect("inspect ordinary PDF");
        assert_eq!(summary.pages, 2);
        assert!(summary.page_count_reliable);
        assert!(
            summary
                .text_snippets
                .join("\n")
                .contains("Hello, bounded PDF!")
        );
        let extracted = extract_pdf_text(&bytes).expect("extract ordinary PDF");
        assert!(extracted.contains("Hello, bounded PDF!"));
    }

    #[test]
    fn pdf_input_limit_is_checked_before_parser_work() {
        let limits = PdfInspectLimits::for_input_bytes(4);
        let error = inspect_pdf_with_limits(b"%PDF-1.5", &limits).expect_err("input is too large");
        assert!(matches!(error, InspectError::PdfInputTooLarge));
    }

    #[test]
    fn pdf_page_budget_rejects_many_small_pages() {
        let mut limits = PdfInspectLimits::default();
        limits.max_pages = 1;
        let error = inspect_pdf_with_limits(&text_pdf("page", 2), &limits)
            .expect_err("page budget must be enforced");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "page count"
            }
        ));
    }

    #[test]
    fn pdf_aggregate_text_budget_rejects_many_small_pages() {
        let mut limits = PdfInspectLimits::default();
        limits.max_extracted_text_bytes = 8;
        let error = extract_pdf_text_with_limits(&text_pdf("text", 3), &limits)
            .expect_err("aggregate text budget must be enforced");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "extracted text"
            }
        ));
    }

    #[test]
    fn pdf_aggregate_load_budget_rejects_many_small_object_streams() {
        let bytes = object_stream_pdf();
        let object_stream_count = bytes
            .windows(b"/ObjStm".len())
            .filter(|window| *window == b"/ObjStm")
            .count();
        assert!(
            object_stream_count >= 2,
            "fixture must contain multiple individually-small object streams"
        );

        let mut limits = PdfInspectLimits::for_input_bytes(bytes.len());
        limits.max_decompressed_stream_bytes = 1024;
        limits.max_decompressed_page_bytes = 1024;
        // One reservation is consumed by the xref stream and one by the first
        // object stream. The next otherwise-valid object stream must fail
        // before it can allocate its decoded body.
        limits.max_decompressed_total_bytes = 2 * 1024;
        let error = inspect_pdf_with_limits(&bytes, &limits)
            .expect_err("aggregate eager-load budget must be enforced");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "aggregate decompression"
            }
        ));
    }

    #[test]
    fn pdf_object_budget_rejects_before_page_or_text_work() {
        let bytes = object_stream_pdf();
        let mut limits = PdfInspectLimits::default();
        limits.max_objects = 1;
        let error = inspect_pdf_with_limits(&bytes, &limits)
            .expect_err("object budget must be enforced before inspection");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "object count"
            }
        ));
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn encrypted_pdf_is_explicitly_unsupported_without_password_probe() {
        use lopdf::{Document, EncryptionState, EncryptionVersion, Object, Permissions};

        let mut document = Document::load_mem(&text_pdf("secret", 1)).expect("load test PDF");
        document.trailer.set(
            "ID",
            Object::Array(vec![
                Object::string_literal(vec![1_u8; 16]),
                Object::string_literal(vec![2_u8; 16]),
            ]),
        );
        let state = EncryptionState::try_from(EncryptionVersion::V2 {
            document: &document,
            owner_password: "owner-password",
            user_password: "user-password",
            key_length: 128,
            permissions: Permissions::all(),
        })
        .expect("create encryption state");
        document.encrypt(&state).expect("encrypt test PDF");
        let mut bytes = Vec::new();
        document
            .save_to(&mut bytes)
            .expect("save encrypted test PDF");

        let error = inspect_pdf(&bytes).expect_err("encrypted PDFs must be unsupported");
        assert!(matches!(error, InspectError::EncryptedPdf));
    }

    #[test]
    fn pdf_aggregate_page_budget_rejects_many_individually_small_pages() {
        let mut limits = PdfInspectLimits::default();
        limits.max_decompressed_stream_bytes = 1024;
        limits.max_decompressed_page_bytes = 1024;
        limits.max_decompressed_total_bytes = 2 * 1024;
        let error = extract_pdf_text_with_limits(&text_pdf("page", 3), &limits)
            .expect_err("aggregate page decode budget must be enforced");
        assert!(matches!(
            error,
            InspectError::PdfLimitExceeded {
                limit: "aggregate decompression"
            }
        ));
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
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::DEFAULT)
            .compression_method(zip::CompressionMethod::Deflated);

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
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::DEFAULT)
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("ppt/slides/slide1.xml", options)
            .expect("start slide1.xml");
        zip.write_all(slide.as_bytes()).expect("write slide1.xml");

        zip.finish().expect("finish zip");
        cursor.into_inner()
    }

    /// Build a PPTX whose slide parts carry exactly the given numbers, written
    /// to the archive in the order supplied.
    #[expect(clippy::expect_used, reason = "test fixture construction")]
    fn pptx_with_numbered_slides(slides: &[(u32, &str)]) -> Vec<u8> {
        use std::io::Write;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::DEFAULT)
            .compression_method(zip::CompressionMethod::Deflated);

        for (number, text) in slides {
            let slide = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp><p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#
            );
            let name = format!("ppt/slides/slide{number}.xml");
            zip.start_file(&name, options).expect("start slide part");
            zip.write_all(slide.as_bytes()).expect("write slide part");
        }

        zip.finish().expect("finish zip");
        cursor.into_inner()
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pptx_reads_slides_past_a_numbering_gap() {
        // WHY: deleting a slide in PowerPoint leaves the surviving part names
        // unrenumbered, so slide3 can be absent while slide4 exists. Probing
        // upward from slide1 and stopping at the first missing index returned
        // only the first two slides and silently dropped the rest.
        let bytes = pptx_with_numbered_slides(&[(1, "first"), (2, "second"), (4, "fourth")]);
        let summary = inspect_pptx(&bytes).expect("inspect must succeed");

        assert_eq!(
            summary.slides,
            vec!["first".to_owned(), "second".to_owned(), "fourth".to_owned()],
            "every slide part must be read, including those past a numbering gap"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pptx_orders_slides_by_number_not_archive_order() {
        // WHY: ZIP entry order is arbitrary, so slide order must come from the
        // part number rather than the order the parts happen to be stored in.
        let bytes = pptx_with_numbered_slides(&[(3, "third"), (1, "first"), (2, "second")]);
        let summary = inspect_pptx(&bytes).expect("inspect must succeed");

        assert_eq!(
            summary.slides,
            vec!["first".to_owned(), "second".to_owned(), "third".to_owned()],
            "slides must be ordered by slide number"
        );
    }

    #[test]
    #[expect(clippy::expect_used, reason = "test assertions")]
    fn inspect_pptx_ignores_non_slide_parts() {
        // WHY: sibling prefixes under `ppt/` hold template parts whose names also
        // begin with `slide`, and `_rels` entries repeat the slide part names.
        // Only the exact `ppt/slides/slideN.xml` shape is a slide.
        use std::io::Write;
        use zip::ZipWriter;
        use zip::write::SimpleFileOptions;

        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(&mut cursor);
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::DEFAULT)
            .compression_method(zip::CompressionMethod::Deflated);
        for name in [
            "ppt/slideLayouts/slideLayout1.xml",
            "ppt/slideMasters/slideMaster1.xml",
            "ppt/slides/_rels/slide1.xml.rels",
        ] {
            zip.start_file(name, options).expect("start part");
            zip.write_all(b"<x/>").expect("write part");
        }
        zip.finish().expect("finish zip");
        let bytes = cursor.into_inner();

        let summary = inspect_pptx(&bytes).expect("inspect must succeed");
        assert!(
            summary.slides.is_empty(),
            "non-slide parts must not be counted, got: {:?}",
            summary.slides
        );
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
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::DEFAULT)
            .compression_method(zip::CompressionMethod::Deflated);

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
