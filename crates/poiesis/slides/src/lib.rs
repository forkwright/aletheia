#![deny(missing_docs)]
//! poiesis-slides: PPTX presentation rendering backend.
//!
//! This crate provides both a traditional [`Document`]-centric API via
//! [`PptxRenderer`] and a new JSON-first API via [`render_pptx`] and
//! [`inspect_pptx`].
//!
//! ## Decision: keep hand-rolled emitter
//!
//! The v1 JSON-first wrapper intentionally continues to use the existing
//! hand-rolled ZIP/XML emitter rather than replacing it with `ppt-rs`.
//! Migration to `ppt-rs` (or a fork) is tracked separately in #3445; that
//! work is out of scope here so that this issue can land quickly without
//! adding a new heavy dependency or destabilising the current output.
//!
//! Feature flags:
//! - `pptx` (default): `PowerPoint` PPTX output via hand-rolled ZIP/XML emitter.

#[cfg(feature = "pptx")]
pub mod pptx;

#[cfg(feature = "pptx")]
mod error;

#[cfg(feature = "pptx")]
pub use pptx::PptxRenderer;

#[cfg(feature = "pptx")]
pub use error::Error;

#[cfg(feature = "pptx")]
use poiesis_core::{Block, Document, ListItem, Metadata, Renderer, RichText, Span};
#[cfg(feature = "pptx")]
use serde_json::Value;
#[cfg(feature = "pptx")]
use tracing::instrument;

/// Result alias for poiesis-slides operations.
#[cfg(feature = "pptx")]
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Summary of a single slide extracted from an existing PPTX file.
#[cfg(feature = "pptx")]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SlideSummary {
    /// Slide title (first text run in the slide, typically the title placeholder).
    pub title: String,
    /// Bullet text extracted from the slide body.
    pub bullets: Vec<String>,
}

/// Summary of a complete presentation extracted from PPTX bytes.
#[cfg(feature = "pptx")]
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PresentationSummary {
    /// Per-slide summaries, ordered by slide position.
    pub slides: Vec<SlideSummary>,
    /// Total number of slides.
    pub slide_count: usize,
}

/// Render a JSON slide descriptor to PPTX bytes.
///
/// The expected JSON schema is:
/// ```json
/// {
///   "slides": [
///     {
///       "layout": "titleAndContent",
///       "title": "Slide Title",
///       "content": [
///         { "text": "A paragraph." },
///         { "bullets": ["First bullet", "Second bullet"] }
///       ]
///     }
///   ]
/// }
/// ```
///
/// `layout` is optional and reserved for future use.
/// Each item in `content` may contain `text` (a paragraph) and/or `bullets`
/// (an unordered list).  The JSON is translated into a [`Document`] and
/// rendered through the existing hand-rolled emitter.
///
/// # Errors
///
/// Returns [`Error::InvalidJson`] if the JSON does not conform to the
/// expected schema, or [`Error::Render`] if the hand-rolled emitter fails.
#[cfg(feature = "pptx")]
#[instrument(skip_all)]
pub fn render_pptx(data: &Value) -> Result<Vec<u8>> {
    let slides = data
        .get("slides")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::InvalidJson {
            message: "missing top-level 'slides' array".to_owned(),
        })?;

    if slides.is_empty() {
        return Err(Error::InvalidJson {
            message: "'slides' array must contain at least one slide".to_owned(),
        });
    }

    let mut blocks: Vec<Block> = Vec::new();
    let mut first_title: Option<String> = None;

    for (i, slide) in slides.iter().enumerate() {
        let title = slide.get("title").and_then(Value::as_str).unwrap_or("");

        if i == 0 {
            first_title = Some(title.to_owned());
        } else {
            blocks.push(Block::Heading {
                level: 1,
                text: plain(title),
            });
        }

        if let Some(content) = slide.get("content").and_then(Value::as_array) {
            for item in content {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    blocks.push(Block::Paragraph(plain(text)));
                }
                if let Some(bullets) = item.get("bullets").and_then(Value::as_array) {
                    let items: Vec<ListItem> = bullets
                        .iter()
                        .filter_map(|b| b.as_str())
                        .map(|s| ListItem { content: plain(s) })
                        .collect();
                    blocks.push(Block::List {
                        ordered: false,
                        items,
                    });
                }
            }
        }
    }

    let doc = Document {
        metadata: Metadata {
            title: first_title.unwrap_or_default(), // kanon:ignore RUST/no-result-unwrap-or-default — Option<String> default is empty string; missing title is semantically default
            author: None,
            created: None,
        },
        content: blocks,
    };

    let renderer = PptxRenderer::new();
    renderer
        .render(&doc)
        .map_err(|source| Error::Render { source })
}

