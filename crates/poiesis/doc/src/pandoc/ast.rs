//! Document → Pandoc JSON AST serialization.
//!
//! Converts `poiesis_core::Document` to the Pandoc native JSON format
//! (`pandoc -f json`). The mapping is near-identity per B-006 § 1.
//!
//! # B-001 upgrade note
//!
//! This module currently serializes `poiesis_core::Document` (current types).
//! When B-001 lands (`DeliverableSpec`, `Body::Document`, `Cite(FactId)`),
//! the entry point will be upgraded to accept `DeliverableSpec` and the
//! `Cite` → `Span("apx-cite")` mapping will be wired. See ESCALATION.md Q1.

use poiesis_core::{Block, Document, RichText, Span};
use serde_json::{Value, json};

/// Serialize a `Document` to Pandoc JSON AST bytes.
///
/// The emitted JSON matches Pandoc's `--from json` input format.
/// `pandoc-api-version` is pinned to `[1, 24, 1]` (Pandoc 3.x series).
pub fn document_to_pandoc_json(doc: &Document) -> Vec<u8> {
    let blocks: Vec<Value> = doc.content.iter().map(block_to_pandoc).collect();

    let ast = json!({
        "pandoc-api-version": [1, 24, 1],
        "meta": build_meta(doc),
        "blocks": blocks
    });

    serde_json::to_vec(&ast).unwrap_or_else(|_| b"{}".to_vec())
}

fn build_meta(doc: &Document) -> Value {
    let mut meta = serde_json::Map::new();
    meta.insert(
        "title".to_owned(),
        json!({
            "t": "MetaInlines",
            "c": [{ "t": "Str", "c": doc.metadata.title }]
        }),
    );
    if let Some(author) = &doc.metadata.author {
        meta.insert(
            "author".to_owned(),
            json!({
                "t": "MetaList",
                "c": [{
                    "t": "MetaInlines",
                    "c": [{ "t": "Str", "c": author }]
                }]
            }),
        );
    }
    Value::Object(meta)
}

fn block_to_pandoc(block: &Block) -> Value {
    match block {
        Block::Heading { level, text } => {
            let level_val = u64::from((*level).clamp(1, 6));
            json!({
                "t": "Header",
                "c": [level_val, ["", [], []], inlines_to_pandoc(text)]
            })
        }
        Block::Paragraph(text) => {
            json!({
                "t": "Para",
                "c": inlines_to_pandoc(text)
            })
        }
        Block::PageBreak => {
            json!({ "t": "HorizontalRule" })
        }
        Block::Table(table) => {
            // Simple table: header row + data rows, one column per header.
            let col_count = table.headers.len();
            let col_spec: Vec<Value> = (0..col_count)
                .map(|_| json!([{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]))
                .collect();

            let header_cells: Vec<Value> = table
                .headers
                .iter()
                .map(|h| pandoc_cell(&json!([{"t": "Para", "c": [{"t": "Str", "c": h}]}])))
                .collect();

            let data_rows: Vec<Value> = table
                .rows
                .iter()
                .map(|row| {
                    let cells: Vec<Value> = row
                        .iter()
                        .map(|cell| {
                            pandoc_cell(&json!([{"t": "Para", "c": inlines_to_pandoc(cell)}]))
                        })
                        .collect();
                    json!([[], cells])
                })
                .collect();

            json!({
                "t": "Table",
                "c": [
                    ["", [], []],
                    {"t": "Caption", "c": [null, []]},
                    col_spec,
                    [["", [], []], [header_cells]],
                    [["", [], []], data_rows],
                    [["", [], []], []]
                ]
            })
        }
        Block::List { ordered, items } => {
            let inlines: Vec<Value> = items
                .iter()
                .map(|item| json!([{"t": "Plain", "c": inlines_to_pandoc(&item.content)}]))
                .collect();

            if *ordered {
                json!({
                    "t": "OrderedList",
                    "c": [[1, {"t": "DefaultStyle"}, {"t": "DefaultDelim"}], inlines]
                })
            } else {
                json!({ "t": "BulletList", "c": inlines })
            }
        }
        Block::Image(_) => {
            // Images not yet supported — emit a placeholder Para.
            // TODO(B-005/B-012): wire apx-figure.lua chart emitter.
            json!({
                "t": "Para",
                "c": [{"t": "Str", "c": "[image placeholder]"}]
            })
        }
    }
}

fn pandoc_cell(content: &Value) -> Value {
    json!([["", [], []], {"t": "AlignDefault"}, 1, 1, [content]])
}

fn inlines_to_pandoc(rt: &RichText) -> Vec<Value> {
    rt.spans.iter().map(span_to_pandoc).collect()
}

fn span_to_pandoc(span: &Span) -> Value {
    match span {
        Span::Plain(s) => json!({ "t": "Str", "c": s }),
        Span::Bold(s) => json!({
            "t": "Strong",
            "c": [{"t": "Str", "c": s}]
        }),
        Span::Italic(s) => json!({
            "t": "Emph",
            "c": [{"t": "Str", "c": s}]
        }),
        Span::Code(s) => json!({
            "t": "Code",
            "c": [["", [], []], s]
        }),
        Span::Link { text, url } => json!({
            "t": "Link",
            "c": [
                ["", [], []],
                [{"t": "Str", "c": text}],
                [url, ""]
            ]
        }),
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(clippy::indexing_slicing, reason = "test assertions on known-good JSON")]
mod tests {
    use super::*;
    use poiesis_core::{Block, Document, Metadata, RichText};

    fn minimal_doc() -> Document {
        Document {
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
        }
    }

    #[test]
    fn emits_valid_json() {
        let doc = minimal_doc();
        let bytes = document_to_pandoc_json(&doc);
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("must be valid JSON");
        assert_eq!(v["pandoc-api-version"], json!([1, 24, 1]));
        assert_eq!(v["blocks"][0]["t"], "Header");
        assert_eq!(v["blocks"][1]["t"], "Para");
    }

    #[test]
    fn title_in_meta() {
        let doc = minimal_doc();
        let bytes = document_to_pandoc_json(&doc);
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
        assert_eq!(v["meta"]["title"]["t"], "MetaInlines");
    }
}
