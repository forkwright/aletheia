#![deny(missing_docs)]
//! poiesis-core: typed deliverable envelope, factbase, and open component
//! registry — the model every poiesis render path consumes and the QA gate
//! runs over.
//!
//! Poiesis (ποίησις): "making, creation."
//!
//! # Substrate at a glance
//!
//! This crate exposes four layered surfaces:
//!
//! 1. **Identifiers and scalars** ([`ids`], [`scalar`]) — typed newtypes for
//!    every name and value the operator authors. Parse-don't-validate at the
//!    boundary; renderers and QA never see naked strings or floats.
//! 2. **Factbase** ([`factbase`]) — every cited number lives here, tagged
//!    with a [`scalar::Unit`] and a [`factbase::Source`]. The citation graph
//!    is DAG-resolved with cycle detection before any data adapter is
//!    invoked.
//! 3. **Open component registry** ([`components`]) — slide / block component
//!    packs discovered from a filesystem root. Adding a component means
//!    dropping a pack; no core recompile.
//! 4. **Envelope** ([`envelope`]) — a [`envelope::DeliverableSpec`] wraps
//!    typed [`envelope::Meta`], a [`ids::ThemeId`], a
//!    [`factbase::Factbase`], and a [`envelope::Body`] that selects between
//!    [`bodies::Deck`], [`bodies::DocumentBody`], or [`bodies::Workbook`].
//!
//! # Compatibility with the existing `Document` path
//!
//! The pre-envelope [`document::Document`] / [`block::Block`] /
//! [`renderer::Renderer`] surface is preserved verbatim for the in-tree
//! `organon` consumer. The new envelope wraps a [`document::Document`]
//! inside [`bodies::DocumentBody`], so no existing call site changes. The
//! render-side trait evolution to
//! `render(&DeliverableSpec, &ResolvedTheme) → Artifact` ships with the
//! render-side B-NNN entries; this crate carries the typed [`Artifact`]
//! shape only.

// New B-001 surface.
/// Typed bodies wrapping the existing [`document::Document`] and adding
/// [`bodies::Deck`] / [`bodies::Workbook`] siblings.
pub mod bodies;
/// Open component registry: discovery, schema-validated
/// [`components::ComponentDef`]s.
pub mod components;
/// The deliverable envelope: typed metadata, theme reference, factbase,
/// body.
pub mod envelope;
/// Error types raised at every parse-don't-validate boundary.
pub mod error;
/// The factbase: typed facts, sources, claims, and the DAG resolver.
pub mod factbase;
/// Typed identifier newtypes (component, theme, fact, claim, sheet, data
/// source).
pub mod ids;
/// Typed scalars, units, aspect ratio, tolerance.
pub mod scalar;

// Pre-existing surface retained verbatim for organon back-compat.
/// Block-level document elements: headings, paragraphs, tables, lists,
/// images.
pub mod block;
/// Top-level [`Document`] type assembling metadata and content blocks.
pub mod document;
/// Document metadata: title, author, creation timestamp.
pub mod metadata;
/// [`Renderer`] trait for format backends (legacy signature; render-side
/// B-NNN evolve this).
pub mod renderer;
/// Inline rich text spans: plain, bold, italic, code, links.
pub mod rich_text;

// Re-exports — pre-existing.
pub use block::{Block, Image, ListItem, Table};
pub use document::Document;
pub use metadata::Metadata;
pub use renderer::Renderer;
pub use rich_text::{RichText, Span};

// Re-exports — new B-001 surface.
pub use bodies::{Deck, DocumentBody, Sheet, Slide, Workbook, WorkbookCell};
pub use components::{ComponentDef, ComponentRegistry, TokenRef};
pub use envelope::{Body, DeliverableSpec, Meta};
pub use error::{FactbaseError, IdError, PoiesisError, RegistryError, ScalarError, SpecError};
pub use factbase::{
    Claim, DataSource, DataSourceRegistry, Expr, Fact, Factbase, Location, ResolvedFact, Source,
};
pub use ids::{ClaimId, ComponentId, DataSourceId, FactId, SheetName, ThemeId};
pub use scalar::{AspectRatio, Money, Scalar, ScalarKind, Tolerance, Unit};

/// The typed output of a renderer in the envelope-aware world.
///
/// `Artifact` carries the produced bytes plus the format identifier and the
/// concrete body kind the render produced. The pre-existing
/// [`Renderer::render`] still returns bare `Vec<u8>`; render-side B-NNN
/// entries (B-003 / B-004 / B-006 / B-007) evolve to producing `Artifact`
/// via a new trait that they own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// The format identifier, e.g. `"pdf"`, `"pptx"`, `"docx"`, `"xlsx"`,
    /// `"html"`.
    pub format: String,
    /// The body kind the artifact represents.
    pub body_kind: BodyKind,
    /// The complete byte payload.
    pub bytes: Vec<u8>,
}

/// The kind tag of a [`bodies`] arm; useful for runtime dispatch in the
/// renderer trait that B-003+ will introduce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyKind {
    /// A deck composed of [`bodies::Slide`]s.
    Deck,
    /// A prose document (pre-envelope [`document::Document`]).
    Document,
    /// A workbook of named [`bodies::Sheet`]s.
    Workbook,
}

impl BodyKind {
    /// Short canonical name used in errors and serialised form.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deck => "deck",
            Self::Document => "document",
            Self::Workbook => "workbook",
        }
    }
}

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
