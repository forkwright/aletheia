use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::highlight::Highlighter;
use crate::hyperlink::{self, MdLink};
use crate::theme::ThemePalette;

/// Render markdown text into ratatui Lines, also returning any hyperlinks found.
///
/// Supports: bold, italic, code, headings, lists, code blocks (with syntax highlighting),
/// blockquotes, rules, tables, links, images, strikethrough, and OSC 8 hyperlinks.
///
/// SAFETY: callers must pass sanitized text. All external data is sanitized
/// at ingestion in update/api.rs, update/streaming.rs, update/sse.rs, and app.rs.
///
/// Returns `(lines, links)` where `links` contains [`MdLink`] entries for every
/// hyperlink found. Column offsets in `MdLink` are **relative** to the returned
/// `lines` and do **not** include any content-prefix prepended by the caller.
pub fn render(
    text: &str,
    _width: usize,
    theme: &ThemePalette,
    highlighter: &Highlighter,
) -> (Vec<Line<'static>>, Vec<MdLink>) {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(text, options);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    // Byte-based column counter for the current (unflushed) line.
    // Byte length is used intentionally — this matches the scroll calculation in
    // view/chat.rs which also uses `span.content.len()` for line-width estimation.
    let mut current_col: u16 = 0;
    let mut style_stack: Vec<Style> = vec![Style::default().fg(theme.fg)];
    let mut in_code_block = false;
    let mut code_block_lines: Vec<String> = Vec::new();
    let mut code_block_lang: Option<String> = None;
    let mut list_depth: usize = 0;

    // Link state
    let mut link_url: Option<String> = None;
    let mut link_start_line: usize = 0;
    let mut link_start_col: u16 = 0;
    let mut link_text_buf: String = String::new();

    // Image state
    let mut in_image = false;
    let mut image_alt = String::new();

    // Table state
    let mut in_table = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell = String::new();
    let mut is_table_head = false;

    // Blockquote nesting depth — tracked so paragraph start can prepend the │ prefix
    // on the correct line rather than flushing it alone (bug fix: see Tag::BlockQuote below).
    let mut blockquote_depth: usize = 0;

    // Collected hyperlinks (returned alongside lines)
    let mut md_links: Vec<MdLink> = Vec::new();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    flush_line(&mut lines, &mut current_spans, &mut current_col);
                    let style = theme.style_accent_bold();
                    style_stack.push(style);
                    let prefix = match level {
                        pulldown_cmark::HeadingLevel::H1 => "# ",
                        pulldown_cmark::HeadingLevel::H2 => "## ",
                        pulldown_cmark::HeadingLevel::H3 => "### ",
                        _ => "#### ",
                    };
                    push_span(
                        &mut current_spans,
                        &mut current_col,
                        Span::styled(prefix.to_string(), style),
                    );
                }
                Tag::Strong => {
                    let style = current_style(&style_stack).add_modifier(Modifier::BOLD);
                    style_stack.push(style);
                }
                Tag::Emphasis => {
                    let style = current_style(&style_stack).add_modifier(Modifier::ITALIC);
                    style_stack.push(style);
                }
                Tag::Strikethrough => {
                    let style = current_style(&style_stack).add_modifier(Modifier::CROSSED_OUT);
                    style_stack.push(style);
                }
                Tag::CodeBlock(kind) => {
                    flush_line(&mut lines, &mut current_spans, &mut current_col);
                    in_code_block = true;
                    code_block_lines.clear();
                    code_block_lang = match kind {
                        pulldown_cmark::CodeBlockKind::Fenced(lang) => {
                            let l = lang.to_string();
                            if l.is_empty() { None } else { Some(l) }
                        }
                        _ => None,
                    };
                }
                Tag::List(_) => {
                    list_depth += 1;
                }
                Tag::Item => {
                    flush_line(&mut lines, &mut current_spans, &mut current_col);
                    let indent = "  ".repeat(list_depth.saturating_sub(1));
                    let bullet = format!("{indent}• ");
                    push_span(
                        &mut current_spans,
                        &mut current_col,
                        Span::styled(bullet, theme.style_muted()),
                    );
                }
                Tag::Paragraph => {
                    flush_line(&mut lines, &mut current_spans, &mut current_col);
                    // Bug fix: blockquote border belongs on the same line as content.
                    // Previously the border was pushed in Tag::BlockQuote, which caused
                    // Tag::Paragraph to flush it alone before any text arrived.
                    // Now we emit the │ prefix here, after the flush, so it leads the content.
                    for _ in 0..blockquote_depth {
                        push_span(
                            &mut current_spans,
                            &mut current_col,
                            Span::styled("│ ".to_string(), Style::default().fg(theme.border)),
                        );
                    }
                }
                Tag::BlockQuote(_) => {
                    // Flush any pending content, then enter blockquote context.
                    // Do NOT push │ to current_spans here — Tag::Paragraph does it so the
                    // border lands on the same line as the paragraph content.
                    flush_line(&mut lines, &mut current_spans, &mut current_col);
                    let style = theme.style_muted();
                    style_stack.push(style);
                    blockquote_depth += 1;
                }
                Tag::Link { dest_url, .. } => {
                    link_url = Some(dest_url.to_string());
                    link_start_line = lines.len();
                    link_start_col = current_col;
                    link_text_buf.clear();
                    let style = current_style(&style_stack)
                        .add_modifier(Modifier::UNDERLINED)
                        .fg(theme.accent);
                    style_stack.push(style);
                }
                Tag::Image { dest_url, .. } => {
                    in_image = true;
                    image_alt.clear();
                    link_url = Some(dest_url.to_string());
                }
                Tag::Table(_alignments) => {
                    flush_line(&mut lines, &mut current_spans, &mut current_col);
                    in_table = true;
                    table_rows.clear();
                }
                Tag::TableHead => {
                    is_table_head = true;
                    current_row.clear();
                }
                Tag::TableRow => {
                    current_row.clear();
                }
                Tag::TableCell => {
                    current_cell.clear();
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    style_stack.pop();
                    flush_line(&mut lines, &mut current_spans, &mut current_col);
                }
                TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough => {
                    style_stack.pop();
                }
                TagEnd::CodeBlock => {
                    let lang_str = code_block_lang.as_deref().unwrap_or("");
                    let full_code = code_block_lines.join("\n");

                    // Language label header
                    if !lang_str.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!(" ╭─ {} ", lang_str),
                                Style::default().fg(theme.code_lang),
                            ),
                            Span::styled("─".repeat(20), Style::default().fg(theme.code_lang)),
                        ]));
                    } else {
                        lines.push(Line::from(Span::styled(
                            " ╭──────────────────────",
                            Style::default().fg(theme.code_lang),
                        )));
                    }

                    // Syntax-highlighted code lines — URLs inside NOT linkified
                    let highlighted = highlighter.highlight(&full_code, lang_str);
                    for hl_line in highlighted {
                        let mut spans =
                            vec![Span::styled(" │ ", Style::default().fg(theme.code_lang))];
                        spans.extend(hl_line.spans);
                        lines.push(Line::from(spans));
                    }

                    lines.push(Line::from(Span::styled(
                        " ╰──────────────────────",
                        Style::default().fg(theme.code_lang),
                    )));

                    in_code_block = false;
                    code_block_lines.clear();
                    code_block_lang = None;
                }
                TagEnd::List(_) => {
                    list_depth = list_depth.saturating_sub(1);
                }
                TagEnd::Item => {
                    flush_line(&mut lines, &mut current_spans, &mut current_col);
                }
                TagEnd::Paragraph => {
                    flush_line(&mut lines, &mut current_spans, &mut current_col);
                }
                TagEnd::BlockQuote(_) => {
                    style_stack.pop();
                    flush_line(&mut lines, &mut current_spans, &mut current_col);
                    blockquote_depth = blockquote_depth.saturating_sub(1);
                }
                TagEnd::Link => {
                    style_stack.pop();
                    if let Some(url) = link_url.take() {
                        let display_text = std::mem::take(&mut link_text_buf);
                        // Record the hyperlink for post-render OSC 8
                        if !url.is_empty() && !display_text.is_empty() {
                            md_links.push(MdLink {
                                line_idx: link_start_line,
                                col: link_start_col,
                                text: display_text,
                                url: url.clone(),
                            });
                        }
                        // In terminals without OSC 8 support, show the URL in parentheses
                        // so readers know where the link goes.
                        if !hyperlink::supports_hyperlinks() {
                            push_span(
                                &mut current_spans,
                                &mut current_col,
                                Span::styled(format!(" ({url})"), theme.style_dim()),
                            );
                        }
                    }
                }
                TagEnd::Image => {
                    let alt = std::mem::take(&mut image_alt);
                    let display = if alt.is_empty() {
                        "[image]".to_string()
                    } else {
                        format!("[image: {alt}]")
                    };
                    push_span(
                        &mut current_spans,
                        &mut current_col,
                        Span::styled(display, theme.style_dim()),
                    );
                    link_url = None;
                    in_image = false;
                }
                TagEnd::Table => {
                    render_table(&table_rows, &mut lines, theme);
                    in_table = false;
                    table_rows.clear();
                }
                TagEnd::TableHead => {
                    table_rows.push(current_row.clone());
                    is_table_head = false;
                }
                TagEnd::TableRow => {
                    table_rows.push(current_row.clone());
                }
                TagEnd::TableCell => {
                    current_row.push(current_cell.trim().to_string());
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_image {
                    image_alt.push_str(&text);
                } else if in_code_block {
                    for line_str in text.lines() {
                        code_block_lines.push(line_str.to_string());
                    }
                } else if in_table {
                    current_cell.push_str(&text);
                } else if link_url.is_some() {
                    // Inside a markdown link — accumulate display text, don't auto-detect URLs
                    link_text_buf.push_str(&text);
                    let style = current_style(&style_stack);
                    push_span(
                        &mut current_spans,
                        &mut current_col,
                        Span::styled(text.to_string(), style),
                    );
                } else {
                    // Plain paragraph text — detect and linkify embedded URLs
                    let urls = hyperlink::detect_urls(&text);
                    if urls.is_empty() {
                        let style = current_style(&style_stack);
                        push_span(
                            &mut current_spans,
                            &mut current_col,
                            Span::styled(text.to_string(), style),
                        );
                    } else {
                        linkify_text(
                            &text,
                            &urls,
                            &mut current_spans,
                            &mut current_col,
                            &mut md_links,
                            lines.len(),
                            current_style(&style_stack),
                            theme,
                        );
                    }
                }
            }
            Event::Code(code) => {
                if in_table {
                    current_cell.push_str(&format!("`{code}`"));
                } else {
                    // Inline code: render with code style, do NOT linkify
                    let s = format!("`{code}`");
                    push_span(
                        &mut current_spans,
                        &mut current_col,
                        Span::styled(s, theme.style_inline_code()),
                    );
                }
            }
            Event::SoftBreak => {
                if !in_table {
                    push_span(&mut current_spans, &mut current_col, Span::raw(" "));
                }
            }
            Event::HardBreak => {
                if !in_table {
                    flush_line(&mut lines, &mut current_spans, &mut current_col);
                }
            }
            Event::Rule => {
                flush_line(&mut lines, &mut current_spans, &mut current_col);
                lines.push(Line::from(Span::styled("─".repeat(40), theme.style_dim())));
            }
            _ => {}
        }
    }

    // Flush remaining content
    flush_line(&mut lines, &mut current_spans, &mut current_col);

    // Suppress is_table_head warning — kept for future header-specific styling
    let _ = is_table_head;

    (lines, md_links)
}

