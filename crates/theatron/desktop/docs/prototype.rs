//! Markdown rendering prototype for Dioxus desktop.
//!
//! Validates the pulldown-cmark to `MdNode` to Dioxus RSX pipeline
//! and incremental rendering during streaming.
//!
//! Included as `#[cfg(test)] mod md_prototype` in `theatron-core/src/lib.rs`
//! so tests run with `cargo test -p theatron-core`.

use std::fmt::Write as _;

// ---------------------------------------------------------------------------
// MdNode: lightweight markdown AST
// ---------------------------------------------------------------------------

/// Lightweight markdown AST node produced by parsing pulldown-cmark events.
///
/// Design goals:
/// - Cheap to clone (children are small vecs, strings are owned)
/// - `PartialEq` for memoization (Dioxus skips re-render when props unchanged)
/// - No lifetime parameters — owned data for signal storage
#[derive(Debug, Clone, PartialEq)]
pub enum MdNode {
    // -- Block-level --
    Heading {
        level: u8,
        children: Vec<MdNode>,
    },
    Paragraph(Vec<MdNode>),
    CodeBlock {
        lang: Option<String>,
        code: String,
    },
    BlockQuote(Vec<MdNode>),
    List {
        ordered: bool,
        start: Option<u64>,
        items: Vec<Vec<MdNode>>,
    },
    Table {
        headers: Vec<Vec<MdNode>>,
        rows: Vec<Vec<Vec<MdNode>>>,
    },
    ThematicBreak,

    // -- Inline --
    Text(String),
    Code(String),
    Strong(Vec<MdNode>),
    Emphasis(Vec<MdNode>),
    Strikethrough(Vec<MdNode>),
    Link {
        url: String,
        children: Vec<MdNode>,
    },
    Image {
        url: String,
        alt: String,
    },
    SoftBreak,
    HardBreak,
}

// ---------------------------------------------------------------------------
// Parser: pulldown-cmark events → MdNode tree
// ---------------------------------------------------------------------------

use pulldown_cmark::{Event, Options, Parser, Tag};

/// Internal tag used during stack-based parsing.
#[derive(Debug)]
enum BuilderKind {
    Document,
    Heading(u8),
    Paragraph,
    BlockQuote,
    List { ordered: bool, start: Option<u64> },
    ListItem,
    Strong,
    Emphasis,
    Strikethrough,
    Link(String),
    Image { url: String, alt: String },
    Table,
    TableHead,
    TableRow,
    TableCell,
    CodeBlock { lang: Option<String> },
}

/// Parse markdown text into a list of top-level `MdNode` blocks.
pub fn parse_markdown(text: &str) -> Vec<MdNode> {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(text, options);

    let mut stack: Vec<(BuilderKind, Vec<MdNode>)> = vec![(BuilderKind::Document, Vec::new())];

    for event in parser {
        match event {
            Event::Start(tag) => {
                let kind = tag_to_builder_kind(&tag);
                stack.push((kind, Vec::new()));
            }

            Event::End(_) => {
                if stack.len() <= 1 {
                    continue;
                }
                let Some((kind, children)) = stack.pop() else {
                    continue;
                };
                if let Some(node) = build_node(kind, children)
                    && let Some(last) = stack.last_mut()
                {
                    last.1.push(node);
                }
            }

            Event::Text(t) => {
                if let Some(last) = stack.last_mut() {
                    match &mut last.0 {
                        BuilderKind::Image { alt, .. } => {
                            alt.push_str(&t);
                        }
                        _ => {
                            last.1.push(MdNode::Text(t.to_string()));
                        }
                    }
                }
            }

            Event::Code(c) => {
                if let Some(last) = stack.last_mut() {
                    last.1.push(MdNode::Code(c.to_string()));
                }
            }

            Event::SoftBreak => {
                if let Some(last) = stack.last_mut() {
                    last.1.push(MdNode::SoftBreak);
                }
            }

            Event::HardBreak => {
                if let Some(last) = stack.last_mut() {
                    last.1.push(MdNode::HardBreak);
                }
            }

            Event::Rule => {
                if let Some(last) = stack.last_mut() {
                    last.1.push(MdNode::ThematicBreak);
                }
            }

            _ => {}
        }
    }

    stack
        .pop()
        .map(|(_, children)| children)
        .unwrap_or_default()
}

