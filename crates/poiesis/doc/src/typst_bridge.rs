//! Convert a [`poiesis_core::Document`] to inline Typst markup.

use std::fmt::Write as _;

use poiesis_core::{Block, Document, RichText, Span};

/// Render a [`Document`] as inline Typst source.
pub(crate) fn doc_to_typst(doc: &Document) -> String {
    let mut out = String::new();

    // Preamble
    out.push_str("#set page(paper: \"a4\", margin: 2cm)\n");
    out.push_str("#set text(font: \"Liberation Sans\", size: 11pt)\n\n");

    // Title
    let _ = write!(
        out,
        "#align(center)[#text(18pt, weight: \"bold\")[{}]]\n\n",
        doc.metadata.title
    );

    // Author
    if let Some(author) = &doc.metadata.author {
        let _ = writeln!(out, "#align(center)[#emph[{author}]]");
        out.push_str("#v(8pt)\n\n");
    }

    // Content blocks
    for block in &doc.content {
        match block {
            Block::Heading { level, text } => {
                let hashes = "=".repeat(usize::from((*level).max(1)));
                let _ = write!(out, "{hashes} {}\n\n", render_rich(text));
            }
            Block::Paragraph(text) => {
                let _ = write!(out, "{}\n\n", render_rich(text));
            }
            Block::Note(note) => {
                let _ = writeln!(out, "*{}:* {}", note.kind.label(), render_rich(&note.body));
                out.push('\n');
            }
            Block::DisplayMath(expr) => {
                let _ = write!(out, "${expr}$\n\n");
            }
            Block::RawBlock { format, content } => {
                let _ = writeln!(out, "[raw:{format}] {content}");
                out.push('\n');
            }
            Block::PageBreak => {
                out.push_str("#pagebreak()\n\n");
            }
            Block::Table(t) => {
                let cols = t.headers.len();
                let _ = writeln!(out, "#table(columns: {cols},");
                for h in &t.headers {
                    let _ = write!(out, "  [*{h}*],");
                }
                out.push('\n');
                for row in &t.rows {
                    for cell in row {
                        let _ = write!(out, "  [{}],", render_rich(cell));
                    }
                    out.push('\n');
                }
                out.push_str(")\n\n");
            }
            Block::List { ordered, items } => {
                let macro_name = if *ordered { "enum" } else { "list" };
                let _ = writeln!(out, "#{macro_name}(");
                for item in items {
                    let _ = writeln!(out, "  [{}],", render_rich(&item.content));
                }
                out.push_str(")\n\n");
            }
            Block::Image(_) => {}
        }
    }

    out
}

fn render_rich(text: &RichText) -> String {
    text.spans.iter().map(render_span).collect()
}

fn render_span(span: &Span) -> String {
    match span {
        Span::Plain(s) => s.clone(),
        Span::Bold(s) => format!("*{s}*"),
        Span::Italic(s) => format!("_{s}_"),
        Span::Code(s) => format!("`{s}`"),
        Span::Cite(id) => id.clone(),
        Span::Link { text, url } => format!("#link(\"{url}\")[{text}]"),
    }
}
