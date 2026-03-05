use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::{AgentStatus, App, Overlay};
use crate::keybindings;
use crate::theme::ThemePalette;

pub fn render(app: &App, frame: &mut Frame, area: Rect, theme: &ThemePalette) {
    let overlay = match &app.overlay {
        Some(o) => o,
        None => return,
    };

    let popup_area = centered_rect(60, 70, area);

    // Clear the area under the overlay
    frame.render_widget(Clear, popup_area);

    match overlay {
        Overlay::Help => render_help(app, frame, popup_area, theme),
        Overlay::AgentPicker { cursor } => {
            render_agent_picker(app, frame, popup_area, *cursor, theme)
        }
        Overlay::ToolApproval(approval) => render_tool_approval(frame, popup_area, approval, theme),
        Overlay::PlanApproval(plan) => render_plan_approval(frame, popup_area, plan, theme),
        Overlay::SystemStatus => render_system_status(app, frame, popup_area, theme),
        Overlay::Settings(settings) => super::settings::render(settings, frame, area, theme),
    }
}

/// Standard overlay block with rounded borders.
fn overlay_block<'a>(title: &str, theme: &ThemePalette) -> Block<'a> {
    Block::default()
        .title(format!(" {} ", title.trim()))
        .title_style(theme.style_accent_bold())
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(theme.style_border())
        .style(Style::default().bg(theme.surface))
}

/// Highlighted overlay block (for warnings/approvals).
fn overlay_block_accent(
    title: &str,
    accent_color: ratatui::style::Color,
    theme: &ThemePalette,
) -> Block<'static> {
    Block::default()
        .title(format!(" {} ", title.trim()))
        .title_style(
            Style::default()
                .fg(accent_color)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(Style::default().fg(accent_color))
        .style(Style::default().bg(theme.surface))
}

