#![deny(missing_docs)]
//! poiesis-core: format-agnostic document model and Renderer trait.
//!
//! Poiesis (ποίησις): "making, creation." The core crate defines a
//! format-agnostic document tree that all poiesis backends consume.
//! Backends implement the [`Renderer`] trait to produce format-specific bytes.

/// Document metadata: title, author, creation timestamp.
pub mod metadata;
/// Block-level document elements: headings, paragraphs, tables, lists, images.
pub mod block;
/// Inline rich text spans: plain, bold, italic, code, links.
pub mod rich_text;
/// Top-level [`Document`] type assembling metadata and content blocks.
pub mod document;
/// [`Renderer`] trait for format backends.
pub mod renderer;

pub use block::{Block, Image, ListItem, Table};
pub use document::Document;
pub use metadata::Metadata;
pub use renderer::Renderer;
pub use rich_text::{RichText, Span};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::Block;
    use crate::rich_text::{RichText, Span};

    fn sample_document() -> Document {
        Document {
            metadata: Metadata {
                title: "Test Document".to_owned(),
                author: Some("Alice".to_owned()),
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
                    spans: vec![
                        Span::Plain("Hello ".to_owned()),
                        Span::Bold("world".to_owned()),
                        Span::Plain(".".to_owned()),
                    ],
                }),
                Block::Table(crate::block::Table {
                    headers: vec!["Name".to_owned(), "Value".to_owned()],
                    rows: vec![vec![
                        RichText {
                            spans: vec![Span::Plain("alpha".to_owned())],
                        },
                        RichText {
                            spans: vec![Span::Plain("1".to_owned())],
                        },
                    ]],
                }),
                Block::List {
                    ordered: false,
                    items: vec![crate::block::ListItem {
                        content: RichText {
                            spans: vec![Span::Plain("item one".to_owned())],
                        },
                    }],
                },
                Block::PageBreak,
            ],
        }
    }

    #[test]
    fn document_round_trips_metadata() {
        let doc = sample_document();
        assert_eq!(doc.metadata.title, "Test Document");
        assert_eq!(doc.metadata.author.as_deref(), Some("Alice"));
        assert!(doc.metadata.created.is_none());
    }

    #[test]
    fn document_content_len() {
        let doc = sample_document();
        assert_eq!(doc.content.len(), 5);
    }

    #[test]
    fn richtext_plain_text() {
        let rt = RichText {
            spans: vec![
                Span::Plain("hello ".to_owned()),
                Span::Bold("world".to_owned()),
            ],
        };
        assert_eq!(rt.plain_text(), "hello world");
    }
}
