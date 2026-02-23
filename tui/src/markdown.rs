use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::ThemePalette;

/// Render markdown text into ratatui Lines.
/// Phase 1: bold, italic, code, headings, lists, code blocks, blockquotes, rules.
/// Phase 2 adds: tables, links, syntax highlighting.
pub fn render(text: &str, _width: usize, theme: &ThemePalette) -> Vec<Line<'static>> {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(text, options);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default().fg(theme.fg)];
    let mut in_code_block = false;
    let mut code_block_lines: Vec<String> = Vec::new();
    let mut code_block_lang: Option<String> = None;
    let mut list_depth: usize = 0;

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
                    current_spans.push(Span::styled(
                        format!("{}• ", indent),
                        theme.style_muted(),
                    ));
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
                    // Language label
                    if let Some(ref lang) = code_block_lang {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!(" ╭─ {} ", lang),
                                Style::default().fg(theme.code_lang),
                            ),
                            Span::styled(
                                "─".repeat(20),
                                Style::default().fg(theme.code_lang),
                            ),
                        ]));
                    } else {
                        lines.push(Line::from(Span::styled(
                            " ╭──────────────────────",
                            Style::default().fg(theme.code_lang),
                        )));
                    }

                    // Code lines with background
                    let code_style = theme.style_code();
                    for code_line in &code_block_lines {
                        lines.push(Line::from(vec![
                            Span::styled(" │ ", Style::default().fg(theme.code_lang)),
                            Span::styled(code_line.to_string(), code_style),
                        ]));
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
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block {
                    for line in text.lines() {
                        code_block_lines.push(line.to_string());
                    }
                } else {
                    let style = current_style(&style_stack);
                    current_spans.push(Span::styled(text.to_string(), style));
                }
            }
            Event::Code(code) => {
                current_spans.push(Span::styled(
                    format!("`{}`", code),
                    theme.style_inline_code(),
                ));
            }
            Event::SoftBreak => {
                current_spans.push(Span::raw(" "));
            }
            Event::HardBreak => {
                flush_line(&mut lines, &mut current_spans);
            }
            Event::Rule => {
                flush_line(&mut lines, &mut current_spans);
                lines.push(Line::from(Span::styled(
                    "─".repeat(40),
                    theme.style_dim(),
                )));
            }
            _ => {}
        }
    }

    // Flush remaining
    flush_line(&mut lines, &mut current_spans);

    lines
}

fn flush_line(lines: &mut Vec<Line<'static>>, spans: &mut Vec<Span<'static>>) {
    if !spans.is_empty() {
        lines.push(Line::from(std::mem::take(spans)));
    }
}

fn current_style(stack: &[Style]) -> Style {
    stack.last().copied().unwrap_or_default()
}
