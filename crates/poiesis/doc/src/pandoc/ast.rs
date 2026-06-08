//! Document → Pandoc JSON AST serialization.
//!
//! Converts `poiesis_core::Document` to the Pandoc native JSON format
//! (`pandoc -f json`). The mapping is near-identity per B-006 § 1, with the
//! typed note/cite/math/raw-block forms carried directly into the AST.

use poiesis_core::{Block, Document, NoteKind, RichText, Span};
use serde_json::{Value, json};

/// Serialize a `Document` to Pandoc JSON AST bytes.
///
/// The emitted JSON matches Pandoc's `--from json` input format.
/// `pandoc-api-version` is pinned to `[1, 23, 1, 1]` for Pandoc 3.7.x.
pub fn document_to_pandoc_json(doc: &Document) -> Vec<u8> {
    let blocks: Vec<Value> = doc.content.iter().map(block_to_pandoc).collect();

    let ast = json!({
        "pandoc-api-version": [1, 23, 1, 1],
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
        Block::Note(note) => {
            json!({
                "t": "Div",
                "c": [
                    ["", ["apx-note"], [["data-kind", note_kind_to_attr(note.kind)]]],
                    [{
                        "t": "Para",
                        "c": inlines_to_pandoc(&note.body)
                    }]
                ]
            })
        }
        Block::DisplayMath(expr) => {
            json!({
                "t": "Para",
                "c": [{
                    "t": "Math",
                    "c": [{"t": "DisplayMath"}, expr]
                }]
            })
        }
        Block::RawBlock { format, content } => {
            json!({
                "t": "RawBlock",
                "c": [format, content]
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
        Span::Cite(fact_id) => json!({
            "t": "Span",
            "c": [
                ["", ["apx-cite"], [["data-factid", fact_id]]],
                []
            ]
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

fn note_kind_to_attr(kind: NoteKind) -> &'static str {
    kind.as_str()
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-good JSON"
)]
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
        assert_eq!(v["pandoc-api-version"], json!([1, 23, 1, 1]));
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

    #[test]
    fn note_maps_to_div_with_kind_attr() {
        use poiesis_core::{Block, Document, Metadata, Note, NoteKind, RichText, Span};

        let doc = Document {
            metadata: Metadata {
                title: "Test".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::Note(Note {
                kind: NoteKind::Warning,
                body: RichText {
                    spans: vec![Span::Plain("Mind the gap.".to_owned())],
                },
            })],
        };

        let bytes = document_to_pandoc_json(&doc);
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
        assert_eq!(v["blocks"][0]["t"], "Div");
        assert_eq!(
            v["blocks"][0]["c"][0],
            json!(["", ["apx-note"], [["data-kind", "warning"]]])
        );
        assert_eq!(v["blocks"][0]["c"][1][0]["t"], "Para");
    }

    #[test]
    fn cite_maps_to_span_with_factid_attr() {
        use poiesis_core::{Block, Document, Metadata, RichText, Span};

        let doc = Document {
            metadata: Metadata {
                title: "Test".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::Paragraph(RichText {
                spans: vec![Span::Cite("fact-42".to_owned())],
            })],
        };

        let bytes = document_to_pandoc_json(&doc);
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
        assert_eq!(v["blocks"][0]["t"], "Para");
        assert_eq!(v["blocks"][0]["c"][0]["t"], "Span");
        assert_eq!(
            v["blocks"][0]["c"][0]["c"][0],
            json!(["", ["apx-cite"], [["data-factid", "fact-42"]]])
        );
        assert_eq!(v["blocks"][0]["c"][0]["c"][1], json!([]));
    }

    #[test]
    fn display_math_maps_to_math_inline() {
        use poiesis_core::{Block, Document, Metadata};

        let doc = Document {
            metadata: Metadata {
                title: "Test".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::DisplayMath("x^2 + y^2 = z^2".to_owned())],
        };

        let bytes = document_to_pandoc_json(&doc);
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
        assert_eq!(v["blocks"][0]["t"], "Para");
        assert_eq!(v["blocks"][0]["c"][0]["t"], "Math");
        assert_eq!(v["blocks"][0]["c"][0]["c"][0]["t"], "DisplayMath");
        assert_eq!(v["blocks"][0]["c"][0]["c"][1], "x^2 + y^2 = z^2");
    }

    #[test]
    fn raw_block_maps_to_rawblock() {
        use poiesis_core::{Block, Document, Metadata};

        let doc = Document {
            metadata: Metadata {
                title: "Test".to_owned(),
                author: None,
                created: None,
            },
            content: vec![Block::RawBlock {
                format: "latex".to_owned(),
                content: "\\alpha".to_owned(),
            }],
        };

        let bytes = document_to_pandoc_json(&doc);
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
        assert_eq!(v["blocks"][0]["t"], "RawBlock");
        assert_eq!(v["blocks"][0]["c"], json!(["latex", "\\alpha"]));
    }
}
