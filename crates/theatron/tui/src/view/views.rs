//! View-specific rendering for each View variant in the navigation stack.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;
use crate::hyperlink::OscLink;
use crate::state::view_stack::View;
use crate::theme::Theme;

/// Render the sessions list for a specific agent.
pub(crate) fn render_sessions(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    let agent = app
        .focused_agent
        .as_ref()
        .and_then(|id| app.agents.iter().find(|a| a.id == *id));

    if let Some(agent) = agent {
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                format!("Sessions for {}", agent.name),
                theme.style_accent().add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::raw(""));

        if agent.sessions.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("No sessions found.", theme.style_dim()),
            ]));
        } else {
            for (idx, session) in agent.sessions.iter().enumerate() {
                let is_focused = app
                    .focused_session_id
                    .as_ref()
                    .is_some_and(|id| *id == session.id);

                let marker = if is_focused { "▸" } else { " " };
                let marker_style = if is_focused {
                    Style::default().fg(theme.borders.selected)
                } else {
                    Style::default()
                };

                let status_icon = match session.status.as_deref() {
                    Some("archived") => "◌",
                    Some("active") => "●",
                    _ => "○",
                };

                let name_style = if is_focused {
                    theme.style_fg().add_modifier(Modifier::BOLD)
                } else {
                    theme.style_fg()
                };

                let mut spans = vec![
                    Span::styled(marker, marker_style),
                    Span::raw(" "),
                    Span::styled(status_icon, theme.style_dim()),
                    Span::raw(" "),
                    Span::styled(format!("{}. {}", idx + 1, session.key), name_style),
                ];

                if let Some(ref updated) = session.updated_at {
                    let time_str = updated
                        .split('T')
                        .nth(1)
                        .and_then(|t| t.split('.').next())
                        .unwrap_or(updated);
                    spans.push(Span::styled(format!("  {time_str}"), theme.style_dim()));
                }

                lines.push(Line::from(spans));
            }
        }
    } else {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("No agent selected.", theme.style_dim()),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Enter", theme.style_accent()),
        Span::styled(" select  ", theme.style_dim()),
        Span::styled("Esc", theme.style_accent()),
        Span::styled(" back", theme.style_dim()),
    ]));

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Render full message detail view.
pub(crate) fn render_message_detail(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    message_index: usize,
) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    if let Some(msg) = app.messages.get(message_index) {
        // Header
        let role_label = match msg.role.as_str() {
            "user" => "You",
            "assistant" => "Assistant",
            _ => "System",
        };
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                format!("Message #{message_index} — {role_label}"),
                theme.style_accent().add_modifier(Modifier::BOLD),
            ),
        ]));

        // Metadata
        if let Some(ref model) = msg.model {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Model: ", theme.style_dim()),
                Span::styled(model.clone(), theme.style_fg()),
            ]));
        }

        if let Some(ref ts) = msg.timestamp {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Time: ", theme.style_dim()),
                Span::styled(ts.clone(), theme.style_fg()),
            ]));
        }

        lines.push(Line::raw(""));

        // Tool calls
        if !msg.tool_calls.is_empty() {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Tool Calls:", theme.style_fg().add_modifier(Modifier::BOLD)),
            ]));
            for tc in &msg.tool_calls {
                let status = if tc.is_error { "FAILED" } else { "OK" };
                let dur = tc
                    .duration_ms
                    .map(|ms| format!(" ({ms}ms)"))
                    .unwrap_or_default();
                let color = if tc.is_error {
                    theme.status.error
                } else {
                    theme.text.fg_dim
                };
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(
                        format!("{} [{status}]{dur}", tc.name),
                        Style::default().fg(color),
                    ),
                ]));
            }
            lines.push(Line::raw(""));
        }

        // Full content
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Content:", theme.style_fg().add_modifier(Modifier::BOLD)),
        ]));

        let (rendered, _) = crate::markdown::render(
            &msg.text,
            area.width.saturating_sub(4) as usize,
            theme,
            &app.highlighter,
        );
        for line in rendered {
            let mut padded = vec![Span::raw("  ")];
            padded.extend(line.spans);
            lines.push(Line::from(padded));
        }
    } else {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("Message #{message_index} not found."),
                theme.style_error(),
            ),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Esc", theme.style_accent()),
        Span::styled(" back  ", theme.style_dim()),
        Span::styled("c", theme.style_accent()),
        Span::styled(" copy  ", theme.style_dim()),
        Span::styled("y", theme.style_accent()),
        Span::styled(" yank code", theme.style_dim()),
    ]));

    let block = Block::default().borders(Borders::NONE);
    let scroll = if app.auto_scroll {
        0
    } else {
        app.scroll_offset as u16
    };
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Dispatch rendering to the appropriate view based on the current view stack.
pub(crate) fn render_for_view(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
) -> Vec<OscLink> {
    match app.view_stack.current() {
        View::Home => {
            // Home view uses the existing chat area rendering
            super::render_chat_area(app, frame, area, theme)
        }
        View::Sessions { .. } => {
            render_sessions(app, frame, area, theme);
            Vec::new()
        }
        View::Conversation { .. } => {
            // Conversation view reuses the chat area rendering
            super::render_chat_area(app, frame, area, theme)
        }
        View::MessageDetail { message_index } => {
            render_message_detail(app, frame, area, theme, *message_index);
            Vec::new()
        }
        View::MemoryInspector => {
            super::memory::render_inspector(app, frame, area, theme);
            Vec::new()
        }
        View::FactDetail { .. } => {
            super::memory::render_fact_detail(app, frame, area, theme);
            Vec::new()
        }
    }
}
