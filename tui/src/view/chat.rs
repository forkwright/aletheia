use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
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
    if !app.streaming_text.is_empty() || !app.streaming_thinking.is_empty() {
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
                (line_width + wrap_width - 1) / wrap_width
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

    // Header: role name + optional model (dim) + timestamp right-aligned
    let mut header_spans = vec![Span::styled(format!(" {}", role_label), role_style)];

    if let Some(ref model) = msg.model {
        // Extract just the model name (strip provider prefix if present)
        let short_model = model
            .split('/')
            .last()
            .unwrap_or(model);
        header_spans.push(Span::styled(
            format!(" · {}", short_model),
            theme.style_dim(),
        ));
    }

    if let Some(ref ts) = msg.timestamp {
        // Show just the time portion if available
        let time_str = ts
            .split('T')
            .nth(1)
            .and_then(|t| t.split('.').next())
            .unwrap_or(ts);
        header_spans.push(Span::styled(
            format!("  {}", time_str),
            theme.style_dim(),
        ));
    }

    lines.push(Line::from(header_spans));

    // Message content — markdown parsed, indented
    let rendered = markdown::render(&msg.text, inner_width.saturating_sub(2), theme);
    for line in rendered {
        // Add left padding for visual hierarchy
        let mut padded_spans = vec![Span::raw(" ")];
        padded_spans.extend(line.spans);
        lines.push(Line::from(padded_spans));
    }

    // Blank line between messages (breathing room)
    lines.push(Line::raw(""));
}

fn render_streaming(
    app: &App,
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    theme: &ThemePalette,
) {
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
                Span::styled(
                    line.to_string(),
                    Style::default().fg(theme.thinking),
                ),
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

    // Streaming text with cursor
    if !app.streaming_text.is_empty() {
        let name = app
            .focused_agent
            .as_ref()
            .and_then(|id| app.agents.iter().find(|a| a.id == *id))
            .map(|a| a.name.to_lowercase())
            .unwrap_or_else(|| "assistant".to_string());

        lines.push(Line::from(vec![
            Span::styled(format!(" {}", name), theme.style_assistant()),
        ]));

        let rendered = markdown::render(&app.streaming_text, inner_width.saturating_sub(2), theme);
        for line in rendered {
            let mut padded_spans = vec![Span::raw(" ")];
            padded_spans.extend(line.spans);
            lines.push(Line::from(padded_spans));
        }

        // Braille cursor instead of block cursor
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
        // Thinking but no text yet — show spinner
        let ch = theme::spinner_frame(app.tick_count);
        let name = app
            .focused_agent
            .as_ref()
            .and_then(|id| app.agents.iter().find(|a| a.id == *id))
            .map(|a| a.name.to_lowercase())
            .unwrap_or_else(|| "assistant".to_string());

        lines.push(Line::from(vec![
            Span::styled(format!(" {}", name), theme.style_assistant()),
            Span::styled(
                format!(" {} thinking…", ch),
                theme.style_muted(),
            ),
        ]));
    }
}
