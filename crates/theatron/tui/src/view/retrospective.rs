// Retrospective view: completed project phases with outcomes and key metrics.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;
use crate::theme::Theme;

const HEADER_HEIGHT: u16 = 3;
const STATUS_BAR_HEIGHT: u16 = 1;
const CONTENT_MIN_HEIGHT: u16 = 5;

#[expect(
    clippy::indexing_slicing,
    reason = "Layout.split() returns exactly as many Rects as constraints; indices match the three defined constraints"
)]
pub(crate) fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(HEADER_HEIGHT),
            Constraint::Min(CONTENT_MIN_HEIGHT),
            Constraint::Length(STATUS_BAR_HEIGHT),
        ])
        .split(area);

    render_header(frame, layout[0], theme);
    render_completed_phases(app, frame, layout[1], theme);
    render_status_bar(frame, layout[2], theme);
}

fn render_header(frame: &mut Frame, area: Rect, theme: &Theme) {
    let lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "Retrospective",
                theme.style_accent().add_modifier(Modifier::BOLD),
            ),
            Span::styled("  |  ", theme.style_dim()),
            Span::styled("Completed phases and session outcomes", theme.style_muted()),
        ]),
        Line::raw(""),
    ];
    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_completed_phases(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();

    // Session summary
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Session Summary",
            theme.style_fg().add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::raw(""));

    let total_messages = app.dashboard.messages.len();
    let user_messages = app
        .dashboard
        .messages
        .iter()
        .filter(|m| m.role == "user")
        .count();
    let assistant_messages = app
        .dashboard
        .messages
        .iter()
        .filter(|m| m.role == "assistant")
        .count();
    let tool_call_count: usize = app
        .dashboard
        .messages
        .iter()
        .map(|m| m.tool_calls.len())
        .sum();
    let error_count: usize = app
        .dashboard
        .messages
        .iter()
        .flat_map(|m| m.tool_calls.iter())
        .filter(|tc| tc.is_error)
        .count();

    lines.push(Line::from(vec![
        Span::raw("    "),
        Span::styled("Messages: ", theme.style_dim()),
        Span::styled(total_messages.to_string(), theme.style_accent()),
        Span::styled(
            format!("  (user: {user_messages}, assistant: {assistant_messages})"),
            theme.style_muted(),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::raw("    "),
        Span::styled("Tool calls: ", theme.style_dim()),
        Span::styled(tool_call_count.to_string(), theme.style_fg()),
        if error_count > 0 {
            Span::styled(
                format!("  ({error_count} errors)"),
                Style::default().fg(theme.status.error),
            )
        } else {
            Span::styled("  (no errors)", theme.style_success())
        },
    ]));

    if app.dashboard.session_cost_cents > 0 || app.dashboard.daily_cost_cents > 0 {
        let session_cost = format!(
            "${:.2}",
            f64::from(app.dashboard.session_cost_cents) / 100.0
        );
        let daily_cost = format!("${:.2}", f64::from(app.dashboard.daily_cost_cents) / 100.0);
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled("Session cost: ", theme.style_dim()),
            Span::styled(session_cost, theme.style_fg()),
            Span::styled("  |  Daily total: ", theme.style_dim()),
            Span::styled(daily_cost, theme.style_fg()),
        ]));
    }

    lines.push(Line::raw(""));

    // Completed agents/sessions
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Agent Activity",
            theme.style_fg().add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::raw(""));

    if app.dashboard.agents.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled("No agents connected.", theme.style_dim()),
        ]));
    } else {
        for agent in &app.dashboard.agents {
            let session_count = agent.sessions.len();
            let archived_count = agent
                .sessions
                .iter()
                .filter(|s| s.status.as_deref() == Some("archived"))
                .count();
            let active_count = session_count.saturating_sub(archived_count);

            let (status_icon, status_style) = match agent.status {
                crate::state::AgentStatus::Idle => {
                    ("\u{25cb}", Style::default().fg(theme.status.idle))
                }
                _ => ("\u{25cf}", Style::default().fg(theme.status.streaming)),
            };

            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(status_icon, status_style),
                Span::raw(" "),
                Span::styled(
                    agent.name.clone(),
                    theme.style_fg().add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::raw("      "),
                Span::styled(
                    format!("{active_count} active, {archived_count} archived"),
                    theme.style_dim(),
                ),
            ]));

            // WHY: Show completed (archived) sessions as retrospective entries.
            for session in agent
                .sessions
                .iter()
                .filter(|s| s.status.as_deref() == Some("archived"))
            {
                let time_str = session
                    .updated_at
                    .as_ref()
                    .and_then(|ts| ts.split('T').nth(1))
                    .and_then(|t| t.split('.').next())
                    .unwrap_or("--:--:--");

                lines.push(Line::from(vec![
                    Span::raw("      "),
                    Span::styled("\u{2713} ", theme.style_success()),
                    Span::styled(session.key.clone(), theme.style_muted()),
                    Span::styled(format!("  completed {time_str}"), theme.style_dim()),
                ]));
            }

            lines.push(Line::raw(""));
        }
    }

    // Decision history
    if !app.dashboard.submitted_decisions.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "Decision History",
                theme.style_fg().add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::raw(""));

        for decision in &app.dashboard.submitted_decisions {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("\u{25b8} ", theme.style_accent()),
                Span::styled(decision.question.clone(), theme.style_fg()),
            ]));
            lines.push(Line::from(vec![
                Span::raw("      "),
                Span::styled("Answer: ", theme.style_dim()),
                Span::styled(decision.chosen_label.clone(), theme.style_accent()),
            ]));
            lines.push(Line::raw(""));
        }
    }

    let block = Block::default().borders(Borders::NONE);
    let scroll = if app.viewport.render.auto_scroll {
        0
    } else {
        u16::try_from(app.viewport.render.scroll_offset).unwrap_or(u16::MAX)
    };
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(paragraph, area);
}

fn render_status_bar(frame: &mut Frame, area: Rect, theme: &Theme) {
    let line = Line::from(vec![
        Span::raw("  "),
        Span::styled("Esc", theme.style_accent()),
        Span::styled(" back", theme.style_dim()),
    ]);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}
