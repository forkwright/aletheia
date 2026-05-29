#![deny(missing_docs)]
//! poiesis-doc: DOCX write, inspect, and Pandoc-dispatch backend for poiesis.
//!
//! This crate provides:
//!
//! - [`render_docx`] — build a `.docx` file from a JSON descriptor.
//! - [`inspect_docx`] — extract paragraph text from an existing `.docx` file.
//! - [`pandoc`] — Pandoc subprocess wrapper, AST serialization, and unified
//!   dispatch (`render_doc`) for the full format matrix (B-012).
//!
//! ## Pandoc dispatch (B-012)
//!
//! [`pandoc::render_doc`] routes by format:
//! - PDF → `poiesis-typst` in-process fast-lane (default).
//! - PDF with explicit `LaTeX` engine → Pandoc + `LaTeX`.
//! - docx / odt / md / latex / html / epub → Pandoc subprocess.
//!
//! # GPL-clean boundary
//!
//! `pandoc` is invoked as a subprocess only. This crate never links against
//! the `pandoc` Rust crate.

mod error;
mod pandoc_probe;
mod typst_bridge;

/// Pandoc subprocess wrapper, AST serialization, and format dispatch (B-012).
pub mod pandoc;

pub use error::Error;
pub use pandoc_probe::{PandocProbe, PandocProbeError};

use serde_json::Value;
use snafu::ResultExt;
use tracing::instrument;

/// Result alias for poiesis-doc operations.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Summary of a DOCX file produced by [`inspect_docx`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct DocxSummary {
    /// Plain-text content of each paragraph, in document order.
    pub paragraphs: Vec<String>,
}

impl DocxSummary {
    /// Create a new summary with the given paragraphs.
    #[must_use]
    pub fn new(paragraphs: Vec<String>) -> Self {
        Self { paragraphs }
    }
}

/// Render a JSON descriptor to DOCX bytes.
///
/// See the crate-level documentation for the expected JSON schema.
///
/// # Errors
///
/// Returns [`Error::MalformedInput`] if `data` does not match the schema,
/// or [`Error::BuildDocx`] if the DOCX encoder fails.
#[instrument(skip(data))]
pub fn render_docx(data: &Value) -> Result<Vec<u8>> {
    let paragraphs = data
        .get("paragraphs")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::MalformedInput {
            detail: "missing or non-array 'paragraphs' field".to_owned(),
        })?;

    let mut docx = docx_rs::Docx::new();

    if let Some(title) = data.get("title").and_then(Value::as_str) {
        let para = docx_rs::Paragraph::new()
            .add_run(docx_rs::Run::new().add_text(title))
            .style("Title");
        docx = docx.add_paragraph(para);
    }

    for para_value in paragraphs {
        let text = para_value
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::MalformedInput {
                detail: "paragraph missing 'text' field".to_owned(),
            })?;

        let mut para = docx_rs::Paragraph::new().add_run(docx_rs::Run::new().add_text(text));

        if let Some(style) = para_value.get("style").and_then(Value::as_str) {
            para = para.style(style);
        }

        docx = docx.add_paragraph(para);
    }

    let xml_docx = docx.build();
    let mut buf = std::io::Cursor::new(Vec::new());
    xml_docx.pack(&mut buf).map_err(|e| Error::BuildDocx {
        detail: e.to_string(),
    })?;
    Ok(buf.into_inner())
}

/// Inspect an in-memory DOCX file and extract paragraph text.
///
/// Reads `word/document.xml` from the ZIP archive and collects the
/// concatenated text inside each `<w:t>` element, grouping by `<w:p>`.
///
/// # Errors
///
/// Returns [`Error::ReadZip`] if the bytes are not a valid ZIP archive,
/// or [`Error::ParseXml`] if `document.xml` cannot be parsed.
pub fn inspect_docx(bytes: &[u8]) -> Result<DocxSummary> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).context(error::ReadZipSnafu)?;

    let mut document_xml = archive
        .by_name("word/document.xml")
        .map_err(|e| Error::ReadZip { source: e })?;

    let mut xml_bytes = Vec::new();
    std::io::Read::read_to_end(&mut document_xml, &mut xml_bytes).map_err(|e| Error::ReadZip {
        source: zip::result::ZipError::Io(e),
    })?;

    let mut reader = quick_xml::Reader::from_reader(xml_bytes.as_slice());
    reader.config_mut().trim_text(true);

    let mut paragraphs: Vec<String> = Vec::new();
    let mut current_text = String::new();
    let mut in_paragraph = false;
    let mut in_text = false;

    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(e)) => {
                let name = e.name();
                let local_name = name.local_name();
                let local = local_name.as_ref();
                if local == b"p" {
                    in_paragraph = true;
                    current_text.clear();
                } else if local == b"t" {
                    in_text = true;
                }
            }
            Ok(quick_xml::events::Event::Text(e)) if in_text => {
                let txt = e
                    .decode()
                    .map_err(|e| Error::ParseXml { source: e.into() })?;
                current_text.push_str(&txt);
            }
            Ok(quick_xml::events::Event::End(e)) => {
                let name = e.name();
                let local_name = name.local_name();
                let local = local_name.as_ref();
                if local == b"p" {
                    if in_paragraph {
                        paragraphs.push(current_text.clone());
                    }
                    in_paragraph = false;
                    current_text.clear();
                } else if local == b"t" {
                    in_text = false;
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(e) => {
                return Err(Error::ParseXml { source: e });
            }
            _ => (), // kanon:ignore RUST/empty-match-arm — intentional no-op for unmatched XML events; kanon:ignore RUST/silent-wildcard-success — non-paragraph XML events are intentionally skipped
        }
    }

    Ok(DocxSummary::new(paragraphs))
}

