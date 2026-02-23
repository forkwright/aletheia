use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::highlight::Highlighter;
use crate::theme::ThemePalette;

/// Render markdown text into ratatui Lines.
/// Supports: bold, italic, code, headings, lists, code blocks (with syntax highlighting),
/// blockquotes, rules, and tables.
pub fn render(
    text: &str,
    _width: usize,
    theme: &ThemePalette,
    highlighter: &Highlighter,
) -> Vec<Line<'static>> {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(text, options);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default().fg(theme.fg)];
    let mut in_code_block = false;
    let mut code_block_lines: Vec<String> = Vec::new();
    let mut code_block_lang: Option<String> = None;
    let mut list_depth: usize = 0;

    // Table state
    let mut in_table = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_cell = String::new();
    let mut is_table_head = false;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    flush_line(&mut lines, &mut current_spans);
                    let style = theme.style_accent_bold();
                    style_stack.push(style);
                    let prefix = match level {
                        pulldown_cmark::HeadingLevel::H1 => "# ",
                        pulldown_cmark::HeadingLevel::H2 => "## ",
                        pulldown_cmark::HeadingLevel::H3 => "### ",
                        _ => "#### ",
                    };
                    current_spans.push(Span::styled(prefix.to_string(), style));
                }
                Tag::Strong => {
                    let style = current_style(&style_stack).add_modifier(Modifier::BOLD);
                    style_stack.push(style);
                }
                Tag::Emphasis => {
                    let style = current_style(&style_stack).add_modifier(Modifier::ITALIC);
                    style_stack.push(style);
                }
                Tag::CodeBlock(kind) => {
                    flush_line(&mut lines, &mut current_spans);
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
                    flush_line(&mut lines, &mut current_spans);
                    let indent = "  ".repeat(list_depth.saturating_sub(1));
                    current_spans.push(Span::styled(format!("{}• ", indent), theme.style_muted()));
                }
                Tag::Paragraph => {
                    flush_line(&mut lines, &mut current_spans);
                }
                Tag::BlockQuote(_) => {
                    let style = theme.style_muted();
                    style_stack.push(style);
                    current_spans.push(Span::styled(
                        "│ ".to_string(),
                        Style::default().fg(theme.border),
                    ));
                }
                Tag::Table(_alignments) => {
                    flush_line(&mut lines, &mut current_spans);
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
                    flush_line(&mut lines, &mut current_spans);
                }
                TagEnd::Strong | TagEnd::Emphasis => {
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

                    // Syntax-highlighted code lines
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
                    flush_line(&mut lines, &mut current_spans);
                }
                TagEnd::Paragraph => {
                    flush_line(&mut lines, &mut current_spans);
                }
                TagEnd::BlockQuote(_) => {
                    style_stack.pop();
                    flush_line(&mut lines, &mut current_spans);
                }
                TagEnd::Table => {
                    // Render the accumulated table
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
                if in_code_block {
                    for line in text.lines() {
                        code_block_lines.push(line.to_string());
                    }
                } else if in_table {
                    current_cell.push_str(&text);
                } else {
                    let style = current_style(&style_stack);
                    current_spans.push(Span::styled(text.to_string(), style));
                }
            }
            Event::Code(code) => {
                if in_table {
                    current_cell.push_str(&format!("`{}`", code));
                } else {
                    current_spans.push(Span::styled(
                        format!("`{}`", code),
                        theme.style_inline_code(),
                    ));
                }
            }
            Event::SoftBreak => {
                if !in_table {
                    current_spans.push(Span::raw(" "));
                }
            }
            Event::HardBreak => {
                if !in_table {
                    flush_line(&mut lines, &mut current_spans);
                }
            }
            Event::Rule => {
                flush_line(&mut lines, &mut current_spans);
                lines.push(Line::from(Span::styled("─".repeat(40), theme.style_dim())));
            }
            _ => {}
        }
    }

    // Flush remaining
    flush_line(&mut lines, &mut current_spans);

    // Suppress is_table_head warning — it's used for future header styling
    let _ = is_table_head;

    lines
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
        let mut spans = vec![Span::styled(" │", border_style)];
        for (i, width) in col_widths.iter().enumerate() {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            let padded = format!(" {:width$} ", cell, width = width);
            let style = if row_idx == 0 {
                header_style
            } else {
                cell_style
            };
            spans.push(Span::styled(padded, style));
            spans.push(Span::styled("│", border_style));
        }
        lines.push(Line::from(spans));

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

fn flush_line(lines: &mut Vec<Line<'static>>, spans: &mut Vec<Span<'static>>) {
    if !spans.is_empty() {
        lines.push(Line::from(std::mem::take(spans)));
    }
}

fn current_style(stack: &[Style]) -> Style {
    stack.last().copied().unwrap_or_default()
}
