//! Document → Pandoc JSON AST serialization.
//!
//! Converts `poiesis_core::Document` to the Pandoc native JSON format
//! (`pandoc -f json`). The mapping is near-identity per B-006 § 1, with the
//! typed note/cite/math/raw-block forms carried directly into the AST.

use poiesis_core::{Block, Document, Image as DocImage, NoteKind, RichText, Span};
use serde_json::{Value, json};

use super::error::PandocError;

/// Serialize a `Document` to Pandoc JSON AST bytes.
///
/// The emitted JSON matches Pandoc's `--from json` input format.
/// `pandoc-api-version` is pinned to `[1, 23, 1, 1]` for Pandoc 3.7.x.
///
/// NOTE: the current AST is assembled entirely from `serde_json::Value`, whose
/// serialization is structurally valid by construction. The `Result` return
/// still preserves a traceable error path if that invariant changes.
pub(crate) fn document_to_pandoc_json(doc: &Document) -> Result<Vec<u8>, PandocError> {
    let mut figure_index = 0usize;
    let blocks: Vec<Value> = doc
        .content
        .iter()
        .map(|block| block_to_pandoc(block, &mut figure_index))
        .collect();

    let ast = json!({
        "pandoc-api-version": [1, 23, 1, 1],
        "meta": build_meta(doc),
        "blocks": blocks
    });

    serde_json::to_vec(&ast).map_err(|source| PandocError::AstSerialize { source })
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

fn block_to_pandoc(block: &Block, figure_index: &mut usize) -> Value {
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
            // WHY: Pandoc 1.23 Table AST shapes — Caption is a bare
            // [shortcaption, [Block]] array (not a tagged node), a Row is
            // [Attr, [Cell]], and a TableBody is [Attr, RowHeadColumns, [Row], [Row]].
            let col_count = table.headers.len();
            let col_spec: Vec<Value> = (0..col_count)
                .map(|_| json!([{"t": "AlignDefault"}, {"t": "ColWidthDefault"}]))
                .collect();

            let header_cells: Vec<Value> = table
                .headers
                .iter()
                .map(|h| pandoc_cell(&json!([{"t": "Para", "c": [{"t": "Str", "c": h}]}])))
                .collect();
            let head_row = json!([["", [], []], header_cells]);

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
                    json!([["", [], []], cells])
                })
                .collect();

            json!({
                "t": "Table",
                "c": [
                    ["", [], []],
                    [null, []],
                    col_spec,
                    [["", [], []], [head_row]],
                    [[["", [], []], 0, [], data_rows]],
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
        Block::Image(image) => image_to_pandoc(image, figure_index),
    }
}

fn image_to_pandoc(image: &DocImage, figure_index: &mut usize) -> Value {
    let figure_id = super::figure::figure_id(*figure_index);
    *figure_index += 1;

    let caption = inline_text(&image.alt);
    let figure_src = format!("apx-figure://{figure_id}");
    let figure_id_attr = figure_id.clone();
    let attr = json!([
        figure_id_attr,
        ["apx-figure"],
        [["data-figure-id", figure_id]]
    ]);

    json!({
        "t": "Para",
        "c": [{
            "t": "Image",
            "c": [
                attr,
                caption,
                [figure_src, ""]
            ]
        }]
    })
}

fn inline_text(text: &str) -> Vec<Value> {
    let mut inlines = Vec::new();
    for (idx, word) in text.split_whitespace().enumerate() {
        if idx > 0 {
            inlines.push(json!({"t": "Space"}));
        }
        inlines.push(json!({"t": "Str", "c": word}));
    }
    if inlines.is_empty() {
        inlines.push(json!({"t": "Str", "c": "Figure"}));
    }
    inlines
}

fn pandoc_cell(content: &Value) -> Value {
    // WHY: Pandoc Cell = [Attr, Alignment, RowSpan, ColSpan, [Block]]; `content`
    // is already the [Block] list, so embed it directly (no extra wrapping).
    json!([["", [], []], {"t": "AlignDefault"}, 1, 1, content])
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
    use poiesis_core::{Block, Document, Metadata, RichText};

    use super::*;

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
        let bytes = document_to_pandoc_json(&doc).expect("serialize AST");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("must be valid JSON");
        assert_eq!(v["pandoc-api-version"], json!([1, 23, 1, 1]));
        assert_eq!(v["blocks"][0]["t"], "Header");
        assert_eq!(v["blocks"][1]["t"], "Para");
    }

    #[test]
    fn title_in_meta() {
        let doc = minimal_doc();
        let bytes = document_to_pandoc_json(&doc).expect("serialize AST");
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

        let bytes = document_to_pandoc_json(&doc).expect("serialize AST");
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

        let bytes = document_to_pandoc_json(&doc).expect("serialize AST");
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

        let bytes = document_to_pandoc_json(&doc).expect("serialize AST");
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

        let bytes = document_to_pandoc_json(&doc).expect("serialize AST");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
        assert_eq!(v["blocks"][0]["t"], "RawBlock");
        assert_eq!(v["blocks"][0]["c"], json!(["latex", "\\alpha"]));
    }
}
