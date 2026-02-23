use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, ToolCallInfo};
use crate::markdown;
use crate::theme::{self, ThemePalette};

pub fn render(app: &App, frame: &mut Frame, area: Rect, theme: &ThemePalette) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();

    // Small top padding
    lines.push(Line::raw(""));

    // Render each message
    for msg in &app.messages {
        render_message(app, msg, &mut lines, inner_width, theme);
    }

    // Streaming response (in progress)
    if !app.streaming_text.is_empty()
        || !app.streaming_thinking.is_empty()
        || app.active_turn_id.is_some()
    {
        render_streaming(app, &mut lines, inner_width, theme);
    }

    // Calculate scroll — must account for line wrapping.
    let visible_height = area.height.saturating_sub(2) as usize;
    let wrap_width = area.width.saturating_sub(2).max(1) as usize;
    let total_visual_lines: usize = lines
        .iter()
        .map(|line| {
            let line_width: usize = line.spans.iter().map(|s| s.content.len()).sum();
            if line_width == 0 {
                1
            } else {
                line_width.div_ceil(wrap_width)
            }
        })
        .sum();
    let scroll = if app.auto_scroll {
        total_visual_lines.saturating_sub(visible_height)
    } else {
        total_visual_lines
            .saturating_sub(visible_height)
            .saturating_sub(app.scroll_offset)
    };

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll as u16, 0));

    frame.render_widget(paragraph, area);
}

fn render_message(
    app: &App,
    msg: &crate::app::ChatMessage,
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    theme: &ThemePalette,
) {
    let (role_label, role_style) = match msg.role.as_str() {
        "user" => ("you".to_string(), theme.style_user()),
        "assistant" => {
            let name = app
                .focused_agent
                .as_ref()
                .and_then(|id| app.agents.iter().find(|a| a.id == *id))
                .map(|a| a.name.to_lowercase())
                .unwrap_or_else(|| "assistant".to_string());
            (name, theme.style_assistant())
        }
        _ => ("system".to_string(), theme.style_muted()),
    };

    // Header: role name + optional model (dim) + timestamp
    let mut header_spans = vec![Span::styled(format!(" {}", role_label), role_style)];

    if let Some(ref model) = msg.model {
        let short_model = model.split('/').next_back().unwrap_or(model);
        header_spans.push(Span::styled(
            format!(" · {}", short_model),
            theme.style_dim(),
        ));
    }

    if let Some(ref ts) = msg.timestamp {
        let time_str = ts
            .split('T')
            .nth(1)
            .and_then(|t| t.split('.').next())
            .unwrap_or(ts);
        header_spans.push(Span::styled(format!("  {}", time_str), theme.style_dim()));
    }

    lines.push(Line::from(header_spans));

    // Inline tool call summary (compact, between header and content)
    if !msg.tool_calls.is_empty() {
        render_tool_summary(&msg.tool_calls, lines, theme);
    }

    // Message content — markdown parsed with syntax highlighting
    let rendered = markdown::render(
        &msg.text,
        inner_width.saturating_sub(2),
        theme,
        &app.highlighter,
    );
    for line in rendered {
        let mut padded_spans = vec![Span::raw(" ")];
        padded_spans.extend(line.spans);
        lines.push(Line::from(padded_spans));
    }

    // Breathing room between messages
    lines.push(Line::raw(""));
}

/// Render a compact tool call summary line:
///   ╰─ exec (0.3s) → read (0.1s) → grep (0.2s)
fn render_tool_summary(
    tools: &[ToolCallInfo],
    lines: &mut Vec<Line<'static>>,
    theme: &ThemePalette,
) {
    let mut spans: Vec<Span> = vec![Span::raw("  "), Span::styled("╰─ ", theme.style_dim())];

    for (i, tc) in tools.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" → ", theme.style_dim()));
        }

        let color = if tc.is_error {
            theme.error
        } else {
            theme.fg_dim
        };
        let icon = if tc.is_error { "✗ " } else { "" };

        let label = if let Some(ms) = tc.duration_ms {
            if ms >= 1000 {
                format!("{}{} ({:.1}s)", icon, tc.name, ms as f64 / 1000.0)
            } else {
                format!("{}{}  ({}ms)", icon, tc.name, ms)
            }
        } else {
            format!("{}{}", icon, tc.name)
        };

        spans.push(Span::styled(label, Style::default().fg(color)));
    }

    lines.push(Line::from(spans));
}