fn render_help(app: &App, frame: &mut Frame, area: Rect, theme: &ThemePalette) {
    let key_style = Style::default()
        .fg(theme.accent)
        .add_modifier(Modifier::BOLD);
    let desc_style = theme.style_fg();
    let section_style = Style::default().fg(theme.fg).add_modifier(Modifier::BOLD);

    let contexts = keybindings::current_contexts(app);
    let groups = keybindings::grouped_keybindings(&contexts);

    let mut lines: Vec<Line> = Vec::new();

    for (section_label, bindings) in &groups {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!("  {section_label}"),
            section_style,
        )));
        lines.push(Line::raw(""));
        for kb in bindings {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<13}", kb.keys), key_style),
                Span::styled(kb.description, desc_style),
            ]));
        }
    }

    lines.push(Line::raw(""));

    let label = keybindings::context_label(app);
    let title = format!("Help — {label}");
    let block = overlay_block(&title, theme);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_agent_picker(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    cursor: usize,
    theme: &ThemePalette,
) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    for (i, agent) in app.agents.iter().enumerate() {
        let selected = i == cursor;
        let marker = if selected { "▸" } else { " " };
        let emoji = agent.emoji.as_deref().unwrap_or("");

        let style = if selected {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        lines.push(Line::from(vec![
            Span::raw(format!("  {} ", marker)),
            Span::styled(format!("{} {} ", emoji, agent.name), style),
            Span::styled(format!("({})", agent.id), theme.style_dim()),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Enter",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" select  ", theme.style_muted()),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", theme.style_muted()),
    ]));

    let block = overlay_block("Switch Agent", theme);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_tool_approval(
    frame: &mut Frame,
    area: Rect,
    approval: &crate::app::ToolApprovalOverlay,
    theme: &ThemePalette,
) {
    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Tool: ", theme.style_muted()),
            Span::styled(
                &approval.tool_name,
                Style::default()
                    .fg(theme.warning)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Risk: ", theme.style_muted()),
            Span::styled(&approval.risk, theme.style_error()),
        ]),
        Line::from(vec![
            Span::styled("  Reason: ", theme.style_muted()),
            Span::styled(&approval.reason, theme.style_fg()),
        ]),
        Line::raw(""),
        Line::from(Span::styled(
            "  Input:",
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        )),
    ];

    // Truncated input display
    let input_str = serde_json::to_string_pretty(&approval.input).unwrap_or_default();
    for line in input_str.lines().take(10) {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            theme.style_dim(),
        )));
    }
    if input_str.lines().count() > 10 {
        lines.push(Line::from(Span::styled("  …", theme.style_dim())));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("[A]", theme.style_success_bold()),
        Span::styled("pprove  ", theme.style_muted()),
        Span::styled("[D]", theme.style_error_bold()),
        Span::styled("eny  ", theme.style_muted()),
        Span::styled(
            "[Esc]",
            Style::default()
                .fg(theme.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", theme.style_muted()),
    ]));

    let block = overlay_block_accent("⚠ Tool Approval Required", theme.warning, theme);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_plan_approval(
    frame: &mut Frame,
    area: Rect,
    plan: &crate::app::PlanApprovalOverlay,
    theme: &ThemePalette,
) {
    let cost = format!("${:.2}", plan.total_cost_cents as f64 / 100.0);
    let title = format!("Plan ({} steps, ~{})", plan.steps.len(), cost);

    let mut lines = vec![Line::raw("")];

    for (i, step) in plan.steps.iter().enumerate() {
        let selected = i == plan.cursor;
        let check = if step.checked { "✓" } else { " " };
        let marker = if selected { "▸" } else { " " };

        let check_style = if step.checked {
            theme.style_success()
        } else {
            theme.style_dim()
        };

        let style = if selected {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        lines.push(Line::from(vec![
            Span::raw(format!("  {} ", marker)),
            Span::styled(format!("[{}]", check), check_style),
            Span::styled(format!(" {}. ", step.id), theme.style_dim()),
            Span::styled(&step.label, style),
            Span::styled(format!(" ({})", step.role), theme.style_dim()),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("[A]", theme.style_success_bold()),
        Span::styled("pprove all  ", theme.style_muted()),
        Span::styled(
            "[Space]",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" toggle  ", theme.style_muted()),
        Span::styled("[C]", theme.style_error_bold()),
        Span::styled("ancel", theme.style_muted()),
    ]));

    let block = overlay_block_accent(&title, theme.accent, theme);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_system_status(app: &App, frame: &mut Frame, area: Rect, theme: &ThemePalette) {
    let mut lines = vec![Line::raw("")];
    let section_style = Style::default().fg(theme.fg).add_modifier(Modifier::BOLD);

    // Connection status
    lines.push(Line::from(Span::styled("  Connection", section_style)));
    lines.push(Line::raw(""));
    let sse_status = if app.sse_connected {
        Span::styled("  SSE: connected ●", Style::default().fg(theme.success))
    } else {
        Span::styled("  SSE: disconnected ○", Style::default().fg(theme.error))
    };
    lines.push(Line::from(sse_status));
    lines.push(Line::from(Span::styled(
        format!("  Gateway: {}", app.config.url),
        theme.style_muted(),
    )));
    lines.push(Line::from(Span::styled(
        format!("  Terminal: {}×{}", app.terminal_width, app.terminal_height),
        theme.style_muted(),
    )));
    lines.push(Line::raw(""));

    // Agents
    lines.push(Line::from(Span::styled("  Agents", section_style)));
    lines.push(Line::raw(""));

    for agent in &app.agents {
        let status_str = match agent.status {
            AgentStatus::Idle => Span::styled("idle", theme.style_dim()),
            AgentStatus::Working => Span::styled("working", Style::default().fg(theme.spinner)),
            AgentStatus::Streaming => {
                Span::styled("streaming", Style::default().fg(theme.streaming))
            }
            AgentStatus::Compacting => {
                let stage = agent.compaction_stage.as_deref().unwrap_or("...");
                Span::styled(
                    format!("compacting ({})", stage),
                    Style::default().fg(theme.compacting),
                )
            }
        };

        let emoji = agent.emoji.as_deref().unwrap_or("");
        let session_count = agent.sessions.len();

        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} {} ", emoji, agent.name),
                theme.style_accent_bold(),
            ),
            Span::styled(format!("({}) ", agent.id), theme.style_dim()),
            status_str,
        ]));
        lines.push(Line::from(Span::styled(
            format!("     {} sessions", session_count),
            theme.style_dim(),
        )));
    }

    lines.push(Line::raw(""));

    // Cost
    let cost = app.daily_cost_cents as f64 / 100.0;
    lines.push(Line::from(Span::styled("  Today", section_style)));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        format!("  Cost: ${:.2}", cost),
        theme.style_muted(),
    )));

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" close", theme.style_muted()),
    ]));

    let block = overlay_block("System Status — Ctrl+I", theme);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Create a centered rect within the given area.
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
