use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Overlay};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let overlay = match &app.overlay {
        Some(o) => o,
        None => return,
    };

    let popup_area = centered_rect(60, 70, area);

    // Clear the area under the overlay
    frame.render_widget(Clear, popup_area);

    match overlay {
        Overlay::Help => render_help(frame, popup_area),
        Overlay::AgentPicker { cursor } => render_agent_picker(app, frame, popup_area, *cursor),
        Overlay::ToolApproval(approval) => render_tool_approval(frame, popup_area, approval),
        Overlay::PlanApproval(plan) => render_plan_approval(frame, popup_area, plan),
        _ => {
            let block = Block::default()
                .title(" TODO ")
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Black));
            frame.render_widget(block, popup_area);
        }
    }
}

fn render_help(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled(
            "Keybindings",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
        Line::raw("  Ctrl+Q     Quit"),
        Line::raw("  Ctrl+F     Toggle sidebar"),
        Line::raw("  Ctrl+A     Agent picker"),
        Line::raw("  Ctrl+T     Toggle thinking blocks"),
        Line::raw("  Ctrl+U     Clear input"),
        Line::raw("  Ctrl+W     Delete word"),
        Line::raw("  Ctrl+E     Open $EDITOR"),
        Line::raw("  Enter      Send message"),
        Line::raw("  Up/Down    Input history"),
        Line::raw("  Shift+Up   Scroll up"),
        Line::raw("  Shift+Down Scroll down"),
        Line::raw("  PgUp/PgDn  Page scroll"),
        Line::raw("  Esc        Close overlay"),
    ];

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_agent_picker(app: &App, frame: &mut Frame, area: Rect, cursor: usize) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    for (i, agent) in app.agents.iter().enumerate() {
        let selected = i == cursor;
        let marker = if selected { "▸ " } else { "  " };
        let emoji = agent.emoji.as_deref().unwrap_or("");

        let style = if selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        lines.push(Line::from(vec![
            Span::raw(format!("  {}", marker)),
            Span::styled(
                format!("{}{} ({})", emoji, agent.name, agent.id),
                style,
            ),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  Enter to select, Esc to cancel",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .title(" Switch Agent ")
        .borders(Borders::ALL)
        .style(Style::default().bg(Color::Black));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_tool_approval(
    frame: &mut Frame,
    area: Rect,
    approval: &crate::app::ToolApprovalOverlay,
) {
    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw("  Tool: "),
            Span::styled(
                &approval.tool_name,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::raw("  Risk: "),
            Span::styled(
                &approval.risk,
                Style::default().fg(Color::Red),
            ),
        ]),
        Line::from(vec![
            Span::raw("  Reason: "),
            Span::styled(
                &approval.reason,
                Style::default().fg(Color::White),
            ),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "  Input:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
    ];

    // Truncated input display
    let input_str = serde_json::to_string_pretty(&approval.input).unwrap_or_default();
    for line in input_str.lines().take(10) {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  [A]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("pprove    "),
        Span::styled("[D]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw("eny    "),
        Span::styled("[Esc]", Style::default().fg(Color::DarkGray)),
        Span::raw(" cancel"),
    ]));

    let block = Block::default()
        .title(" ⚠ Tool Approval Required ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black));

    let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_plan_approval(
    frame: &mut Frame,
    area: Rect,
    plan: &crate::app::PlanApprovalOverlay,
) {
    let cost = format!("${:.2}", plan.total_cost_cents as f64 / 100.0);
    let title = format!(" Plan ({} steps, ~{}) ", plan.steps.len(), cost);

    let mut lines = vec![Line::raw("")];

    for (i, step) in plan.steps.iter().enumerate() {
        let selected = i == plan.cursor;
        let check = if step.checked { "✓" } else { " " };
        let marker = if selected { "▸" } else { " " };

        let style = if selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        lines.push(Line::from(vec![
            Span::raw(format!("  {} [{}] {}. ", marker, check, step.id)),
            Span::styled(&step.label, style),
            Span::styled(
                format!(" ({})", step.role),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  [A]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("pprove all    "),
        Span::styled("[Space]", Style::default().fg(Color::Cyan)),
        Span::raw(" toggle    "),
        Span::styled("[C]", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
        Span::raw("ancel"),
    ]));

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

/// Create a centered rect within the given area
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