fn tag_to_builder_kind(tag: &Tag<'_>) -> BuilderKind {
    match tag {
        Tag::Heading { level, .. } => BuilderKind::Heading(*level as u8),
        Tag::BlockQuote(_) => BuilderKind::BlockQuote,
        Tag::List(first) => BuilderKind::List {
            ordered: first.is_some(),
            start: *first,
        },
        Tag::Item => BuilderKind::ListItem,
        Tag::Strong => BuilderKind::Strong,
        Tag::Emphasis => BuilderKind::Emphasis,
        Tag::Strikethrough => BuilderKind::Strikethrough,
        Tag::Link { dest_url, .. } => BuilderKind::Link(dest_url.to_string()),
        Tag::Image { dest_url, .. } => BuilderKind::Image {
            url: dest_url.to_string(),
            alt: String::new(),
        },
        Tag::Table(_) => BuilderKind::Table,
        Tag::TableHead => BuilderKind::TableHead,
        Tag::TableRow => BuilderKind::TableRow,
        Tag::TableCell => BuilderKind::TableCell,
        Tag::CodeBlock(kind) => {
            let lang = match kind {
                pulldown_cmark::CodeBlockKind::Fenced(l) => {
                    let s = l.to_string();
                    if s.is_empty() { None } else { Some(s) }
                }
                pulldown_cmark::CodeBlockKind::Indented => None,
            };
            BuilderKind::CodeBlock { lang }
        }
        _ => BuilderKind::Paragraph,
    }
}

/// Convert a builder frame into an `MdNode`.
fn build_node(kind: BuilderKind, children: Vec<MdNode>) -> Option<MdNode> {
    match kind {
        BuilderKind::Document => None,
        BuilderKind::Heading(level) => Some(MdNode::Heading { level, children }),
        BuilderKind::BlockQuote => Some(MdNode::BlockQuote(children)),
        BuilderKind::Strong => Some(MdNode::Strong(children)),
        BuilderKind::Emphasis => Some(MdNode::Emphasis(children)),
        BuilderKind::Strikethrough => Some(MdNode::Strikethrough(children)),
        BuilderKind::Link(url) => Some(MdNode::Link { url, children }),
        BuilderKind::Image { url, alt } => Some(MdNode::Image { url, alt }),
        BuilderKind::Paragraph
        | BuilderKind::ListItem
        | BuilderKind::TableCell
        | BuilderKind::TableHead
        | BuilderKind::TableRow => Some(MdNode::Paragraph(children)),
        BuilderKind::List { ordered, start } => {
            let items: Vec<Vec<MdNode>> = children
                .into_iter()
                .map(|item| match item {
                    MdNode::Paragraph(c) => c,
                    other => vec![other],
                })
                .collect();
            Some(MdNode::List {
                ordered,
                start,
                items,
            })
        }
        BuilderKind::CodeBlock { lang } => {
            let code = children
                .into_iter()
                .map(|n| match n {
                    MdNode::Text(t) => t,
                    _ => String::new(),
                })
                .collect::<String>();
            Some(MdNode::CodeBlock { lang, code })
        }
        BuilderKind::Table => {
            let mut headers = Vec::new();
            let mut rows = Vec::new();
            for (i, row_node) in children.into_iter().enumerate() {
                let cells: Vec<Vec<MdNode>> = match row_node {
                    MdNode::Paragraph(cell_nodes) => cell_nodes
                        .into_iter()
                        .map(|cell| match cell {
                            MdNode::Paragraph(c) => c,
                            other => vec![other],
                        })
                        .collect(),
                    other => vec![vec![other]],
                };
                if i == 0 {
                    headers = cells;
                } else {
                    rows.push(cells);
                }
            }
            Some(MdNode::Table { headers, rows })
        }
    }
}

