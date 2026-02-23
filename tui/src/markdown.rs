use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

/// Render markdown text into ratatui Lines.
/// Phase 1: bold, italic, code, headings, lists, code blocks.
/// Phase 2 adds: tables, links, syntax highlighting.
pub fn render(text: &str, _width: usize) -> Vec<Line<'static>> {
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(text, options);

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default()];
    let mut in_code_block = false;
    let mut code_block_lines: Vec<String> = Vec::new();
    let mut list_depth: usize = 0;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    flush_line(&mut lines, &mut current_spans);
                    let style = Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD);
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
                Tag::CodeBlock(_) => {
                    flush_line(&mut lines, &mut current_spans);
                    in_code_block = true;
                    code_block_lines.clear();
                }
                Tag::List(_) => {
                    list_depth += 1;
                }
                Tag::Item => {
                    flush_line(&mut lines, &mut current_spans);
                    let indent = "  ".repeat(list_depth.saturating_sub(1));
                    current_spans.push(Span::raw(format!("{}• ", indent)));
                }
                Tag::Paragraph => {
                    flush_line(&mut lines, &mut current_spans);
                }
                Tag::BlockQuote(_) => {
                    let style = Style::default().fg(Color::DarkGray);
                    style_stack.push(style);
                    current_spans.push(Span::styled("│ ".to_string(), style));
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
                    // Render code block with background
                    let code_style = Style::default().fg(Color::White).bg(Color::Rgb(40, 40, 40));
                    for code_line in &code_block_lines {
                        lines.push(Line::from(Span::styled(
                            format!(" {} ", code_line),
                            code_style,
                        )));
                    }
                    in_code_block = false;
                    code_block_lines.clear();
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
                let style = Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::Rgb(40, 40, 40));
                current_spans.push(Span::styled(format!("`{}`", code), style));
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
                    Style::default().fg(Color::DarkGray),
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
