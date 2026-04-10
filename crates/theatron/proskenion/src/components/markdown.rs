//! Markdown-to-RSX renderer using pulldown-cmark.

use dioxus::prelude::*;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use super::code_block::CodeBlock;
use super::table::MdTable;

/// Render markdown text as Dioxus RSX elements.
#[component]
pub(crate) fn Markdown(content: String) -> Element {
    let blocks = parse_to_blocks(&content);

    rsx! {
        div {
            class: "markdown-body",
            for (i , block) in blocks.into_iter().enumerate() {
                {render_block(block, i)}
            }
        }
    }
}

/// Intermediate block representation from pulldown-cmark events.
///
/// WHY: pulldown-cmark emits a flat event stream, not a tree. Converting to
/// blocks first gives us a clean structure for RSX generation. This is the
/// same two-pass approach the TUI uses (events → intermediate → output).
#[derive(Debug, Clone)]
pub(crate) enum MdBlock {
    Paragraph(Vec<MdInline>),
    Heading {
        level: u8,
        content: Vec<MdInline>,
    },
    CodeBlock {
        language: String,
        code: String,
    },
    BlockQuote(Vec<MdBlock>),
    UnorderedList(Vec<Vec<MdBlock>>),
    OrderedList {
        start: u64,
        items: Vec<Vec<MdBlock>>,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
        alignments: Vec<Alignment>,
    },
    HorizontalRule,
}

/// Inline content within a block.
#[derive(Debug, Clone)]
pub(crate) enum MdInline {
    Text(String),
    Bold(Vec<MdInline>),
    Italic(Vec<MdInline>),
    Strikethrough(Vec<MdInline>),
    Code(String),
    Link { text: Vec<MdInline>, url: String },
    Image { alt: String, url: String },
    SoftBreak,
    HardBreak,
}

/// Parse markdown text into a block tree.
pub(crate) fn parse_to_blocks(text: &str) -> Vec<MdBlock> {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(text, options);
    let events: Vec<Event<'_>> = parser.collect();
    let mut pos = 0;
    parse_blocks(&events, &mut pos, None)
}