// ---------------------------------------------------------------------------
// IncrementalMarkdown: streaming-friendly incremental parser
// ---------------------------------------------------------------------------

/// Incremental markdown parser for streaming LLM output.
///
/// Maintains a boundary between "frozen" blocks (complete, memoized)
/// and an "active tail" (in-progress, re-parsed on each delta).
///
/// This ensures `O(tail_size)` work per delta instead of `O(total_size)`.
pub struct IncrementalMarkdown {
    full_text: String,
    frozen_boundary: usize,
    frozen_nodes: Vec<MdNode>,
    active_tail_nodes: Vec<MdNode>,
}

impl IncrementalMarkdown {
    pub fn new() -> Self {
        Self {
            full_text: String::new(),
            frozen_boundary: 0,
            frozen_nodes: Vec::new(),
            active_tail_nodes: Vec::new(),
        }
    }

    /// Append a streaming delta and incrementally update the parse.
    pub fn push_delta(&mut self, delta: &str) {
        self.full_text.push_str(delta);
        self.try_freeze_blocks();
        self.reparse_tail();
    }

    fn try_freeze_blocks(&mut self) {
        let unfrozen = &self.full_text[self.frozen_boundary..];

        if let Some(pos) = rfind_block_boundary(unfrozen) {
            let abs_boundary = self.frozen_boundary + pos;
            let new_frozen_text = &self.full_text[self.frozen_boundary..abs_boundary];

            if !new_frozen_text.trim().is_empty() {
                let new_nodes = parse_markdown(new_frozen_text);
                self.frozen_nodes.extend(new_nodes);
            }

            self.frozen_boundary = abs_boundary;
        }
    }

    fn reparse_tail(&mut self) {
        let tail = &self.full_text[self.frozen_boundary..];
        if tail.trim().is_empty() {
            self.active_tail_nodes.clear();
        } else {
            self.active_tail_nodes = parse_markdown(tail);
        }
    }

    /// Iterator over all nodes (frozen + active tail).
    pub fn nodes(&self) -> impl Iterator<Item = &MdNode> {
        self.frozen_nodes
            .iter()
            .chain(self.active_tail_nodes.iter())
    }

    /// Number of frozen (stable) nodes.
    pub fn frozen_count(&self) -> usize {
        self.frozen_nodes.len()
    }

    /// Number of active tail (re-parsed) nodes.
    pub fn tail_count(&self) -> usize {
        self.active_tail_nodes.len()
    }

    /// Full accumulated text.
    pub fn text(&self) -> &str {
        &self.full_text
    }

    /// Freeze everything and return the complete node list.
    pub fn finalize(&mut self) -> Vec<MdNode> {
        if self.frozen_boundary < self.full_text.len() {
            let remaining = &self.full_text[self.frozen_boundary..];
            if !remaining.trim().is_empty() {
                let nodes = parse_markdown(remaining);
                self.frozen_nodes.extend(nodes);
            }
            self.frozen_boundary = self.full_text.len();
            self.active_tail_nodes.clear();
        }
        self.frozen_nodes.clone()
    }
}

/// Find the byte offset past the last block boundary (`\n\n`) in text,
/// excluding trailing boundaries.
fn rfind_block_boundary(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.len() < 4 {
        return None;
    }

    // Walk backwards, skip the last 2 bytes to avoid freezing a trailing \n\n
    let mut i = bytes.len().saturating_sub(3);
    while i > 0 {
        if bytes[i] == b'\n' && bytes[i + 1] == b'\n' {
            return Some(i + 2);
        }
        i -= 1;
    }
    None
}