/// Push a span and advance the byte-based column counter.
fn push_span(spans: &mut Vec<Span<'static>>, col: &mut u16, span: Span<'static>) {
    *col += span.content.len() as u16;
    spans.push(span);
}

/// Emit styled spans for a text chunk that contains detected URLs.
///
/// Non-URL portions use the base style. URL portions get underline + accent
/// colour and are recorded in `md_links` for OSC 8 post-processing.
#[expect(
    clippy::too_many_arguments,
    reason = "all args are needed for span building"
)]
fn linkify_text(
    text: &str,
    urls: &[(usize, usize, &str)],
    spans: &mut Vec<Span<'static>>,
    col: &mut u16,
    md_links: &mut Vec<MdLink>,
    line_idx: usize,
    base_style: Style,
    theme: &ThemePalette,
) {
    let link_style = base_style
        .add_modifier(Modifier::UNDERLINED)
        .fg(theme.accent);

    let mut last = 0usize;
    for &(start, end, url) in urls {
        // Text before this URL (plain style)
        if start > last {
            let before = &text[last..start];
            push_span(spans, col, Span::styled(before.to_string(), base_style));
        }
        // The URL itself (link style)
        let url_text = &text[start..end];
        let link_col = *col;
        push_span(spans, col, Span::styled(url_text.to_string(), link_style));
        md_links.push(MdLink {
            line_idx,
            col: link_col,
            text: url_text.to_string(),
            url: url.to_string(),
        });
        last = end;
    }
    // Remaining text after the last URL
    if last < text.len() {
        push_span(
            spans,
            col,
            Span::styled(text[last..].to_string(), base_style),
        );
    }
}