fn parse_blocks<'a>(
    events: &[Event<'a>],
    pos: &mut usize,
    stop_at: Option<TagEnd>,
) -> Vec<MdBlock> {
    let mut blocks = Vec::new();

    while *pos < events.len() {
        if let Some(ref stop) = stop_at {
            if matches!(&events[*pos], Event::End(tag) if tag == stop) {
                *pos += 1;
                return blocks;
            }
        }

        match &events[*pos] {
            Event::Start(Tag::Paragraph) => {
                *pos += 1;
                let inlines = parse_inlines(events, pos, TagEnd::Paragraph);
                blocks.push(MdBlock::Paragraph(inlines));
            }
            Event::Start(Tag::Heading { level, .. }) => {
                let level_num = heading_level_num(*level);
                *pos += 1;
                let inlines = parse_inlines(events, pos, TagEnd::Heading(*level));
                blocks.push(MdBlock::Heading {
                    level: level_num,
                    content: inlines,
                });
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                *pos += 1;
                let mut code = String::new();
                while *pos < events.len() {
                    match &events[*pos] {
                        Event::Text(t) => {
                            code.push_str(t);
                            *pos += 1;
                        }
                        Event::End(TagEnd::CodeBlock) => {
                            *pos += 1;
                            break;
                        }
                        _ => {
                            *pos += 1;
                        }
                    }
                }
                // WHY: trailing newline from fence is cosmetic noise
                if code.ends_with('\n') {
                    code.pop();
                }
                blocks.push(MdBlock::CodeBlock { language, code });
            }
            Event::Start(Tag::BlockQuote(_)) => {
                *pos += 1;
                let inner = parse_blocks(events, pos, Some(TagEnd::BlockQuote(None)));
                blocks.push(MdBlock::BlockQuote(inner));
            }
            Event::Start(Tag::List(start_num)) => {
                let start = *start_num;
                *pos += 1;
                let mut items = Vec::new();
                while *pos < events.len() {
                    match &events[*pos] {
                        Event::Start(Tag::Item) => {
                            *pos += 1;
                            let item_blocks = parse_blocks(events, pos, Some(TagEnd::Item));
                            items.push(item_blocks);
                        }
                        Event::End(TagEnd::List(_)) => {
                            *pos += 1;
                            break;
                        }
                        _ => {
                            *pos += 1;
                        }
                    }
                }
                match start {
                    Some(n) => blocks.push(MdBlock::OrderedList { start: n, items }),
                    None => blocks.push(MdBlock::UnorderedList(items)),
                }
            }
            Event::Start(Tag::Table(alignments)) => {
                let alignments = alignments.clone();
                *pos += 1;
                let mut headers = Vec::new();
                let mut rows = Vec::new();
                let mut in_head = false;
                let mut current_row: Vec<String> = Vec::new();
                let mut current_cell = String::new();

                while *pos < events.len() {
                    match &events[*pos] {
                        Event::Start(Tag::TableHead) => {
                            in_head = true;
                            current_row.clear();
                            *pos += 1;
                        }
                        Event::End(TagEnd::TableHead) => {
                            headers = current_row.clone();
                            in_head = false;
                            *pos += 1;
                        }
                        Event::Start(Tag::TableRow) => {
                            current_row.clear();
                            *pos += 1;
                        }
                        Event::End(TagEnd::TableRow) => {
                            if !in_head {
                                rows.push(current_row.clone());
                            }
                            *pos += 1;
                        }
                        Event::Start(Tag::TableCell) => {
                            current_cell.clear();
                            *pos += 1;
                        }
                        Event::End(TagEnd::TableCell) => {
                            current_row.push(current_cell.trim().to_string());
                            *pos += 1;
                        }
                        Event::Text(t) => {
                            current_cell.push_str(t);
                            *pos += 1;
                        }
                        Event::Code(c) => {
                            current_cell.push('`');
                            current_cell.push_str(c);
                            current_cell.push('`');
                            *pos += 1;
                        }
                        Event::End(TagEnd::Table) => {
                            *pos += 1;
                            break;
                        }
                        _ => {
                            *pos += 1;
                        }
                    }
                }
                blocks.push(MdBlock::Table {
                    headers,
                    rows,
                    alignments,
                });
            }
            Event::Rule => {
                *pos += 1;
                blocks.push(MdBlock::HorizontalRule);
            }
            _ => {
                *pos += 1;
            }
        }
    }

    blocks
}

fn parse_inlines<'a>(events: &[Event<'a>], pos: &mut usize, stop_at: TagEnd) -> Vec<MdInline> {
    let mut inlines = Vec::new();

    while *pos < events.len() {
        if matches!(&events[*pos], Event::End(tag) if *tag == stop_at) {
            *pos += 1;
            return inlines;
        }

        match &events[*pos] {
            Event::Text(t) => {
                inlines.push(MdInline::Text(t.to_string()));
                *pos += 1;
            }
            Event::Code(c) => {
                inlines.push(MdInline::Code(c.to_string()));
                *pos += 1;
            }
            Event::SoftBreak => {
                inlines.push(MdInline::SoftBreak);
                *pos += 1;
            }
            Event::HardBreak => {
                inlines.push(MdInline::HardBreak);
                *pos += 1;
            }
            Event::Start(Tag::Strong) => {
                *pos += 1;
                let inner = parse_inlines(events, pos, TagEnd::Strong);
                inlines.push(MdInline::Bold(inner));
            }
            Event::Start(Tag::Emphasis) => {
                *pos += 1;
                let inner = parse_inlines(events, pos, TagEnd::Emphasis);
                inlines.push(MdInline::Italic(inner));
            }
            Event::Start(Tag::Strikethrough) => {
                *pos += 1;
                let inner = parse_inlines(events, pos, TagEnd::Strikethrough);
                inlines.push(MdInline::Strikethrough(inner));
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                let url = dest_url.to_string();
                *pos += 1;
                let inner = parse_inlines(events, pos, TagEnd::Link);
                inlines.push(MdInline::Link { text: inner, url });
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                let url = dest_url.to_string();
                *pos += 1;
                // Consume until end of image tag, collecting alt text.
                let mut alt = String::new();
                while *pos < events.len() {
                    match &events[*pos] {
                        Event::Text(t) => {
                            alt.push_str(t);
                            *pos += 1;
                        }
                        Event::End(TagEnd::Image) => {
                            *pos += 1;
                            break;
                        }
                        _ => {
                            *pos += 1;
                        }
                    }
                }
                inlines.push(MdInline::Image { alt, url });
            }
            _ => {
                *pos += 1;
            }
        }
    }

    inlines
}