/// Render a [`poiesis_core::Document`] to PDF bytes via the embedded Typst compiler.
///
/// Intermediate replacement for the retired text-format PDF backend.
/// The Pandoc-backed path (B-012) will supersede this for the full format matrix.
///
/// # Errors
///
/// Returns [`Error::PdfRenderFailed`] if Typst compilation fails.
#[instrument(skip(doc))]
pub fn render_pdf_from_doc(doc: &poiesis_core::Document) -> Result<Vec<u8>> {
    let source = typst_bridge::doc_to_typst(doc);
    poiesis_typst::render_typst(&source, &serde_json::json!({})).map_err(|e| {
        Error::PdfRenderFailed {
            detail: e.to_string(),
        }
    })
}

/// Render a [`poiesis_core::Document`] to ODT bytes.
///
/// ODT output requires the Pandoc backend (B-012) which has not landed yet.
/// Returns [`Error::PandocRequired`] until B-012 ships.
///
/// # Errors
///
/// Always returns [`Error::PandocRequired`] in this stub implementation.
pub fn render_odt_from_doc(_doc: &poiesis_core::Document) -> Result<Vec<u8>> {
    Err(Error::PandocRequired {
        format: "odt".to_owned(),
    })
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn render_docx_simple() {
        let data = serde_json::json!({
            "paragraphs": [
                { "text": "Hello, world!" }
            ]
        });
        let bytes = render_docx(&data).expect("render must succeed");
        assert!(
            bytes.starts_with(b"PK"),
            "output must be a ZIP/DOCX archive"
        );

        let summary = inspect_docx(&bytes).expect("inspect must succeed");
        assert_eq!(summary.paragraphs, vec!["Hello, world!"]);
    }

    #[test]
    fn render_docx_multi_paragraph() {
        let data = serde_json::json!({
            "title": "My Report",
            "paragraphs": [
                { "text": "First paragraph.", "style": "Heading1" },
                { "text": "Second paragraph." },
                { "text": "Third paragraph." }
            ]
        });
        let bytes = render_docx(&data).expect("render must succeed");

        let summary = inspect_docx(&bytes).expect("inspect must succeed");
        assert_eq!(
            summary.paragraphs,
            vec![
                "My Report",
                "First paragraph.",
                "Second paragraph.",
                "Third paragraph."
            ]
        );
    }

    #[test]
    fn inspect_docx_extracts_text() {
        // Build a known-good DOCX via render and re-inspect it.
        let data = serde_json::json!({
            "paragraphs": [
                { "text": "Alpha" },
                { "text": "Beta" }
            ]
        });
        let bytes = render_docx(&data).expect("render must succeed");
        let summary = inspect_docx(&bytes).expect("inspect must succeed");
        assert_eq!(summary.paragraphs, vec!["Alpha", "Beta"]);
    }

    #[test]
    fn render_docx_malformed_json_errors() {
        let data = serde_json::json!({ "paragraphs": "not-an-array" });
        let err = render_docx(&data).expect_err("must error");
        assert!(
            matches!(err, Error::MalformedInput { .. }),
            "expected MalformedInput, got: {err:?}"
        );

        let data = serde_json::json!({ "paragraphs": [{ "style": "Heading1" }] });
        let err = render_docx(&data).expect_err("must error");
        assert!(
            matches!(err, Error::MalformedInput { .. }),
            "expected MalformedInput, got: {err:?}"
        );
    }

    #[test]
    fn render_pdf_from_doc_produces_bytes() {
        use poiesis_core::{Block, Document, Metadata, RichText};
        let doc = Document {
            metadata: Metadata {
                title: "Test".to_owned(),
                author: None,
                created: None,
            },
            content: vec![
                Block::Heading {
                    level: 1,
                    text: RichText::from("Section"),
                },
                Block::Paragraph(RichText::from("Content.")),
            ],
        };
        let bytes = render_pdf_from_doc(&doc).expect("must render");
        assert!(bytes.starts_with(b"%PDF"), "must be a PDF");
    }

    #[test]
    fn render_odt_from_doc_returns_error() {
        use poiesis_core::{Document, Metadata};
        let doc = Document {
            metadata: Metadata {
                title: "Test".to_owned(),
                author: None,
                created: None,
            },
            content: vec![],
        };
        let err = render_odt_from_doc(&doc).expect_err("must fail");
        assert!(matches!(err, Error::PandocRequired { .. }));
    }
}