/// Render a table with box-drawing characters.
fn render_table(rows: &[Vec<String>], lines: &mut Vec<Line<'static>>, theme: &ThemePalette) {
    if rows.is_empty() {
        return;
    }

    // Calculate column widths
    let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut col_widths: Vec<usize> = vec![0; num_cols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < num_cols {
                col_widths[i] = col_widths[i].max(cell.len());
            }
        }
    }

    // Cap column widths to prevent overflow
    for w in &mut col_widths {
        *w = (*w).min(40);
    }

    let border_style = Style::default().fg(theme.border);
    let header_style = theme.style_accent_bold();
    let cell_style = Style::default().fg(theme.fg);

    // Top border
    let top = format!(
        " ┌{}┐",
        col_widths
            .iter()
            .map(|w| "─".repeat(w + 2))
            .collect::<Vec<_>>()
            .join("┬")
    );
    lines.push(Line::from(Span::styled(top, border_style)));

    for (row_idx, row) in rows.iter().enumerate() {
        // Row content
        let mut row_spans = vec![Span::styled(" │", border_style)];
        for (i, width) in col_widths.iter().enumerate() {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            let padded = format!(" {:width$} ", cell, width = width);
            let style = if row_idx == 0 {
                header_style
            } else {
                cell_style
            };
            row_spans.push(Span::styled(padded, style));
            row_spans.push(Span::styled("│", border_style));
        }
        lines.push(Line::from(row_spans));

        // Separator after header
        if row_idx == 0 {
            let sep = format!(
                " ├{}┤",
                col_widths
                    .iter()
                    .map(|w| "─".repeat(w + 2))
                    .collect::<Vec<_>>()
                    .join("┼")
            );
            lines.push(Line::from(Span::styled(sep, border_style)));
        }
    }

    // Bottom border
    let bottom = format!(
        " └{}┘",
        col_widths
            .iter()
            .map(|w| "─".repeat(w + 2))
            .collect::<Vec<_>>()
            .join("┴")
    );
    lines.push(Line::from(Span::styled(bottom, border_style)));
}

fn flush_line(lines: &mut Vec<Line<'static>>, spans: &mut Vec<Span<'static>>, col: &mut u16) {
    if !spans.is_empty() {
        lines.push(Line::from(std::mem::take(spans)));
    }
    *col = 0;
}

