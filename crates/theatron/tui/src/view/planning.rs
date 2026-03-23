// Planning dashboard: active phases, progress, and pending checkpoint approvals.

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
    render_phases(app, frame, layout[1], theme);
    render_status_bar(frame, layout[2], theme);
}

fn render_header(frame: &mut Frame, area: Rect, theme: &Theme) {
    let lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "Planning Dashboard",
                theme.style_accent().add_modifier(Modifier::BOLD),
            ),
            Span::styled("  |  ", theme.style_dim()),
            Span::styled(
                "Active phases and checkpoint approvals",
                theme.style_muted(),
            ),
        ]),
        Line::raw(""),
    ];
    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_phases(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();

    // WHY: Planning data comes from the stream plan events. Show whatever plan state
    // is available from the current connection.
    let has_plan =
        !app.connection.streaming_text.is_empty() && app.connection.active_turn_id.is_some();

    if has_plan {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Active Turn", theme.style_fg().add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::raw(""));

        let phase_style = Style::default().fg(theme.status.streaming);
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled("\u{25cf} ", phase_style),
            Span::styled("In Progress", phase_style),
            Span::styled("  \u{2014}  streaming response", theme.style_dim()),
        ]));
        lines.push(Line::raw(""));
    }

    // WHY: Show connected agents as potential planning sources.
    if !app.dashboard.agents.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "Agent Status",
                theme.style_fg().add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::raw(""));

        for agent in &app.dashboard.agents {
            let (icon, icon_style) = match agent.status {
                crate::state::AgentStatus::Idle => {
                    ("\u{25cb}", Style::default().fg(theme.status.idle))
                }
                crate::state::AgentStatus::Working => {
                    ("\u{25cf}", Style::default().fg(theme.status.spinner))
                }
                crate::state::AgentStatus::Streaming => {
                    ("\u{25cf}", Style::default().fg(theme.status.streaming))
                }
                crate::state::AgentStatus::Compacting => {
                    ("\u{25cf}", Style::default().fg(theme.status.compacting))
                }
            };

            let session_count = agent.sessions.len();
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(icon, icon_style),
                Span::raw(" "),
                Span::styled(agent.name.clone(), theme.style_fg()),
                Span::styled(format!("  ({session_count} sessions)"), theme.style_dim()),
            ]));
        }
        lines.push(Line::raw(""));
    }

    // Checkpoint approvals section
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Pending Checkpoints",
            theme.style_fg().add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::raw(""));

    if app.layout.overlay.is_none() {
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled("No pending checkpoint approvals.", theme.style_dim()),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled(
                "\u{25b8} 1 approval pending",
                Style::default().fg(theme.status.warning),
            ),
        ]));
    }

    lines.push(Line::raw(""));

    // Progress overview
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Session Progress",
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
    let assistant_messages = total_messages.saturating_sub(user_messages);

    lines.push(Line::from(vec![
        Span::raw("    "),
        Span::styled("Total messages: ", theme.style_dim()),
        Span::styled(total_messages.to_string(), theme.style_accent()),
    ]));
    lines.push(Line::from(vec![
        Span::raw("    "),
        Span::styled("User: ", theme.style_dim()),
        Span::styled(user_messages.to_string(), theme.style_fg()),
        Span::styled("  |  Assistant: ", theme.style_dim()),
        Span::styled(assistant_messages.to_string(), theme.style_fg()),
    ]));

    if let Some(pct) = app.dashboard.context_usage_pct {
        let bar_width = 20usize;
        let filled = usize::from(pct) * bar_width / 100;
        let empty = bar_width.saturating_sub(filled);
        let bar_color = if pct > 80 {
            theme.status.warning
        } else {
            theme.status.success
        };

        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::raw("    "),
            Span::styled("Context: ", theme.style_dim()),
            Span::styled("\u{2588}".repeat(filled), Style::default().fg(bar_color)),
            Span::styled("\u{2591}".repeat(empty), theme.style_dim()),
            Span::styled(format!(" {pct}%"), theme.style_fg()),
        ]));
    }

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
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