fn heading_level_num(level: pulldown_cmark::HeadingLevel) -> u8 {
    match level {
        pulldown_cmark::HeadingLevel::H1 => 1,
        pulldown_cmark::HeadingLevel::H2 => 2,
        pulldown_cmark::HeadingLevel::H3 => 3,
        pulldown_cmark::HeadingLevel::H4 => 4,
        pulldown_cmark::HeadingLevel::H5 => 5,
        pulldown_cmark::HeadingLevel::H6 => 6,
    }
}

fn render_block(block: MdBlock, key: usize) -> Element {
    match block {
        MdBlock::Paragraph(inlines) => rsx! {
            p {
                key: "{key}",
                style: "
                    margin: var(--space-1) 0;
                    line-height: var(--leading-relaxed);
                    color: var(--text-primary);
                ",
                {render_inlines(inlines)}
            }
        },
        MdBlock::Heading { level, content } => {
            let (tag_style, font) = heading_style(level);
            rsx! {
                div {
                    key: "{key}",
                    role: "heading",
                    aria_level: "{level}",
                    style: "{tag_style} font-family: {font};",
                    {render_inlines(content)}
                }
            }
        }
        MdBlock::CodeBlock { language, code } => rsx! {
            CodeBlock {
                key: "{key}",
                code,
                language,
            }
        },
        MdBlock::BlockQuote(inner) => rsx! {
            blockquote {
                key: "{key}",
                style: "
                    border-left: 3px solid var(--accent-dim);
                    padding-left: var(--space-3);
                    margin: var(--space-2) 0;
                    color: var(--text-secondary);
                    font-style: italic;
                ",
                for (i , block) in inner.into_iter().enumerate() {
                    {render_block(block, i)}
                }
            }
        },
        MdBlock::UnorderedList(items) => rsx! {
            ul {
                key: "{key}",
                style: "
                    margin: var(--space-1) 0;
                    padding-left: var(--space-6);
                    color: var(--text-primary);
                ",
                for (i , item) in items.into_iter().enumerate() {
                    li {
                        key: "{i}",
                        style: "margin: var(--space-1) 0;",
                        for (j , block) in item.into_iter().enumerate() {
                            {render_block(block, j)}
                        }
                    }
                }
            }
        },
        MdBlock::OrderedList { start, items } => rsx! {
            ol {
                key: "{key}",
                start: "{start}",
                style: "
                    margin: var(--space-1) 0;
                    padding-left: var(--space-6);
                    color: var(--text-primary);
                ",
                for (i , item) in items.into_iter().enumerate() {
                    li {
                        key: "{i}",
                        style: "margin: var(--space-1) 0;",
                        for (j , block) in item.into_iter().enumerate() {
                            {render_block(block, j)}
                        }
                    }
                }
            }
        },
        MdBlock::Table {
            headers,
            rows,
            alignments,
        } => rsx! {
            MdTable {
                key: "{key}",
                headers,
                rows,
                alignments,
            }
        },
        MdBlock::HorizontalRule => rsx! {
            hr {
                key: "{key}",
                style: "
                    border: none;
                    border-top: 1px solid var(--border);
                    margin: var(--space-4) 0;
                ",
            }
        },
    }
}

fn render_inlines(inlines: Vec<MdInline>) -> Element {
    rsx! {
        for (i , inline) in inlines.into_iter().enumerate() {
            {render_inline(inline, i)}
        }
    }
}

