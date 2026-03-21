use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::highlight::Highlighter;
use crate::hyperlink::{self, MdLink};
use crate::theme::Theme;

mod helpers;
use helpers::{current_style, flush_line, linkify_text, push_span, render_table};

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
pub(crate) fn render(
    text: &str,
    _width: usize,
    theme: &Theme,
    highlighter: &Highlighter,
) -> (Vec<Line<'static>>, Vec<MdLink>) {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(text, options);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    // WHY: byte length matches scroll calculation in view/chat.rs which uses span.content.len()
    let mut current_col: u16 = 0;
    let mut style_stack: Vec<Style> = vec![Style::default().fg(theme.text.fg)];
    let mut in_code_block = false;
    let mut code_block_lines: Vec<String> = Vec::new();
    let mut code_block_lang: Option<String> = None;
    let mut list_depth: usize = 0;

    let mut link_url: Option<String> = None;
    let mut link_start_line: usize = 0;
    let mut link_start_col: u16 = 0;
    let mut link_text_buf: String = String::new();

    let mut in_image = false;
    let mut image_alt = String::new();

    let mut in_table = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell = String::new();
    let mut is_table_head = false;

    // WHY: paragraph start prepends │ (not BlockQuote) so the border lands on the same line as content
    let mut blockquote_depth: usize = 0;

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
                        pulldown_cmark::CodeBlockKind::Indented => None,
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
                    // WHY: emit │ prefix here after flush so it leads content: if pushed in
                    // Tag::BlockQuote, Paragraph would flush it alone before any text arrived.
                    for _ in 0..blockquote_depth {
                        push_span(
                            &mut current_spans,
                            &mut current_col,
                            Span::styled(
                                "│ ".to_string(),
                                Style::default().fg(theme.borders.normal),
                            ),
                        );
                    }
                }
                Tag::BlockQuote(_) => {
                    // WHY: do not push │ here: Tag::Paragraph does it so the border lands on content line
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
                        .fg(theme.colors.accent);
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
                _ => {
                    // NOTE: unhandled start tags (footnotes, metadata) are ignored
                }
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

                    if !lang_str.is_empty() {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!(" ╭─ {} ", lang_str),
                                Style::default().fg(theme.code.lang),
                            ),
                            Span::styled("─".repeat(20), Style::default().fg(theme.code.lang)),
                        ]));
                    } else {
                        lines.push(Line::from(Span::styled(
                            " ╭──────────────────────",
                            Style::default().fg(theme.code.lang),
                        )));
                    }

                    // NOTE: URLs inside code blocks are not linkified
                    let highlighted = highlighter.highlight(&full_code, lang_str);
                    for hl_line in highlighted {
                        let mut spans =
                            vec![Span::styled(" │ ", Style::default().fg(theme.code.lang))];
                        spans.extend(hl_line.spans);
                        lines.push(Line::from(spans));
                    }

                    lines.push(Line::from(Span::styled(
                        " ╰──────────────────────",
                        Style::default().fg(theme.code.lang),
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
                        if !url.is_empty() && !display_text.is_empty() {
                            md_links.push(MdLink {
                                line_idx: link_start_line,
                                col: link_start_col,
                                text: display_text,
                                url: url.clone(),
                            });
                        }
                        // NOTE: terminals without OSC 8 support show URL in parentheses as fallback
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
                _ => {
                    // NOTE: unhandled end tags are ignored
                }
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
                    // NOTE: inside a markdown link: accumulate display text, URL detection skipped
                    link_text_buf.push_str(&text);
                    let style = current_style(&style_stack);
                    push_span(
                        &mut current_spans,
                        &mut current_col,
                        Span::styled(text.to_string(), style),
                    );
                } else {
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
                    // NOTE: inline code: rendered with code style, URLs not linkified
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
            _ => {
                // NOTE: unhandled markdown events (footnotes, etc.) are ignored
            }
        }
    }

    flush_line(&mut lines, &mut current_spans, &mut current_col);

    // WHY: suppress unused warning: kept for future header-specific styling
    let _ = is_table_head;

    (lines, md_links)
}

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing for clarity"
)]
mod tests;