// ---------------------------------------------------------------------------
// HTML rendering (validates MdNode tree completeness)
// ---------------------------------------------------------------------------

/// Render an `MdNode` tree to an HTML string.
///
/// In production this would be Dioxus RSX components. The HTML renderer
/// validates the tree contains sufficient information for full rendering.
pub fn render_to_html(nodes: &[MdNode]) -> String {
    let mut out = String::new();
    for node in nodes {
        render_node_html(node, &mut out);
    }
    out
}

#[expect(clippy::too_many_lines, reason = "HTML dispatch is inherently branchy")]
fn render_node_html(node: &MdNode, out: &mut String) {
    match node {
        MdNode::Heading { level, children } => {
            let lvl = (*level).min(6);
            let _ = write!(out, "<h{lvl} class=\"md-h{level}\">");
            render_children(children, out);
            let _ = write!(out, "</h{lvl}>");
        }
        MdNode::Paragraph(children) => {
            out.push_str("<p>");
            render_children(children, out);
            out.push_str("</p>");
        }
        MdNode::CodeBlock { lang, code } => {
            let lang_attr = lang
                .as_deref()
                .map(|l| format!(" class=\"language-{l}\""))
                .unwrap_or_default();
            let _ = write!(out, "<pre><code{lang_attr}>");
            out.push_str(&html_escape(code));
            out.push_str("</code></pre>");
        }
        MdNode::BlockQuote(children) => {
            out.push_str("<blockquote class=\"md-blockquote\">");
            render_children(children, out);
            out.push_str("</blockquote>");
        }
        MdNode::List {
            ordered,
            start,
            items,
        } => {
            if *ordered {
                let start_attr = start.map(|s| format!(" start=\"{s}\"")).unwrap_or_default();
                let _ = write!(out, "<ol{start_attr}>");
            } else {
                out.push_str("<ul>");
            }
            for item in items {
                out.push_str("<li>");
                render_children(item, out);
                out.push_str("</li>");
            }
            out.push_str(if *ordered { "</ol>" } else { "</ul>" });
        }
        MdNode::Table { headers, rows } => {
            out.push_str("<table class=\"md-table\"><thead><tr>");
            for header in headers {
                out.push_str("<th>");
                render_children(header, out);
                out.push_str("</th>");
            }
            out.push_str("</tr></thead><tbody>");
            for row in rows {
                out.push_str("<tr>");
                for cell in row {
                    out.push_str("<td>");
                    render_children(cell, out);
                    out.push_str("</td>");
                }
                out.push_str("</tr>");
            }
            out.push_str("</tbody></table>");
        }
        MdNode::ThematicBreak => out.push_str("<hr />"),
        MdNode::Text(t) => out.push_str(&html_escape(t)),
        MdNode::Code(c) => {
            out.push_str("<code class=\"md-inline-code\">");
            out.push_str(&html_escape(c));
            out.push_str("</code>");
        }
        MdNode::Strong(children) => {
            out.push_str("<strong>");
            render_children(children, out);
            out.push_str("</strong>");
        }
        MdNode::Emphasis(children) => {
            out.push_str("<em>");
            render_children(children, out);
            out.push_str("</em>");
        }
        MdNode::Strikethrough(children) => {
            out.push_str("<del>");
            render_children(children, out);
            out.push_str("</del>");
        }
        MdNode::Link { url, children } => {
            let _ = write!(out, "<a href=\"{}\">", html_escape(url));
            render_children(children, out);
            out.push_str("</a>");
        }
        MdNode::Image { url, alt } => {
            let _ = write!(
                out,
                "<img src=\"{}\" alt=\"{}\" />",
                html_escape(url),
                html_escape(alt)
            );
        }
        MdNode::SoftBreak => out.push(' '),
        MdNode::HardBreak => out.push_str("<br />"),
    }
}

fn render_children(children: &[MdNode], out: &mut String) {
    for child in children {
        render_node_html(child, out);
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test assertions may use unwrap for clarity"
)]
mod tests {
    use super::*;

