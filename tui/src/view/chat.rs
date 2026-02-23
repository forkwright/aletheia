use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::markdown;

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let inner_width = area.width.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();

    // Render each message
    for msg in &app.messages {
        // Role header
        let (role_label, role_color) = match msg.role.as_str() {
            "user" => ("you", Color::Cyan),
            "assistant" => {
                let name = app
                    .focused_agent
                    .as_ref()
                    .and_then(|id| app.agents.iter().find(|a| a.id == *id))
                    .map(|a| a.name.as_str())
                    .unwrap_or("assistant");
                (name, Color::Green)
            }
            _ => ("system", Color::DarkGray),
        };

        lines.push(Line::from(vec![
            Span::styled(
                role_label.to_string(),
                Style::default()
                    .fg(role_color)
                    .add_modifier(Modifier::BOLD),
            ),
            if let Some(ref model) = msg.model {
                Span::styled(
                    format!(" ({})", model),
                    Style::default().fg(Color::DarkGray),
                )
            } else {
                Span::raw("")
            },
        ]));

        // Message content — markdown parsed
        let rendered = markdown::render(&msg.text, inner_width);
        lines.extend(rendered);

        // Blank line between messages
        lines.push(Line::raw(""));
    }

    // Streaming response (in progress)
    if !app.streaming_text.is_empty() || !app.streaming_thinking.is_empty() {
        // Thinking block (if visible)
        if app.thinking_expanded && !app.streaming_thinking.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                "─── thinking ───",
                Style::default().fg(Color::DarkGray),
            )]));
            for line in app.streaming_thinking.lines() {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            lines.push(Line::from(vec![Span::styled(
                "────────────────",
                Style::default().fg(Color::DarkGray),
            )]));
        }

        // Streaming text with cursor
        if !app.streaming_text.is_empty() {
            let name = app
                .focused_agent
                .as_ref()
                .and_then(|id| app.agents.iter().find(|a| a.id == *id))
                .map(|a| a.name.as_str())
                .unwrap_or("assistant");

            lines.push(Line::from(vec![Span::styled(
                name.to_string(),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )]));

            let rendered = markdown::render(&app.streaming_text, inner_width);
            lines.extend(rendered);

            // Blinking cursor
            let show_cursor = (app.tick_count / 8) % 2 == 0;
            if show_cursor {
                lines.push(Line::from(Span::styled(
                    "▌",
                    Style::default().fg(Color::Green),
                )));
            }
        }
    }

    // Calculate scroll
    let visible_height = area.height.saturating_sub(2) as usize;
    let total_lines = lines.len();
    let scroll = if app.auto_scroll {
        total_lines.saturating_sub(visible_height)
    } else {
        total_lines
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