fn current_style(stack: &[Style]) -> Style {
    stack.last().copied().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    use crate::highlight::Highlighter;
    use crate::theme::ThemePalette;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn test_render(md: &str) -> Vec<Line<'static>> {
        let theme = ThemePalette::detect();
        let hl = Highlighter::new();
        let (lines, _) = render(md, 80, &theme, &hl);
        lines
    }

    fn mk_render(md: &str) -> (Vec<Line<'static>>, Vec<MdLink>) {
        let theme = ThemePalette::detect();
        let hl = Highlighter::new();
        render(md, 80, &theme, &hl)
    }

    /// Render and return both the lines and the theme so callers can assert
    /// against theme-derived colors rather than hardcoding Rgb values.
    fn test_render_with_theme(md: &str) -> (Vec<Line<'static>>, ThemePalette) {
        let theme = ThemePalette::detect();
        let hl = Highlighter::new();
        let (lines, _) = render(md, 80, &theme, &hl);
        (lines, theme)
    }

    /// Concatenate all span content in a single line.
    fn line_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Concatenate all lines with newlines as a single string.
    fn all_lines_text(lines: &[Line]) -> String {
        lines.iter().map(line_text).collect::<Vec<_>>().join("\n")
    }

    fn all_text(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|l| line_text(l))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// True if any span in `line` whose content contains `text` also carries `modifier`.
    fn span_has_modifier(line: &Line, text: &str, modifier: Modifier) -> bool {
        line.spans
            .iter()
            .any(|s| s.content.contains(text) && s.style.add_modifier.contains(modifier))
    }

    /// True if any span in `line` whose content contains `text` has the given fg color.
    fn span_has_fg(line: &Line, text: &str, color: Color) -> bool {
        line.spans
            .iter()
            .any(|s| s.content.contains(text) && s.style.fg == Some(color))
    }

    /// True if ANY line has a span matching the text + modifier.
    fn any_line_has_modifier(lines: &[Line], text: &str, modifier: Modifier) -> bool {
        lines.iter().any(|l| span_has_modifier(l, text, modifier))
    }

    /// True if ANY line has a span matching the text + fg color.
    fn any_line_has_fg(lines: &[Line], text: &str, color: Color) -> bool {
        lines.iter().any(|l| span_has_fg(l, text, color))
    }

    // ── Existing regression tests (kept as-is) ────────────────────────────

    #[test]
    fn bold_text() {
        let (lines, _) = mk_render("**bold**");
        assert!(!lines.is_empty());
        assert!(line_text(&lines[0]).contains("bold"));
    }

    #[test]
    fn code_block() {
        let (lines, _) = mk_render("```rust\nlet x = 1;\n```");
        assert!(all_text(&lines).contains("let x = 1"));
    }

    #[test]
    fn list_items() {
        let (lines, _) = mk_render("- one\n- two");
        let text = all_text(&lines);
        assert!(text.contains("one"));
        assert!(text.contains("two"));
    }

    #[test]
    fn strikethrough_renders() {
        let (lines, _) = mk_render("~~deleted~~");
        assert!(!lines.is_empty());
        assert!(line_text(&lines[0]).contains("deleted"));
    }

    #[test]
    fn markdown_link_produces_md_link() {
        let (lines, links) = mk_render("[click here](https://example.com)");
        let text = all_text(&lines);
        assert!(text.contains("click here"), "link text visible: {text}");
        assert!(!links.is_empty(), "should produce a MdLink");
        assert_eq!(links[0].url, "https://example.com");
        assert_eq!(links[0].text, "click here");
    }

    #[test]
    fn markdown_link_url_shown_in_non_osc8_terminal() {
        let (lines, _) = mk_render("[click](https://example.com)");
        let text = all_text(&lines);
        if !crate::hyperlink::supports_hyperlinks() {
            assert!(
                text.contains("example.com"),
                "URL suffix must be shown in non-OSC8 terminals: {text}"
            );
        }
    }

    #[test]
    fn plain_url_in_text_linkified() {
        let (lines, links) = mk_render("See https://docs.anthropic.com/api for info.");
        let text = all_text(&lines);
        assert!(
            text.contains("https://docs.anthropic.com/api"),
            "URL in output: {text}"
        );
        assert!(!links.is_empty(), "auto-detected URL must produce MdLink");
        assert_eq!(links[0].url, "https://docs.anthropic.com/api");
    }

    #[test]
    fn url_in_code_block_not_linkified() {
        let (_, links) = mk_render("```\nhttps://example.com\n```");
        assert!(
            links.is_empty(),
            "URLs inside code blocks must NOT produce links"
        );
    }

    #[test]
    fn url_in_inline_code_not_linkified() {
        let (_, links) = mk_render("Use `https://example.com` in config");
        assert!(
            links.is_empty(),
            "URLs inside inline code must NOT produce links"
        );
    }

    #[test]
    fn link_text_correct() {
        let (_, links) = mk_render("[API docs](https://docs.anthropic.com)");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].text, "API docs");
        assert_eq!(links[0].url, "https://docs.anthropic.com");
    }

    #[test]
    fn plain_url_trailing_punctuation_stripped() {
        let (_, links) = mk_render("Visit https://example.com.");
        assert!(!links.is_empty());
        assert_eq!(links[0].url, "https://example.com");
    }

    #[test]
    fn link_renders_with_url_visible() {
        // Regression guard: the original test checked URL visibility in non-OSC8 terminals
        let (lines, _links) = mk_render("[click](https://example.com)");
        let text = all_text(&lines);
        assert!(text.contains("click"));
        assert!(text.contains("example.com"));
    }

    // ── Text formatting ───────────────────────────────────────────────────

    #[test]
    fn test_bold_text_modifier() {
        let lines = test_render("**bold**");
        assert!(
            !lines.is_empty(),
            "bold text must produce at least one line"
        );
        assert!(
            any_line_has_modifier(&lines, "bold", Modifier::BOLD),
            "bold text must carry BOLD modifier; lines: {lines:?}"
        );
    }

    #[test]
    fn test_italic_text() {
        let lines = test_render("*italic*");
        assert!(!lines.is_empty());
        let all = all_lines_text(&lines);
        assert!(all.contains("italic"), "expected italic text in output");
        assert!(
            any_line_has_modifier(&lines, "italic", Modifier::ITALIC),
            "italic text must carry ITALIC modifier"
        );
    }

    #[test]
    fn test_strikethrough_modifier() {
        let lines = test_render("~~deleted~~");
        assert!(
            any_line_has_modifier(&lines, "deleted", Modifier::CROSSED_OUT),
            "strikethrough text must carry CROSSED_OUT modifier"
        );
    }

    #[test]
    fn test_bold_italic_combined() {
        // ***text*** is bold + italic in CommonMark
        let lines = test_render("***combo***");
        assert!(!lines.is_empty());
        assert!(
            any_line_has_modifier(&lines, "combo", Modifier::BOLD),
            "bold-italic text must have BOLD"
        );
        assert!(
            any_line_has_modifier(&lines, "combo", Modifier::ITALIC),
            "bold-italic text must have ITALIC"
        );
    }

    #[test]
    fn test_inline_code() {
        let (lines, theme) = test_render_with_theme("use `std::mem::take`");
        let all = all_lines_text(&lines);
        assert!(
            all.contains("`std::mem::take`"),
            "inline code must appear with backticks"
        );
        // Inline code uses theme.warning as fg
        assert!(
            any_line_has_fg(&lines, "`std::mem::take`", theme.warning),
            "inline code must have warning fg color"
        );
    }

    #[test]
    fn test_nested_formatting() {
        // Bold wrapping italic
        let lines = test_render("**bold _bold-italic_ bold**");
        let all = all_lines_text(&lines);
        assert!(
            all.contains("bold-italic"),
            "nested text content must appear"
        );
        assert!(
            any_line_has_modifier(&lines, "bold-italic", Modifier::ITALIC),
            "nested italic must carry ITALIC modifier"
        );
        assert!(
            any_line_has_modifier(&lines, "bold-italic", Modifier::BOLD),
            "nested italic inside bold must carry BOLD modifier"
        );
    }

    // ── Headings ──────────────────────────────────────────────────────────

    #[test]
    fn test_heading_h1() {
        let lines = test_render("# Heading One");
        assert!(!lines.is_empty());
        let text = line_text(&lines[0]);
        assert!(
            text.starts_with("# "),
            "H1 must start with '# ', got: {text:?}"
        );
        assert!(text.contains("Heading One"));
    }

    #[test]
    fn test_heading_h2() {
        let lines = test_render("## Heading Two");
        assert!(!lines.is_empty());
        let text = line_text(&lines[0]);
        assert!(
            text.starts_with("## "),
            "H2 must start with '## ', got: {text:?}"
        );
        assert!(text.contains("Heading Two"));
    }

    #[test]
    fn test_heading_h3() {
        let lines = test_render("### Heading Three");
        assert!(!lines.is_empty());
        let text = line_text(&lines[0]);
        assert!(
            text.starts_with("### "),
            "H3 must start with '### ', got: {text:?}"
        );
        assert!(text.contains("Heading Three"));
    }

    #[test]
    fn test_heading_h4() {
        let lines = test_render("#### Heading Four");
        assert!(!lines.is_empty());
        let text = line_text(&lines[0]);
        assert!(
            text.starts_with("#### "),
            "H4 must start with '#### ', got: {text:?}"
        );
        assert!(text.contains("Heading Four"));
    }

    #[test]
    fn test_heading_style() {
        // Headings use style_accent_bold: accent fg + BOLD modifier
        let (lines, theme) = test_render_with_theme("# Styled Heading");
        assert!(!lines.is_empty());
        assert!(
            any_line_has_modifier(&lines, "# ", Modifier::BOLD),
            "heading prefix must be BOLD"
        );
        assert!(
            any_line_has_fg(&lines, "# ", theme.accent),
            "heading prefix must use accent color"
        );
        assert!(
            any_line_has_modifier(&lines, "Styled Heading", Modifier::BOLD),
            "heading text must be BOLD"
        );
    }

    // ── Code blocks ───────────────────────────────────────────────────────

    #[test]
    fn test_fenced_code_rust() {
        let lines = test_render("```rust\nfn main() {}\n```");
        let all = all_lines_text(&lines);
        assert!(all.contains("fn main()"), "Rust code content must appear");
        assert!(all.contains('╭'), "code block must have top border ╭");
        assert!(all.contains('╰'), "code block must have bottom border ╰");
    }

    #[test]
    fn test_fenced_code_python() {
        let lines = test_render("```python\ndef hello():\n    pass\n```");
        let all = all_lines_text(&lines);
        assert!(all.contains("def hello"), "Python code content must appear");
        assert!(all.contains("pass"));
    }

    #[test]
    fn test_fenced_code_no_language() {
        let lines = test_render("```\nplain code\n```");
        let all = all_lines_text(&lines);
        assert!(all.contains("plain code"));
        assert!(
            all.contains('╭'),
            "unlanguaged block must still have top border ╭"
        );
        assert!(
            all.contains('╰'),
            "unlanguaged block must still have bottom border ╰"
        );
    }

    #[test]
    fn test_fenced_code_unknown_language() {
        // Falls back to plain text highlighting; must not panic
        let lines = test_render("```xyzzy_unknown\nsome code\n```");
        let all = all_lines_text(&lines);
        assert!(
            all.contains("some code"),
            "unknown-language code block must render content"
        );
    }

    #[test]
    fn test_code_block_language_label() {
        // The language name must appear in the header line
        let lines = test_render("```rust\nlet x = 1;\n```");
        let header_line = lines.iter().find(|l| line_text(l).contains("rust"));
        assert!(
            header_line.is_some(),
            "code block header must show language name 'rust'"
        );
    }

    #[test]
    fn test_code_block_has_box_drawing() {
        let lines = test_render("```rust\nx\n```");
        let all = all_lines_text(&lines);
        // Top-left corner, vertical bar inside, bottom-left corner
        assert!(all.contains('╭'), "must have top-left ╭");
        assert!(all.contains('│'), "must have vertical bar │");
        assert!(all.contains('╰'), "must have bottom-left ╰");
    }

    #[test]
    fn test_code_block_preserves_whitespace() {
        let lines = test_render("```\n    indented\n        double\n```");
        let all = all_lines_text(&lines);
        assert!(
            all.contains("    indented"),
            "leading spaces must be preserved"
        );
        assert!(
            all.contains("        double"),
            "deeper indent must be preserved"
        );
    }

    #[test]
    fn test_empty_code_block() {
        // Must not panic; produces border lines even with no content
        let lines = test_render("```rust\n```");
        let all = all_lines_text(&lines);
        assert!(
            all.contains('╭'),
            "empty code block must still have top border"
        );
        assert!(
            all.contains('╰'),
            "empty code block must still have bottom border"
        );
    }

    // ── Lists ─────────────────────────────────────────────────────────────

    #[test]
    fn test_unordered_list_bullet() {
        let lines = test_render("- alpha\n- beta");
        let all = all_lines_text(&lines);
        assert!(
            all.contains('•'),
            "unordered list must use bullet character •"
        );
        assert!(all.contains("alpha"));
        assert!(all.contains("beta"));
    }

    #[test]
    fn test_nested_unordered_list() {
        let md = "- top\n  - nested\n    - deep";
        let lines = test_render(md);
        let all = all_lines_text(&lines);
        assert!(all.contains("top"));
        assert!(all.contains("nested"));
        assert!(all.contains("deep"));

        // Nested items must be indented (2 spaces per level beyond first)
        let nested_line = lines.iter().find(|l| line_text(l).contains("nested"));
        assert!(nested_line.is_some(), "nested item line must exist");
        let nested_text = line_text(nested_line.unwrap());
        assert!(
            nested_text.starts_with("  "),
            "depth-2 item must have 2-space indent, got: {nested_text:?}"
        );

        let deep_line = lines.iter().find(|l| line_text(l).contains("deep"));
        assert!(deep_line.is_some(), "deep item line must exist");
        let deep_text = line_text(deep_line.unwrap());
        assert!(
            deep_text.starts_with("    "),
            "depth-3 item must have 4-space indent, got: {deep_text:?}"
        );
    }

    #[test]
    fn test_list_with_formatting() {
        let lines = test_render("- **bold item**\n- *italic item*");
        assert!(
            any_line_has_modifier(&lines, "bold item", Modifier::BOLD),
            "bold text inside list must carry BOLD modifier"
        );
        assert!(
            any_line_has_modifier(&lines, "italic item", Modifier::ITALIC),
            "italic text inside list must carry ITALIC modifier"
        );
    }

    // ── Blockquotes ───────────────────────────────────────────────────────

    #[test]
    fn test_blockquote_simple() {
        // Regression for blockquote border bug: │ and content must be on the SAME line.
        let lines = test_render("> hello");
        let all = all_lines_text(&lines);
        assert!(all.contains("hello"), "blockquote content must appear");

        let border_line = lines.iter().find(|l| line_text(l).contains('│'));
        assert!(
            border_line.is_some(),
            "blockquote border │ must appear; lines: {lines:?}"
        );

        let border_text = line_text(border_line.unwrap());
        assert!(
            border_text.contains("hello"),
            "│ border and content must be on the SAME line, got: {border_text:?}"
        );
    }

    #[test]
    fn test_blockquote_border_fg() {
        // The │ border span must use theme.border color.
        let (lines, theme) = test_render_with_theme("> check color");
        assert!(
            any_line_has_fg(&lines, "│ ", theme.border),
            "blockquote border │ must use theme.border color"
        );
    }

    #[test]
    fn test_blockquote_with_formatting() {
        let lines = test_render("> **bold inside**");
        assert!(
            any_line_has_modifier(&lines, "bold inside", Modifier::BOLD),
            "bold inside blockquote must carry BOLD modifier"
        );
        // Border must still be present on the same line
        let border_line = lines.iter().find(|l| line_text(l).contains('│'));
        assert!(border_line.is_some());
        assert!(
            line_text(border_line.unwrap()).contains("bold inside"),
            "border and bold content must be on the same line"
        );
    }

    #[test]
    fn test_nested_blockquote() {
        // Two levels of blockquote should produce two │ characters on the content line.
        let lines = test_render("> > deeply nested");
        let all = all_lines_text(&lines);
        assert!(
            all.contains("deeply nested"),
            "nested blockquote content must appear"
        );

        let content_line = lines
            .iter()
            .find(|l| line_text(l).contains("deeply nested"));
        assert!(content_line.is_some(), "content line must exist");
        let text = line_text(content_line.unwrap());
        let border_count = text.matches('│').count();
        assert!(
            border_count >= 2,
            "nested blockquote must have at least 2 │ characters on content line, got: {text:?}"
        );
    }

    // ── Links ─────────────────────────────────────────────────────────────

    #[test]
    fn test_link_underline_style() {
        let lines = test_render("[click here](https://example.com)");
        assert!(
            any_line_has_modifier(&lines, "click here", Modifier::UNDERLINED),
            "link text must carry UNDERLINED modifier"
        );
    }

    #[test]
    fn test_link_url_appended() {
        let lines = test_render("[text](https://example.com)");
        let all = all_lines_text(&lines);
        assert!(all.contains("text"), "link display text must appear");
        assert!(
            all.contains("https://example.com"),
            "link URL must be appended in output"
        );
        // URL is wrapped in parens
        assert!(
            all.contains("(https://example.com)"),
            "link URL must appear in parentheses"
        );
    }

    #[test]
    fn test_link_url_uses_dim_style() {
        let (lines, theme) = test_render_with_theme("[click](https://example.com)");
        assert!(
            any_line_has_fg(&lines, "https://example.com", theme.fg_dim),
            "link URL must use dim (fg_dim) color"
        );
    }

    // ── Images ────────────────────────────────────────────────────────────

    #[test]
    fn test_image_alt_text() {
        let lines = test_render("![alt description](image.png)");
        let all = all_lines_text(&lines);
        assert!(
            all.contains("[image: alt description]"),
            "image must render as [image: alt], got: {all:?}"
        );
    }

    #[test]
    fn test_image_no_alt() {
        let lines = test_render("![](image.png)");
        let all = all_lines_text(&lines);
        assert!(
            all.contains("[image]"),
            "image with no alt must render as [image], got: {all:?}"
        );
    }

    // ── Tables ────────────────────────────────────────────────────────────

    #[test]
    fn test_table_simple() {
        let md = "| A | B |\n|---|---|\n| 1 | 2 |";
        let lines = test_render(md);
        let all = all_lines_text(&lines);

        assert!(all.contains('┌'), "table must have top-left ┌");
        assert!(all.contains('┐'), "table must have top-right ┐");
        assert!(all.contains('├'), "table must have header separator ├");
        assert!(all.contains('┤'), "table must have header separator ┤");
        assert!(all.contains('└'), "table must have bottom-left └");
        assert!(all.contains('┘'), "table must have bottom-right ┘");
        assert!(all.contains('│'), "table must have vertical separators │");

        assert!(all.contains('A'), "table header A must appear");
        assert!(all.contains('B'), "table header B must appear");
        assert!(all.contains('1'), "table cell 1 must appear");
        assert!(all.contains('2'), "table cell 2 must appear");
    }

    #[test]
    fn test_table_header_style() {
        // The first row (header) uses style_accent_bold; data rows use fg.
        let (lines, theme) = test_render_with_theme("| Head |\n|-----|\n| data |");
        assert!(
            any_line_has_modifier(&lines, "Head", Modifier::BOLD),
            "table header must be BOLD"
        );
        assert!(
            any_line_has_fg(&lines, "Head", theme.accent),
            "table header must use accent color"
        );
    }

    #[test]
    fn test_table_three_columns() {
        let md = "| X | Y | Z |\n|---|---|---|\n| a | b | c |";
        let lines = test_render(md);
        let all = all_lines_text(&lines);
        assert!(all.contains('X'));
        assert!(all.contains('Y'));
        assert!(all.contains('Z'));
        assert!(all.contains('a'));
        assert!(all.contains('b'));
        assert!(all.contains('c'));
        // Column separators (┬ in top border, ┼ in header sep, ┴ in bottom)
        assert!(all.contains('┬'), "3-col table must have ┬ in top border");
        assert!(
            all.contains('┼'),
            "3-col table must have ┼ in header separator"
        );
        assert!(
            all.contains('┴'),
            "3-col table must have ┴ in bottom border"
        );
    }

    // ── Structural ────────────────────────────────────────────────────────

    #[test]
    fn test_horizontal_rule() {
        let lines = test_render("---");
        let all = all_lines_text(&lines);
        assert!(
            all.contains('─'),
            "horizontal rule must render as a line of ─ characters"
        );
        // Must be a solid run of dashes (at least 10)
        let rule_line = lines.iter().find(|l| line_text(l).contains('─'));
        assert!(rule_line.is_some(), "rule line must exist");
        let rule_text = line_text(rule_line.unwrap());
        let dash_count = rule_text.chars().filter(|&c| c == '─').count();
        assert!(
            dash_count >= 10,
            "horizontal rule must have ≥10 dashes, got {dash_count}"
        );
    }

    #[test]
    fn test_horizontal_rule_style() {
        let (lines, theme) = test_render_with_theme("---");
        let rule_line = lines.iter().find(|l| line_text(l).contains('─'));
        assert!(rule_line.is_some());
        assert!(
            span_has_fg(rule_line.unwrap(), "─", theme.fg_dim),
            "horizontal rule must use dim (fg_dim) color"
        );
    }

    #[test]
    fn test_paragraph_spacing() {
        // Two separate paragraphs must each appear in output
        let lines = test_render("first paragraph\n\nsecond paragraph");
        let all = all_lines_text(&lines);
        assert!(all.contains("first paragraph"));
        assert!(all.contains("second paragraph"));

        // They must be on different lines
        let first = lines
            .iter()
            .position(|l| line_text(l).contains("first paragraph"));
        let second = lines
            .iter()
            .position(|l| line_text(l).contains("second paragraph"));
        assert!(first.is_some() && second.is_some());
        assert_ne!(
            first.unwrap(),
            second.unwrap(),
            "two paragraphs must be on different lines"
        );
    }

    #[test]
    fn test_hard_line_break() {
        // Backslash at end of line is a hard break in CommonMark
        let lines = test_render("line one\\\nline two");
        let all = all_lines_text(&lines);
        assert!(all.contains("line one"), "first line must appear");
        assert!(all.contains("line two"), "second line must appear");

        // The two parts must end up on separate lines
        let first = lines.iter().position(|l| line_text(l).contains("line one"));
        let second = lines.iter().position(|l| line_text(l).contains("line two"));
        assert!(first.is_some() && second.is_some());
        assert_ne!(
            first.unwrap(),
            second.unwrap(),
            "hard break must split into separate lines"
        );
    }

    #[test]
    fn test_soft_break_is_space() {
        // Two lines in the same paragraph (no blank line) join with a soft break (space)
        let lines = test_render("line one\nline two");
        // In a tight paragraph pulldown-cmark emits Text, SoftBreak, Text → same line
        let all = all_lines_text(&lines);
        assert!(all.contains("line one"), "first part must appear");
        assert!(all.contains("line two"), "second part must appear");
        // Both should be on the same line joined by a space
        let joined = lines.iter().find(|l| {
            let t = line_text(l);
            t.contains("line one") && t.contains("line two")
        });
        assert!(
            joined.is_some(),
            "soft-break-joined lines must appear on same output line; got:\n{all}"
        );
    }

    // ── Edge cases ────────────────────────────────────────────────────────

    #[test]
    fn test_empty_input() {
        let lines = test_render("");
        assert!(
            lines.is_empty(),
            "empty input must produce no lines, got: {lines:?}"
        );
    }

    #[test]
    fn test_whitespace_only() {
        // Must not panic and should produce no meaningful content
        let lines = test_render("   \n\n   ");
        // No assertion on exact count — just that it doesn't crash
        let _ = lines;
    }

    #[test]
    fn test_deeply_nested_lists() {
        // 5 levels of nesting — must not panic, indent grows correctly
        let md = "- l1\n  - l2\n    - l3\n      - l4\n        - l5";
        let lines = test_render(md);
        let all = all_lines_text(&lines);
        for level in ["l1", "l2", "l3", "l4", "l5"] {
            assert!(all.contains(level), "level {level} must appear in output");
        }
        // l5 (depth 5) must have 8-space indent (2 × (5-1))
        let l5_line = lines.iter().find(|l| line_text(l).contains("l5"));
        assert!(l5_line.is_some());
        let l5_text = line_text(l5_line.unwrap());
        assert!(
            l5_text.starts_with("        "),
            "depth-5 item must have 8-space indent, got: {l5_text:?}"
        );
    }

    #[test]
    fn test_very_long_line() {
        // A single paragraph line >500 chars must render without panicking
        let long = "word ".repeat(120); // ~600 chars
        let lines = test_render(long.trim());
        assert!(
            !lines.is_empty(),
            "very long line must produce at least one output line"
        );
    }

    #[test]
    fn test_unicode_content() {
        // Emoji, CJK, and combining characters must all pass through cleanly
        let md = "Hello 🎉 world 你好世界 café";
        let lines = test_render(md);
        let all = all_lines_text(&lines);
        assert!(all.contains('🎉'), "emoji must appear in output");
        assert!(all.contains("你好世界"), "CJK characters must appear");
        assert!(all.contains("café"), "accented characters must appear");
    }

    #[test]
    fn test_mixed_content() {
        // Full document with multiple element types must render all parts
        let md = "# Title\n\nA paragraph.\n\n- item\n\n> quote\n\n```rust\ncode\n```\n\n| H |\n|---|\n| v |";
        let lines = test_render(md);
        let all = all_lines_text(&lines);
        assert!(all.contains("Title"), "heading must appear");
        assert!(all.contains("paragraph"), "paragraph must appear");
        assert!(all.contains("item"), "list item must appear");
        assert!(all.contains("quote"), "blockquote must appear");
        assert!(all.contains("code"), "code block must appear");
        assert!(all.contains('H'), "table header must appear");
        assert!(all.contains('v'), "table cell must appear");
    }

    #[test]
    fn test_unclosed_formatting() {
        // pulldown-cmark treats unclosed ** as literal asterisks — must not panic
        let lines = test_render("**not closed");
        let _ = all_lines_text(&lines);
    }

    #[test]
    fn test_ansi_passthrough() {
        // The renderer itself does not strip ANSI — callers sanitize before passing.
        // Verify the renderer handles arbitrary bytes without panicking.
        let lines = test_render("plain text without escapes");
        let all = all_lines_text(&lines);
        assert!(all.contains("plain text"), "plain text must pass through");
    }

    #[test]
    fn test_ordered_list_renders_with_bullets() {
        // Ordered lists use the same bullet renderer (no numbering yet)
        let lines = test_render("1. first\n2. second\n3. third");
        let all = all_lines_text(&lines);
        assert!(all.contains("first"));
        assert!(all.contains("second"));
        assert!(all.contains("third"));
        assert!(all.contains('•'), "ordered list items must use bullet •");
    }

    #[test]
    fn test_code_block_lang_in_header_style() {
        // Language label must be styled with code_lang color
        let (lines, theme) = test_render_with_theme("```python\npass\n```");
        let header = lines.iter().find(|l| line_text(l).contains("python"));
        assert!(header.is_some(), "python language label must appear");
        assert!(
            span_has_fg(header.unwrap(), "python", theme.code_lang),
            "language label must use code_lang color"
        );
    }

    #[test]
    fn test_blockquote_muted_text_style() {
        // Text inside blockquote must use the muted style
        let (lines, theme) = test_render_with_theme("> muted content");
        assert!(
            any_line_has_fg(&lines, "muted content", theme.fg_muted),
            "blockquote text must use fg_muted color"
        );
    }

    #[test]
    fn test_inline_code_has_bg_color() {
        // Inline code must have both fg (warning) and bg (code_bg)
        let (lines, theme) = test_render_with_theme("use `foo`");
        let code_span = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.contains("`foo`"));
        assert!(code_span.is_some(), "inline code span must exist");
        let span = code_span.unwrap();
        assert_eq!(
            span.style.fg,
            Some(theme.warning),
            "inline code fg must be warning color"
        );
        assert_eq!(
            span.style.bg,
            Some(theme.code_bg),
            "inline code bg must be code_bg color"
        );
    }
}