/// Inspect an existing PPTX file and extract text per slide.
///
/// Uses the `zip` crate to read the OOXML archive and `quick-xml` to pull
/// text runs (`<a:t>`) from each slide part.  The first text run in a slide
/// is treated as the title; all subsequent runs are treated as bullets.
///
/// # Errors
///
/// Returns [`Error::ZipRead`] if the bytes are not a valid ZIP archive, or
/// [`Error::XmlParse`] if a slide XML part cannot be parsed.
#[cfg(feature = "pptx")]
#[instrument(skip_all, fields(bytes_len = bytes.len()))]
pub fn inspect_pptx(bytes: &[u8]) -> Result<PresentationSummary> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| Error::ZipRead {
        message: e.to_string(),
    })?;

    let mut slide_names: Vec<String> = Vec::new();
    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|e| Error::ZipRead {
            message: e.to_string(),
        })?;
        let name = file.name();
        if name.starts_with("ppt/slides/slide")
            && std::path::Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("xml"))
        {
            slide_names.push(name.to_owned());
        }
    }

    // WHY: ZIP entry order is arbitrary — sort by slide number so the
    // summary order is deterministic.
    slide_names.sort_by(|a, b| {
        let num_a = extract_slide_number(a).unwrap_or(0);
        let num_b = extract_slide_number(b).unwrap_or(0);
        num_a.cmp(&num_b)
    });

    let mut summaries: Vec<SlideSummary> = Vec::with_capacity(slide_names.len());

    for name in &slide_names {
        let mut file = archive.by_name(name).map_err(|e| Error::ZipRead {
            message: e.to_string(),
        })?;
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut file, &mut xml).map_err(|e| Error::ZipRead {
            message: e.to_string(),
        })?;

        let texts = extract_text_runs(&xml)?;
        let (title, bullets) = if texts.is_empty() {
            (String::new(), Vec::new())
        } else {
            let mut iter = texts.into_iter();
            let t = iter.next().unwrap_or_default(); // kanon:ignore RUST/no-result-unwrap-or-default — Option<String> default is empty string; missing title is semantically empty
            (t, iter.collect())
        };

        summaries.push(SlideSummary { title, bullets });
    }

    let slide_count = summaries.len();
    Ok(PresentationSummary {
        slides: summaries,
        slide_count,
    })
}

// ── Helpers ──

#[cfg(feature = "pptx")]
fn plain(s: &str) -> RichText {
    RichText {
        spans: vec![Span::Plain(s.to_owned())],
    }
}

#[cfg(feature = "pptx")]
fn extract_slide_number(name: &str) -> Option<usize> {
    let stem = name.strip_prefix("ppt/slides/slide")?;
    let num_str = stem.strip_suffix(".xml")?;
    num_str.parse().ok()
}

#[cfg(feature = "pptx")]
fn extract_text_runs(xml: &str) -> Result<Vec<String>> {
    use quick_xml::escape::unescape;
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut texts: Vec<String> = Vec::new();
    let mut buf = Vec::new();
    let mut current = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"a:t" => {
                current.clear();
            }
            Ok(Event::Text(e)) => {
                if let Ok(decoded) = e.decode()
                    && let Ok(txt) = unescape(&decoded)
                {
                    current.push_str(&txt);
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"a:t" => {
                texts.push(current.clone());
                current.clear();
            }
            Ok(Event::Empty(e)) if e.name().as_ref() == b"a:t" => {
                texts.push(String::new());
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(Error::XmlParse {
                    message: e.to_string(),
                });
            }
            _ => (), // kanon:ignore RUST/empty-match-arm — intentional no-op for unmatched XML events
        }
        buf.clear();
    }

    Ok(texts)
}

