//! The three body arms wrapped by an envelope: deck, document, workbook.
//!
//! Each body is the minimal data carrier the render-side B-NNN entries need.
//! Concrete layout decisions (`layout.json`, slot tables, EMU coordinate
//! math) live in those render entries — this module names the data only.

use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::ids::{ComponentId, FactId, SheetName};
use crate::scalar::{AspectRatio, Scalar, ScalarKind};

/// A deck composed of slides at a fixed aspect ratio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deck {
    /// The deck-wide aspect ratio (e.g. [`AspectRatio::WIDESCREEN_16_9`]).
    pub aspect: AspectRatio,
    /// Slides in display order.
    pub slides: Vec<Slide>,
}

/// One slide, parameterised by a component id and a field payload.
///
/// The `component` field names an entry in the [`crate::components::ComponentRegistry`];
/// the `fields` payload is validated against that component's schema at
/// parse time. `notes` is optional speaker notes as plain prose text
/// (rich-text upgrades belong to the render entries).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slide {
    /// The component pack this slide instantiates.
    pub component: ComponentId,
    /// The slide field payload; opaque to this crate aside from schema
    /// validation.
    pub fields: serde_json::Value,
    /// Optional speaker notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// A workbook of named sheets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workbook {
    /// Sheets in tab order.
    pub sheets: Vec<Sheet>,
}

/// One sheet of a workbook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    /// Sheet display name.
    pub name: SheetName,
    /// The header row.
    pub headers: Vec<String>,
    /// The data rows; each row must have `headers.len()` cells.
    pub rows: Vec<Vec<WorkbookCell>>,
    /// Per-column scalar kind; drives format-at-the-boundary in the
    /// rendering crate.
    pub column_types: Vec<ScalarKind>,
}

/// A workbook cell value.
///
/// Numeric cells MUST be `Cite` references into the factbase (this carries
/// the "no naked numbers" rule into the workbook body). Free-form labels
/// remain as `Lit(Scalar::Text)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkbookCell {
    /// A typed literal value (typically `Scalar::Text` for labels).
    Lit {
        /// The literal.
        value: Scalar,
    },
    /// A reference into [`crate::factbase::Factbase`] for any numeric value.
    Cite {
        /// The cited fact id.
        fact: FactId,
    },
}

/// The document body — a thin newtype wrapping the pre-envelope
/// [`Document`] so the envelope can carry it without breaking the legacy
/// `poiesis::Renderer::render(&Document)` path.
///
/// The Pandoc-aligned `Block` / `Inline` evolution named in the planning
/// doc is render-side work; it ships with [[B-006]] and gains a typed
/// `DocBlock` enum at that time.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentBody {
    /// The wrapped legacy document.
    pub document: Document,
}

impl DocumentBody {
    /// Wrap a legacy [`Document`] in the new body.
    #[must_use]
    pub fn new(document: Document) -> Self {
        Self { document }
    }
}

impl From<Document> for DocumentBody {
    fn from(document: Document) -> Self {
        Self::new(document)
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::ids::ComponentId;
    use serde_json::json;

    #[test]
    fn deck_round_trips_via_serde() {
        let d = Deck {
            aspect: AspectRatio::WIDESCREEN_16_9,
            slides: vec![Slide {
                component: ComponentId::new("title").unwrap(),
                fields: json!({"text": "Hello"}),
                notes: Some("speaker notes".to_owned()),
            }],
        };
        let s = serde_json::to_string(&d).unwrap();
        let back: Deck = serde_json::from_str(&s).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn workbook_cell_lit_serialises_with_kind_tag() {
        let cell = WorkbookCell::Lit {
            value: Scalar::Text {
                value: "Total".to_owned(),
            },
        };
        let s = serde_json::to_string(&cell).unwrap();
        assert!(s.contains("\"kind\":\"lit\""));
    }

    #[test]
    fn document_body_wraps_legacy_document() {
        let doc = Document::new("untitled");
        let body: DocumentBody = doc.into();
        assert_eq!(body.document.metadata.title, "untitled");
    }
}