fn render_inline(inline: MdInline, key: usize) -> Element {
    match inline {
        MdInline::Text(t) => rsx! {
            span { key: "{key}", "{t}" }
        },
        MdInline::Bold(inner) => rsx! {
            strong {
                key: "{key}",
                style: "font-weight: var(--weight-bold);",
                {render_inlines(inner)}
            }
        },
        MdInline::Italic(inner) => rsx! {
            em {
                key: "{key}",
                style: "font-style: italic;",
                {render_inlines(inner)}
            }
        },
        MdInline::Strikethrough(inner) => rsx! {
            s {
                key: "{key}",
                style: "text-decoration: line-through; color: var(--text-muted);",
                {render_inlines(inner)}
            }
        },
        MdInline::Code(code) => rsx! {
            code {
                key: "{key}",
                style: "
                    font-family: var(--font-mono);
                    background: var(--code-bg);
                    padding: 1px var(--space-1);
                    border-radius: var(--radius-sm);
                    font-size: 0.9em;
                    color: var(--code-fg);
                ",
                "{code}"
            }
        },
        MdInline::Link { text, url } => rsx! {
            a {
                key: "{key}",
                href: "#",
                onclick: {
                    let url = url.clone();
                    move |evt: dioxus::prelude::Event<dioxus::events::MouseData>| {
                        evt.prevent_default();
                        // WHY: open in external browser, not inside webview
                        let escaped = url.replace('\'', "\\'");
                        let js = format!("window.open('{escaped}', '_blank')");
                        document::eval(&js);
                    }
                },
                style: "
                    color: var(--accent);
                    text-decoration: underline;
                    text-decoration-color: var(--accent-dim);
                    cursor: pointer;
                ",
                {render_inlines(text)}
            }
        },
        MdInline::Image { alt, url } => rsx! {
            img {
                key: "{key}",
                src: "{url}",
                alt: "{alt}",
                style: "
                    max-width: 100%;
                    border-radius: var(--radius-lg);
                    margin: var(--space-2) 0;
                ",
            }
        },
        MdInline::SoftBreak => rsx! {
            span { key: "{key}", " " }
        },
        MdInline::HardBreak => rsx! {
            br { key: "{key}" }
        },
    }
}

