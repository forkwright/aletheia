use poiesis_core::{Block, Document, RichText, Span};

use super::PandocError;

#[derive(Debug, Clone, Copy)]
enum TypstEscapeContext {
    Markup,
    StringLiteral,
}

fn typst_escape(text: &str, context: TypstEscapeContext) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '#' | '*' | '_' | '[' | ']' | '$' | '@' | '<' | '>' | '='
                if matches!(context, TypstEscapeContext::Markup) =>
            {
                escaped.push('\\');
                escaped.push(ch);
            }
            '#' | '*' | '_' | '[' | ']' | '$' | '@' | '<' | '>' | '=' => {
                if let Some(escape) = typst_char_escape(ch) {
                    escaped.push_str(escape);
                } else {
                    escaped.push(ch);
                }
            }
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn typst_char_escape(ch: char) -> Option<&'static str> {
    match ch {
        '#' => Some("\\u{23}"),
        '*' => Some("\\u{2a}"),
        '_' => Some("\\u{5f}"),
        '[' => Some("\\u{5b}"),
        ']' => Some("\\u{5d}"),
        '$' => Some("\\u{24}"),
        '@' => Some("\\u{40}"),
        '<' => Some("\\u{3c}"),
        '>' => Some("\\u{3e}"),
        '=' => Some("\\u{3d}"),
        _ => None,
    }
}

fn render_typst_rich_text(rt: &RichText) -> String {
    rt.spans
        .iter()
        .map(|s| match s {
            Span::Plain(t) => typst_escape(t, TypstEscapeContext::Markup),
            Span::Bold(t) => format!("*{}*", typst_escape(t, TypstEscapeContext::Markup)),
            Span::Italic(t) => format!("_{}_", typst_escape(t, TypstEscapeContext::Markup)),
            Span::Code(t) => format!(
                "#raw(\"{}\")",
                typst_escape(t, TypstEscapeContext::StringLiteral)
            ),
            Span::Cite(id) => typst_escape(id, TypstEscapeContext::Markup),
            Span::Link { text, url } => format!(
                "#link(\"{}\")[{}]",
                typst_escape(url, TypstEscapeContext::StringLiteral),
                typst_escape(text, TypstEscapeContext::Markup)
            ),
        })
        .collect()
}

/// Convert the document model to Typst markup for the in-process PDF fast-lane.
pub(super) fn typst_source_from_doc(doc: &Document) -> String {
    let mut src = String::new();
    src.push_str("#set page(paper: \"a4\", margin: 2cm)\n");
    src.push_str("#set text(font: \"Liberation Sans\", size: 11pt)\n\n");
    src.push_str("#align(center)[#text(18pt, weight: \"bold\")[");
    src.push_str(&typst_escape(
        &doc.metadata.title,
        TypstEscapeContext::Markup,
    ));
    src.push_str("]]\n\n");
    if let Some(author) = &doc.metadata.author {
        src.push_str("#align(center)[#emph[");
        src.push_str(&typst_escape(author, TypstEscapeContext::Markup));
        src.push_str("]]\n#v(8pt)\n\n");
    }

    for block in &doc.content {
        match block {
            Block::Heading { level, text } => {
                let eq = "=".repeat(usize::from((*level).max(1)));
                src.push_str(&eq);
                src.push(' ');
                src.push_str(&render_typst_rich_text(text));
                src.push_str("\n\n");
            }
            Block::Paragraph(text) => {
                src.push_str(&render_typst_rich_text(text));
                src.push_str("\n\n");
            }
            Block::Note(note) => {
                src.push('*');
                src.push_str(note.kind.label());
                src.push_str(":* ");
                src.push_str(&render_typst_rich_text(&note.body));
                src.push_str("\n\n");
            }
            Block::DisplayMath(expr) => {
                src.push('$');
                src.push_str(&typst_escape(expr, TypstEscapeContext::Markup));
                src.push_str("$\n\n");
            }
            Block::RawBlock { format, content } => {
                src.push_str("[raw:");
                src.push_str(&typst_escape(format, TypstEscapeContext::Markup));
                src.push_str("] ");
                src.push_str(&typst_escape(content, TypstEscapeContext::Markup));
                src.push_str("\n\n");
            }
            Block::PageBreak => src.push_str("#pagebreak()\n\n"),
            Block::Table(t) => {
                let n = t.headers.len().max(1);
                src.push_str("#table(columns: ");
                src.push_str(&n.to_string());
                src.push_str(",\n");
                for h in &t.headers {
                    src.push_str("  [*");
                    src.push_str(&typst_escape(h, TypstEscapeContext::Markup));
                    src.push_str("*],\n");
                }
                for row in &t.rows {
                    for cell in row {
                        src.push_str("  [");
                        src.push_str(&render_typst_rich_text(cell));
                        src.push_str("],\n");
                    }
                }
                src.push_str(")\n\n");
            }
            Block::List { ordered, items } => {
                let cmd = if *ordered { "enum" } else { "list" };
                src.push('#');
                src.push_str(cmd);
                src.push_str("(\n");
                for item in items {
                    src.push_str("  [");
                    src.push_str(&render_typst_rich_text(&item.content));
                    src.push_str("],\n");
                }
                src.push_str(")\n\n");
            }
            Block::Image(image) => {
                // WARNING: the in-process Typst fast-lane cannot embed image bytes
                // (TypstWorld wires only data.json); emit alt text as a visible
                // placeholder rather than silently dropping the block.
                let alt = if image.alt.trim().is_empty() {
                    "untitled figure"
                } else {
                    image.alt.trim()
                };
                let alt_escaped = typst_escape(alt, TypstEscapeContext::Markup);
                src.push_str("#figure(rect(width: 100%, stroke: 0.5pt)[#emph[Figure: ");
                src.push_str(&alt_escaped);
                src.push_str("]])\n\n");
            }
        }
    }

    src
}

/// Render the document through `poiesis-typst`.
pub(super) fn render_doc_via_typst(doc: &Document) -> Result<Vec<u8>, PandocError> {
    let src = typst_source_from_doc(doc);
    poiesis_typst::render_typst(&src, &serde_json::json!({})).map_err(|e| {
        PandocError::WriterFailed {
            fmt: "pdf".to_owned(),
            stderr: e.to_string(),
        }
    })
}
