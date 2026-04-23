#![deny(missing_docs)]
//! poiesis-doc: DOCX write and inspect backend for poiesis.
//!
//! This crate provides a narrow, JSON-first DOCX pipeline:
//!
//! - [`render_docx`] — build a `.docx` file from a JSON descriptor.
//! - [`inspect_docx`] — extract paragraph text from an existing `.docx` file.
//!
//! ## Render schema (v1)
//!
//! ```json
//! {
//!   "title": "Optional Document Title",
//!   "paragraphs": [
//!     { "text": "First paragraph.", "style": "Heading1" },
//!     { "text": "Second paragraph." }
//!   ]
//! }
//! ```
//!
//! Only `paragraphs` (array of `{text, style?}`) is required. `style` is
//! passed through to the DOCX paragraph style ID; common values are
//! `Heading1`, `Heading2`, `Normal`, etc.

mod error;

pub use error::Error;

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

    let mut reader = quick_xml::Reader::from_reader(&xml_bytes[..]);
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
            _ => {}
        }
    }

    Ok(DocxSummary::new(paragraphs))
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
}