fn heading_style(level: u8) -> (&'static str, &'static str) {
    let display_font = "var(--font-display)";
    let body_font = "var(--font-body)";
    match level {
        1 => (
            "font-size: var(--text-2xl); font-weight: var(--weight-bold); \
             margin: var(--space-4) 0 var(--space-2) 0; color: var(--text-primary); \
             line-height: var(--leading-tight);",
            display_font,
        ),
        2 => (
            "font-size: var(--text-xl); font-weight: var(--weight-semibold); \
             margin: var(--space-3) 0 var(--space-2) 0; color: var(--text-primary); \
             line-height: var(--leading-tight);",
            display_font,
        ),
        3 => (
            "font-size: var(--text-lg); font-weight: var(--weight-semibold); \
             margin: var(--space-3) 0 var(--space-1) 0; color: var(--text-primary); \
             line-height: var(--leading-tight);",
            display_font,
        ),
        4 => (
            "font-size: var(--text-md); font-weight: var(--weight-semibold); \
             margin: var(--space-2) 0 var(--space-1) 0; color: var(--text-primary);",
            body_font,
        ),
        5 => (
            "font-size: var(--text-base); font-weight: var(--weight-semibold); \
             margin: var(--space-2) 0 var(--space-1) 0; color: var(--text-secondary);",
            body_font,
        ),
        _ => (
            "font-size: var(--text-sm); font-weight: var(--weight-semibold); \
             margin: var(--space-1) 0; color: var(--text-secondary);",
            body_font,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_text() {
        let blocks = parse_to_blocks("");
        assert!(blocks.is_empty());
    }

    #[test]
    fn parse_paragraph() {
        let blocks = parse_to_blocks("Hello world");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MdBlock::Paragraph(_)));
    }

    #[test]
    fn parse_heading_levels() {
        let blocks = parse_to_blocks("# H1\n## H2\n### H3");
        assert_eq!(blocks.len(), 3);
        for (i, block) in blocks.iter().enumerate() {
            match block {
                MdBlock::Heading { level, .. } => {
                    assert_eq!(*level, (i + 1) as u8);
                }
                _ => panic!("expected heading at index {i}"),
            }
        }
    }

    #[test]
    fn parse_code_block() {
        let md = "```rust\nfn main() {}\n```";
        let blocks = parse_to_blocks(md);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::CodeBlock { language, code } => {
                assert_eq!(language, "rust");
                assert_eq!(code, "fn main() {}");
            }
            _ => panic!("expected code block"),
        }
    }

    #[test]
    fn parse_inline_formatting() {
        let blocks = parse_to_blocks("**bold** and *italic* and ~~strike~~ and `code`");
        assert_eq!(blocks.len(), 1);
        if let MdBlock::Paragraph(inlines) = &blocks[0] {
            let has_bold = inlines.iter().any(|i| matches!(i, MdInline::Bold(_)));
            let has_italic = inlines.iter().any(|i| matches!(i, MdInline::Italic(_)));
            let has_strike = inlines
                .iter()
                .any(|i| matches!(i, MdInline::Strikethrough(_)));
            let has_code = inlines.iter().any(|i| matches!(i, MdInline::Code(_)));
            assert!(has_bold, "expected bold");
            assert!(has_italic, "expected italic");
            assert!(has_strike, "expected strikethrough");
            assert!(has_code, "expected inline code");
        } else {
            panic!("expected paragraph");
        }
    }

    #[test]
    fn parse_unordered_list() {
        let md = "- item 1\n- item 2\n- item 3";
        let blocks = parse_to_blocks(md);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::UnorderedList(items) => assert_eq!(items.len(), 3),
            _ => panic!("expected unordered list"),
        }
    }

    #[test]
    fn parse_ordered_list() {
        let md = "1. first\n2. second";
        let blocks = parse_to_blocks(md);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::OrderedList { start, items } => {
                assert_eq!(*start, 1);
                assert_eq!(items.len(), 2);
            }
            _ => panic!("expected ordered list"),
        }
    }

    #[test]
    fn parse_blockquote() {
        let md = "> quoted text";
        let blocks = parse_to_blocks(md);
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], MdBlock::BlockQuote(_)));
    }

    #[test]
    fn parse_horizontal_rule() {
        let md = "above\n\n---\n\nbelow";
        let blocks = parse_to_blocks(md);
        let has_rule = blocks.iter().any(|b| matches!(b, MdBlock::HorizontalRule));
        assert!(has_rule, "expected horizontal rule");
    }

    #[test]
    fn parse_link() {
        let blocks = parse_to_blocks("[click](https://example.com)");
        assert_eq!(blocks.len(), 1);
        if let MdBlock::Paragraph(inlines) = &blocks[0] {
            let has_link = inlines.iter().any(|i| match i {
                MdInline::Link { url, .. } => url == "https://example.com",
                _ => false,
            });
            assert!(has_link, "expected link");
        } else {
            panic!("expected paragraph");
        }
    }

    #[test]
    fn parse_table() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |";
        let blocks = parse_to_blocks(md);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            MdBlock::Table { headers, rows, .. } => {
                assert_eq!(headers.len(), 2);
                assert_eq!(headers[0], "A");
                assert_eq!(headers[1], "B");
                assert_eq!(rows.len(), 2);
            }
            _ => panic!("expected table"),
        }
    }

    #[test]
    fn heading_style_display_font_h1_h3() {
        for level in 1..=3 {
            let (_, font) = heading_style(level);
            assert_eq!(font, "var(--font-display)");
        }
    }

    #[test]
    fn heading_style_body_font_h4_plus() {
        for level in 4..=6 {
            let (_, font) = heading_style(level);
            assert_eq!(font, "var(--font-body)");
        }
    }
}
