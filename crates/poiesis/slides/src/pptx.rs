//! PPTX rendering backend using the `ppt-rs` crate.
//!
//! The document content is mapped to slides using these rules:
//! - A new [`SlideContent`] is started for each Heading block.
//! - Paragraph and List blocks append bullet points to the current slide.
//! - Table blocks are summarized as bullet points (one per row) since ppt-rs
//!   table support requires pre-built table objects.
//! - [`PageBreak`] forces a new slide even without a heading.
//!
//! If the document has no headings, all content lands on a single slide
//! titled with the document metadata title.
//!
//! [`SlideContent`]: ppt_rs::SlideContent
//! [`PageBreak`]: poiesis_core::Block::PageBreak

use poiesis_core::{Block, Document, RichText, Renderer};
use ppt_rs::{SlideContent, create_pptx_with_content};
use snafu::Snafu;

/// Errors produced by the PPTX renderer.
#[derive(Debug, Snafu)]
pub enum PptxError {
    /// `ppt-rs` returned an error while generating the presentation.
    #[snafu(display("PPTX error: {message}"))]
    Pptx {
        /// Human-readable error description.
        message: String,
    },
}

/// Renders a [`Document`] to a PPTX byte vector.
///
/// Each top-level [`Block::Heading`] starts a new slide. Other blocks
/// are appended to the current slide as bullet points.
pub struct PptxRenderer;

impl PptxRenderer {
    /// Construct a new `PptxRenderer`.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PptxRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for PptxRenderer {
    type Error = PptxError;

    fn format(&self) -> &'static str {
        "pptx"
    }

    fn render(&self, doc: &Document) -> Result<Vec<u8>, Self::Error> {
        let mut slides: Vec<SlideContent> = Vec::new();

        // Start with a title slide using the document metadata.
        let mut current_slide = SlideContent::new(&doc.metadata.title);

        for block in &doc.content {
            match block {
                Block::Heading { level: _, text } => {
                    // Flush the current slide (if it has any content) and start a new one.
                    slides.push(current_slide);
                    current_slide = SlideContent::new(&text.plain_text());
                }
                Block::Paragraph(rt) => {
                    current_slide =
                        current_slide.add_bullet(&rt.plain_text());
                }
                Block::List { ordered, items } => {
                    for (i, item) in items.iter().enumerate() {
                        let text = if *ordered {
                            format!("{}. {}", i + 1, item.content.plain_text())
                        } else {
                            item.content.plain_text()
                        };
                        current_slide = current_slide.add_bullet(&text);
                    }
                }
                Block::Table(table) => {
                    // Summarize table as header bullet + one bullet per row.
                    let header = table.headers.join(" | ");
                    current_slide = current_slide.add_bullet(&header);
                    for row in &table.rows {
                        let cells: Vec<String> = row.iter().map(RichText::plain_text).collect();
                        current_slide = current_slide.add_bullet(&cells.join(" | "));
                    }
                }
                Block::Image(img) => {
                    current_slide =
                        current_slide.add_bullet(&format!("[Image: {}]", img.alt));
                }
                Block::PageBreak => {
                    slides.push(current_slide);
                    // New untitled slide — title is empty string until next Heading.
                    current_slide = SlideContent::new("");
                }
            }
        }

        slides.push(current_slide);

        // ppt-rs requires at least one slide.
        if slides.is_empty() {
            slides.push(SlideContent::new(&doc.metadata.title));
        }

        create_pptx_with_content(&doc.metadata.title, slides).map_err(|e| PptxError::Pptx {
            message: e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poiesis_core::{Block, Document, Metadata, RichText, Span};

    fn sample_doc() -> Document {
        Document {
            metadata: Metadata {
                title: "PPTX Test Presentation".to_owned(),
                author: None,
                created: None,
            },
            content: vec![
                Block::Heading {
                    level: 1,
                    text: RichText {
                        spans: vec![Span::Plain("Introduction".to_owned())],
                    },
                },
                Block::Paragraph(RichText {
                    spans: vec![Span::Plain("Welcome to the presentation.".to_owned())],
                }),
                Block::List {
                    ordered: false,
                    items: vec![
                        poiesis_core::block::ListItem {
                            content: RichText {
                                spans: vec![Span::Plain("Point A".to_owned())],
                            },
                        },
                        poiesis_core::block::ListItem {
                            content: RichText {
                                spans: vec![Span::Plain("Point B".to_owned())],
                            },
                        },
                    ],
                },
            ],
        }
    }

    #[test]
    fn pptx_produces_nonempty_bytes() {
        let r = PptxRenderer::new();
        let bytes = r.render(&sample_doc()).expect("PPTX render failed");
        assert!(!bytes.is_empty(), "rendered PPTX must not be empty");
    }

    #[test]
    fn pptx_starts_with_pk_magic() {
        // WHY: PPTX is a ZIP/OOXML archive; valid files start with PK (0x50 0x4B).
        let r = PptxRenderer::new();
        let bytes = r.render(&sample_doc()).expect("PPTX render failed");
        assert_eq!(&bytes[..2], b"PK", "PPTX output should be a valid ZIP");
    }
}