    #[test]
    fn parses_heading() {
        let nodes = parse_markdown("# Hello World");
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MdNode::Heading { level: 1, children } => {
                assert_eq!(children.len(), 1);
                assert_eq!(children[0], MdNode::Text("Hello World".to_string()));
            }
            other => panic!("expected Heading, got {other:?}"),
        }
    }

    #[test]
    fn parses_multiple_heading_levels() {
        let nodes = parse_markdown("## Level 2\n\n### Level 3");
        assert_eq!(nodes.len(), 2);
        assert!(matches!(&nodes[0], MdNode::Heading { level: 2, .. }));
        assert!(matches!(&nodes[1], MdNode::Heading { level: 3, .. }));
    }

    #[test]
    fn parses_paragraph_with_inline_formatting() {
        let nodes = parse_markdown("Hello **bold** and *italic* text");
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MdNode::Paragraph(children) => {
                assert!(children.iter().any(|n| matches!(n, MdNode::Strong(_))));
                assert!(children.iter().any(|n| matches!(n, MdNode::Emphasis(_))));
            }
            other => panic!("expected Paragraph, got {other:?}"),
        }
    }

    #[test]
    fn parses_code_block_with_lang() {
        let md = "```rust\nfn main() {\n    println!(\"hello\");\n}\n```";
        let nodes = parse_markdown(md);
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MdNode::CodeBlock { lang, code } => {
                assert_eq!(lang.as_deref(), Some("rust"));
                assert!(code.contains("fn main()"));
            }
            other => panic!("expected CodeBlock, got {other:?}"),
        }
    }

    #[test]
    fn parses_code_block_no_lang() {
        let md = "```\nplain code\n```";
        let nodes = parse_markdown(md);
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MdNode::CodeBlock { lang, code } => {
                assert!(lang.is_none());
                assert!(code.contains("plain code"));
            }
            other => panic!("expected CodeBlock, got {other:?}"),
        }
    }

    #[test]
    fn parses_inline_code() {
        let nodes = parse_markdown("Use `cargo build` to compile");
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MdNode::Paragraph(children) => {
                assert!(
                    children
                        .iter()
                        .any(|n| matches!(n, MdNode::Code(c) if c == "cargo build"))
                );
            }
            other => panic!("expected Paragraph, got {other:?}"),
        }
    }

    #[test]
    fn parses_unordered_list() {
        let md = "- item one\n- item two\n- item three";
        let nodes = parse_markdown(md);
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MdNode::List {
                ordered,
                start,
                items,
            } => {
                assert!(!ordered);
                assert!(start.is_none());
                assert_eq!(items.len(), 3);
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn parses_ordered_list() {
        let md = "1. first\n2. second\n3. third";
        let nodes = parse_markdown(md);
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MdNode::List {
                ordered,
                start,
                items,
            } => {
                assert!(ordered);
                assert_eq!(*start, Some(1));
                assert_eq!(items.len(), 3);
            }
            other => panic!("expected List, got {other:?}"),
        }
    }

    #[test]
    fn parses_blockquote() {
        let nodes = parse_markdown("> quoted text");
        assert_eq!(nodes.len(), 1);
        assert!(matches!(&nodes[0], MdNode::BlockQuote(_)));
    }

    #[test]
    fn parses_link() {
        let nodes = parse_markdown("[click here](https://example.com)");
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MdNode::Paragraph(children) => {
                assert!(children.iter().any(|n| matches!(
                    n,
                    MdNode::Link { url, .. } if url == "https://example.com"
                )));
            }
            other => panic!("expected Paragraph with Link, got {other:?}"),
        }
    }

    #[test]
    fn parses_image() {
        let nodes = parse_markdown("![alt text](image.png)");
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MdNode::Paragraph(children) => {
                assert!(children.iter().any(|n| matches!(
                    n,
                    MdNode::Image { url, alt }
                    if url == "image.png" && alt == "alt text"
                )));
            }
            other => panic!("expected Paragraph with Image, got {other:?}"),
        }
    }

    #[test]
    fn parses_strikethrough() {
        let nodes = parse_markdown("~~deleted~~");
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MdNode::Paragraph(children) => {
                assert!(
                    children
                        .iter()
                        .any(|n| matches!(n, MdNode::Strikethrough(_)))
                );
            }
            other => panic!("expected Paragraph with Strikethrough, got {other:?}"),
        }
    }

    #[test]
    fn parses_thematic_break() {
        let nodes = parse_markdown("above\n\n---\n\nbelow");
        assert!(nodes.iter().any(|n| matches!(n, MdNode::ThematicBreak)));
    }

    #[test]
    fn parses_table() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |";
        let nodes = parse_markdown(md);
        assert_eq!(nodes.len(), 1);
        match &nodes[0] {
            MdNode::Table { headers, rows } => {
                assert_eq!(headers.len(), 2);
                assert_eq!(rows.len(), 2);
            }
            other => panic!("expected Table, got {other:?}"),
        }
    }

    #[test]
    fn renders_heading_html() {
        let nodes = parse_markdown("# Hello");
        let html = render_to_html(&nodes);
        assert!(html.contains("<h1"));
        assert!(html.contains("Hello"));
        assert!(html.contains("</h1>"));
    }

    #[test]
    fn renders_code_block_html() {
        let md = "```python\nprint('hi')\n```";
        let nodes = parse_markdown(md);
        let html = render_to_html(&nodes);
        assert!(html.contains("<pre><code class=\"language-python\">"));
    }

    #[test]
    fn renders_table_html() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let nodes = parse_markdown(md);
        let html = render_to_html(&nodes);
        assert!(html.contains("<table"));
        assert!(html.contains("<thead>"));
        assert!(html.contains("<th>"));
        assert!(html.contains("<td>"));
    }

    #[test]
    fn html_escapes_text() {
        let nodes = parse_markdown("Use `<div>` in HTML & more");
        let html = render_to_html(&nodes);
        assert!(html.contains("&lt;div&gt;"));
        assert!(html.contains("&amp;"));
    }

    #[test]
    fn incremental_basic_streaming() {
        let mut inc = IncrementalMarkdown::new();
        inc.push_delta("Hello world. This is the first paragraph.");
        assert_eq!(inc.frozen_count(), 0, "no frozen blocks yet");
        assert!(inc.tail_count() > 0, "active tail should have content");
        inc.push_delta("\n\nSecond paragraph begins");
        assert!(
            inc.frozen_count() > 0,
            "first paragraph should be frozen after \\n\\n"
        );
    }

    #[test]
    fn incremental_preserves_content() {
        let mut inc = IncrementalMarkdown::new();
        let full_text = "# Heading\n\nFirst paragraph.\n\nSecond paragraph.\n\n```rust\nlet x = 1;\n```\n\nFinal.";
        for chunk in full_text.as_bytes().chunks(10) {
            let s = std::str::from_utf8(chunk).unwrap();
            inc.push_delta(s);
        }
        let incremental_nodes = inc.finalize();
        let direct_nodes = parse_markdown(full_text);
        assert_eq!(
            incremental_nodes.len(),
            direct_nodes.len(),
            "incremental and direct parse should produce same node count"
        );
    }

    #[test]
    fn incremental_code_block_stays_in_tail() {
        let mut inc = IncrementalMarkdown::new();
        inc.push_delta("Some text.\n\n```rust\nfn main() {");
        let frozen_before = inc.frozen_count();
        inc.push_delta("\n    println!(\"hello\");");
        assert_eq!(
            inc.frozen_count(),
            frozen_before,
            "incomplete code block should not freeze more blocks"
        );
        inc.push_delta("\n}\n```\n\nDone.");
        assert!(
            inc.frozen_count() > frozen_before,
            "code block should be frozen after closing"
        );
    }

    #[test]
    fn incremental_empty_deltas_are_safe() {
        let mut inc = IncrementalMarkdown::new();
        inc.push_delta("");
        inc.push_delta("");
        inc.push_delta("hello");
        inc.push_delta("");
        assert_eq!(inc.text(), "hello");
    }

    #[test]
    fn incremental_finalize_collects_all() {
        let mut inc = IncrementalMarkdown::new();
        inc.push_delta("# Title\n\nParagraph one.\n\nParagraph two.");
        let nodes = inc.finalize();
        assert!(
            nodes.len() >= 3,
            "expected heading + 2 paragraphs, got {nodes:?}"
        );
        assert_eq!(inc.tail_count(), 0, "tail should be empty after finalize");
    }

    #[test]
    fn parses_real_llm_output() {
        let llm_output = "I'll help you implement this feature. Let me analyze the codebase first.\n\n## Analysis\n\nThe current implementation has several key components:\n\n1. **Parser** \u{2014} handles markdown tokenization\n2. **Renderer** \u{2014} converts tokens to visual output\n3. **Theme system** \u{2014} manages colors and styles\n\nHere's the proposed change:\n\n```rust\npub struct MarkdownRenderer {\n    parser: Parser,\n    theme: Theme,\n    highlighter: Highlighter,\n}\n\nimpl MarkdownRenderer {\n    pub fn render(&self, text: &str) -> Vec<Element> {\n        let nodes = self.parser.parse(text);\n        nodes.iter().map(|n| self.render_node(n)).collect()\n    }\n}\n```\n\n### Key considerations\n\n| Feature | Status | Priority |\n|---------|--------|----------|\n| Code highlighting | Done | High |\n| Table rendering | In progress | Medium |\n| Image support | Planned | Low |\n\n> **Note**: The rendering pipeline should be incremental to support\n> streaming output without visible flickering.\n\nFor more details, see the [architecture doc](docs/ARCHITECTURE.md).\n\n---\n\nThat covers the main approach. Let me know if you'd like me to proceed with the implementation.";

        let nodes = parse_markdown(llm_output);

        assert!(
            nodes.len() >= 5,
            "expected multiple blocks, got {}",
            nodes.len()
        );
        assert!(nodes.iter().any(|n| matches!(n, MdNode::Heading { .. })));
        assert!(nodes.iter().any(|n| matches!(n, MdNode::CodeBlock { .. })));
        assert!(nodes.iter().any(|n| matches!(n, MdNode::List { .. })));
        assert!(nodes.iter().any(|n| matches!(n, MdNode::Table { .. })));
        assert!(nodes.iter().any(|n| matches!(n, MdNode::BlockQuote { .. })));

        let html = render_to_html(&nodes);
        assert!(!html.is_empty());
        assert!(html.contains("<h2"));
        assert!(html.contains("<table"));
        assert!(html.contains("<code"));
        assert!(html.contains("<blockquote"));
    }

    #[test]
    fn incremental_with_real_llm_streaming() {
        let full = "## Analysis\n\nThe **parser** handles `markdown` tokenization.\n\n```rust\nlet x = 42;\n```\n\nDone.";
        let mut inc = IncrementalMarkdown::new();

        let chunks = [
            "## Ana",
            "lysis\n\n",
            "The **par",
            "ser** handles `mark",
            "down` tokenization",
            ".\n\n```rust\nlet",
            " x = 42;\n```\n\n",
            "Done.",
        ];

        for chunk in &chunks {
            inc.push_delta(chunk);
            let count: usize = inc.nodes().count();
            assert!(count > 0 || inc.text().trim().is_empty());
        }

        let final_nodes = inc.finalize();
        let direct_nodes = parse_markdown(full);
        assert_eq!(
            final_nodes.len(),
            direct_nodes.len(),
            "streaming and direct parse should converge"
        );
    }
}