fn render_streaming(
    app: &App,
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    theme: &ThemePalette,
) {
    let name = app
        .focused_agent
        .as_ref()
        .and_then(|id| app.agents.iter().find(|a| a.id == *id))
        .map(|a| a.name.to_lowercase())
        .unwrap_or_else(|| "assistant".to_string());

    // Thinking block (if visible)
    if app.thinking_expanded && !app.streaming_thinking.is_empty() {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("─── thinking ", Style::default().fg(theme.thinking_border)),
            Span::styled(
                "─".repeat(inner_width.saturating_sub(16).min(40)),
                Style::default().fg(theme.thinking_border),
            ),
        ]));
        for line in app.streaming_thinking.lines() {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(line.to_string(), Style::default().fg(theme.thinking)),
            ]));
        }
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "─".repeat(inner_width.saturating_sub(4).min(40)),
                Style::default().fg(theme.thinking_border),
            ),
        ]));
    }

    // Active tool calls during streaming (show completed + current)
    if !app.streaming_tool_calls.is_empty() {
        let mut tool_spans: Vec<Span> =
            vec![Span::raw("  "), Span::styled("╰─ ", theme.style_dim())];

        for (i, tc) in app.streaming_tool_calls.iter().enumerate() {
            if i > 0 {
                tool_spans.push(Span::styled(" → ", theme.style_dim()));
            }

            if tc.duration_ms.is_some() {
                // Completed tool
                let color = if tc.is_error {
                    theme.error
                } else {
                    theme.fg_dim
                };
                let icon = if tc.is_error { "✗ " } else { "" };
                let label = if let Some(ms) = tc.duration_ms {
                    if ms >= 1000 {
                        format!("{}{} ({:.1}s)", icon, tc.name, ms as f64 / 1000.0)
                    } else {
                        format!("{}{} ({}ms)", icon, tc.name, ms)
                    }
                } else {
                    tc.name.clone()
                };
                tool_spans.push(Span::styled(label, Style::default().fg(color)));
            } else {
                // Currently running tool — animated
                let ch = theme::spinner_frame(app.tick_count);
                tool_spans.push(Span::styled(
                    format!("{} {}", ch, tc.name),
                    Style::default().fg(theme.spinner),
                ));
            }
        }

        lines.push(Line::from(tool_spans));
    }

    // Streaming text with cursor
    if !app.streaming_text.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            format!(" {}", name),
            theme.style_assistant(),
        )]));

        // Use cached markdown if available
        let rendered = if app.cached_markdown_text == app.streaming_text {
            app.cached_markdown_lines.clone()
        } else {
            markdown::render(
                &app.streaming_text,
                inner_width.saturating_sub(2),
                theme,
                &app.highlighter,
            )
        };

        for line in rendered {
            let mut padded_spans = vec![Span::raw(" ")];
            padded_spans.extend(line.spans);
            lines.push(Line::from(padded_spans));
        }

        // Braille cursor
        let ch = theme::spinner_frame(app.tick_count);
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                ch.to_string(),
                Style::default()
                    .fg(theme.streaming)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    } else if app.active_turn_id.is_some() {
        // No text yet — show spinner with agent name
        let ch = theme::spinner_frame(app.tick_count);

        lines.push(Line::from(vec![
            Span::styled(format!(" {}", name), theme.style_assistant()),
            Span::styled(format!(" {} thinking…", ch), theme.style_muted()),
        ]));
    }
}