#[cfg(all(test, feature = "pptx"))]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn render_pptx_simple_json() {
        let data = serde_json::json!({
            "slides": [
                {
                    "title": "Hello World",
                    "content": [
                        { "text": "Welcome to the presentation." },
                        { "bullets": ["Point A", "Point B"] }
                    ]
                }
            ]
        });
        let bytes = render_pptx(&data).expect("render must succeed");
        assert!(!bytes.is_empty(), "PPTX must not be empty");
        assert_eq!(
            bytes.get(..2).expect("at least 2 bytes"),
            b"PK",
            "PPTX must be a valid ZIP"
        );
    }

    #[test]
    fn render_pptx_multi_slide() {
        let data = serde_json::json!({
            "slides": [
                {
                    "title": "First Slide",
                    "content": [
                        { "text": "Opening remarks." }
                    ]
                },
                {
                    "title": "Second Slide",
                    "content": [
                        { "bullets": ["Bullet 1", "Bullet 2"] }
                    ]
                },
                {
                    "title": "Third Slide",
                    "content": [
                        { "text": "Conclusion." },
                        { "bullets": ["Takeaway 1"] }
                    ]
                }
            ]
        });
        let bytes = render_pptx(&data).expect("render must succeed");
        assert!(!bytes.is_empty());
        assert_eq!(bytes.get(..2).expect("at least 2 bytes"), b"PK");

        let summary = inspect_pptx(&bytes).expect("inspect must succeed");
        assert_eq!(summary.slide_count, 3, "must have 3 slides");
        assert_eq!(
            summary.slides.first().expect("first slide").title,
            "First Slide"
        );
        assert_eq!(
            summary.slides.get(1).expect("second slide").title,
            "Second Slide"
        );
        assert_eq!(
            summary.slides.get(2).expect("third slide").title,
            "Third Slide"
        );
    }

    #[test]
    fn inspect_pptx_extracts_text() {
        let data = serde_json::json!({
            "slides": [
                {
                    "title": "Inspection Test",
                    "content": [
                        { "bullets": ["Alpha", "Beta"] }
                    ]
                }
            ]
        });
        let bytes = render_pptx(&data).expect("render must succeed");
        let summary = inspect_pptx(&bytes).expect("inspect must succeed");

        assert_eq!(summary.slide_count, 1);
        assert_eq!(
            summary.slides.first().expect("first slide").title,
            "Inspection Test"
        );
        assert!(
            summary
                .slides
                .first()
                .expect("first slide")
                .bullets
                .contains(&"Alpha".to_owned())
        );
        assert!(
            summary
                .slides
                .first()
                .expect("first slide")
                .bullets
                .contains(&"Beta".to_owned())
        );
    }

    #[test]
    fn render_pptx_malformed_json_errors() {
        let data = serde_json::json!({ "not_slides": [] });
        let err = render_pptx(&data).expect_err("must error");
        assert!(
            matches!(err, Error::InvalidJson { .. }),
            "expected InvalidJson, got: {err:?}"
        );
    }

    #[test]
    fn render_pptx_empty_slides_errors() {
        let data = serde_json::json!({ "slides": [] });
        let err = render_pptx(&data).expect_err("must error");
        assert!(
            matches!(err, Error::InvalidJson { .. }),
            "expected InvalidJson for empty slides, got: {err:?}"
        );
    }

    #[test]
    fn inspect_pptx_invalid_zip_errors() {
        let err = inspect_pptx(b"not a zip file").expect_err("must error");
        assert!(
            matches!(err, Error::ZipRead { .. }),
            "expected ZipRead, got: {err:?}"
        );
    }
}
