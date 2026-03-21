use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::{AgentStatus, App, ContextActionsOverlay, Overlay};
use crate::diff;
use crate::keybindings;
use crate::state::{SearchResultKind, SessionSearchOverlay};
use crate::theme::Theme;

/// Width percentage for the default (help/agent/session) popup.
const POPUP_WIDTH_PCT: u16 = 60;
/// Height percentage for the default popup.
const POPUP_HEIGHT_PCT: u16 = 70;
/// Width percentage for compact overlays (context actions).
const COMPACT_POPUP_WIDTH_PCT: u16 = 40;
/// Height percentage for compact overlays.
const COMPACT_POPUP_HEIGHT_PCT: u16 = 50;
/// Width percentage for the diff view overlay.
const DIFF_POPUP_WIDTH_PCT: u16 = 80;
/// Height percentage for the diff view overlay.
const DIFF_POPUP_HEIGHT_PCT: u16 = 85;
/// Column width reserved for the key label in the help overlay keybinding list.
const HELP_KEY_COLUMN_WIDTH: usize = 13;
/// Maximum number of tool-input lines shown in the tool approval overlay before truncating.
const TOOL_APPROVAL_INPUT_LINES: usize = 10;

pub(crate) fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let overlay = match &app.layout.overlay {
        Some(o) => o,
        None => return,
    };

    let popup_area = centered_rect(POPUP_WIDTH_PCT, POPUP_HEIGHT_PCT, area);

    frame.render_widget(Clear, popup_area);

    match overlay {
        Overlay::Help => render_help(app, frame, popup_area, theme),
        Overlay::AgentPicker { cursor } => {
            render_agent_picker(app, frame, popup_area, *cursor, theme)
        }
        Overlay::SessionPicker(picker) => {
            render_session_picker(app, frame, popup_area, picker, theme)
        }
        Overlay::ToolApproval(approval) => render_tool_approval(frame, popup_area, approval, theme),
        Overlay::PlanApproval(plan) => render_plan_approval(frame, popup_area, plan, theme),
        Overlay::ContextActions(ctx) => {
            let compact_area =
                centered_rect(COMPACT_POPUP_WIDTH_PCT, COMPACT_POPUP_HEIGHT_PCT, area);
            frame.render_widget(Clear, compact_area);
            render_context_actions(frame, compact_area, ctx, theme);
        }
        Overlay::SystemStatus => render_system_status(app, frame, popup_area, theme),
        Overlay::Settings(settings) => super::settings::render(settings, frame, area, theme),
        Overlay::SessionSearch(search) => render_session_search(frame, popup_area, search, theme),
        Overlay::DiffView(diff_state) => {
            let diff_area = centered_rect(DIFF_POPUP_WIDTH_PCT, DIFF_POPUP_HEIGHT_PCT, area);
            frame.render_widget(Clear, diff_area);
            render_diff_view(diff_state, frame, diff_area, theme);
        }
    }
}

/// Standard overlay block with rounded borders.
fn overlay_block<'a>(title: &str, theme: &Theme) -> Block<'a> {
    Block::default()
        .title(format!(" {} ", title.trim()))
        .title_style(theme.style_accent_bold())
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(theme.style_border())
        .style(Style::default().bg(theme.colors.surface))
}

/// Highlighted overlay block (for warnings/approvals).
fn overlay_block_accent(
    title: &str,
    accent_color: ratatui::style::Color,
    theme: &Theme,
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
        .style(Style::default().bg(theme.colors.surface))
}

fn render_help(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let key_style = Style::default()
        .fg(theme.colors.accent)
        .add_modifier(Modifier::BOLD);
    let desc_style = theme.style_fg();
    let section_style = Style::default()
        .fg(theme.text.fg)
        .add_modifier(Modifier::BOLD);

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
                Span::styled(format!("  {:<HELP_KEY_COLUMN_WIDTH$}", kb.keys), key_style),
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

fn render_agent_picker(app: &App, frame: &mut Frame, area: Rect, cursor: usize, theme: &Theme) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    for (i, agent) in app.dashboard.agents.iter().enumerate() {
        let selected = i == cursor;
        let marker = if selected { "▸" } else { " " };
        let emoji = agent.emoji.as_deref().unwrap_or("");

        let style = if selected {
            Style::default()
                .fg(theme.colors.accent)
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
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" select  ", theme.style_muted()),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.text.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", theme.style_muted()),
    ]));

    let block = overlay_block("Switch Agent", theme);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_session_picker(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    picker: &crate::state::SessionPickerOverlay,
    theme: &Theme,
) {
    let agent_id = app.dashboard.focused_agent.as_ref();
    let agent = agent_id.and_then(|id| app.dashboard.agents.iter().find(|a| &a.id == id));

    let sessions: Vec<_> = match agent {
        Some(a) => {
            if picker.show_archived {
                a.sessions.iter().collect()
            } else {
                a.sessions.iter().filter(|s| s.is_interactive()).collect()
            }
        }
        None => Vec::new(),
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    if sessions.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No sessions found",
            theme.style_muted(),
        )));
    }

    for (i, session) in sessions.iter().enumerate() {
        let selected = i == picker.cursor;
        let marker = if selected { "▸" } else { " " };
        let is_current = app.dashboard.focused_session_id.as_ref() == Some(&session.id);

        let style = if selected {
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD)
        } else if is_current {
            Style::default().fg(theme.colors.accent)
        } else {
            theme.style_fg()
        };

        let label = session.label();
        let archived_tag = if session.is_archived() {
            " [archived]"
        } else {
            ""
        };

        lines.push(Line::from(vec![
            Span::raw(format!("  {} ", marker)),
            Span::styled(label, style),
            Span::styled(archived_tag, theme.style_dim()),
            Span::styled(
                format!("  ({} msgs)", session.message_count),
                theme.style_dim(),
            ),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Enter",
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" switch  ", theme.style_muted()),
        Span::styled(
            "n",
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("ew  ", theme.style_muted()),
        Span::styled(
            "d",
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("elete  ", theme.style_muted()),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.text.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", theme.style_muted()),
    ]));

    let agent_name = agent.map(|a| a.name.as_str()).unwrap_or("?");
    let title = if picker.show_archived {
        format!("Sessions — {} (all)", agent_name)
    } else {
        format!("Sessions — {}", agent_name)
    };
    let block = overlay_block(&title, theme);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_tool_approval(
    frame: &mut Frame,
    area: Rect,
    approval: &crate::app::ToolApprovalOverlay,
    theme: &Theme,
) {
    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Tool: ", theme.style_muted()),
            Span::styled(
                &approval.tool_name,
                Style::default()
                    .fg(theme.status.warning)
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
            Style::default()
                .fg(theme.text.fg)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    let input_str = serde_json::to_string_pretty(&approval.input).unwrap_or_default();
    for line in input_str.lines().take(TOOL_APPROVAL_INPUT_LINES) {
        lines.push(Line::from(Span::styled(
            format!("  {}", line),
            theme.style_dim(),
        )));
    }
    if input_str.lines().count() > TOOL_APPROVAL_INPUT_LINES {
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
                .fg(theme.text.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", theme.style_muted()),
    ]));

    let block = overlay_block_accent("⚠ Tool Approval Required", theme.status.warning, theme);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_plan_approval(
    frame: &mut Frame,
    area: Rect,
    plan: &crate::app::PlanApprovalOverlay,
    theme: &Theme,
) {
    let cost = format!("${:.2}", f64::from(plan.total_cost_cents) / 100.0);
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
                .fg(theme.colors.accent)
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
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" toggle  ", theme.style_muted()),
        Span::styled("[C]", theme.style_error_bold()),
        Span::styled("ancel", theme.style_muted()),
    ]));

    let block = overlay_block_accent(&title, theme.colors.accent, theme);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_system_status(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut lines = vec![Line::raw("")];
    let section_style = Style::default()
        .fg(theme.text.fg)
        .add_modifier(Modifier::BOLD);

    lines.push(Line::from(Span::styled("  Connection", section_style)));
    lines.push(Line::raw(""));
    let sse_status = if app.connection.sse_connected {
        Span::styled(
            "  SSE: connected ●",
            Style::default().fg(theme.status.success),
        )
    } else {
        Span::styled(
            "  SSE: disconnected ○",
            Style::default().fg(theme.status.error),
        )
    };
    lines.push(Line::from(sse_status));
    lines.push(Line::from(Span::styled(
        format!("  Gateway: {}", app.config.url),
        theme.style_muted(),
    )));
    lines.push(Line::from(Span::styled(
        format!(
            "  Terminal: {}×{}",
            app.viewport.terminal_width, app.viewport.terminal_height
        ),
        theme.style_muted(),
    )));
    lines.push(Line::raw(""));

    lines.push(Line::from(Span::styled("  Agents", section_style)));
    lines.push(Line::raw(""));

    for agent in &app.dashboard.agents {
        let status_str = match agent.status {
            AgentStatus::Idle => Span::styled("idle", theme.style_dim()),
            AgentStatus::Working => {
                Span::styled("working", Style::default().fg(theme.status.spinner))
            }
            AgentStatus::Streaming => {
                Span::styled("streaming", Style::default().fg(theme.status.streaming))
            }
            AgentStatus::Compacting => {
                let stage = agent.compaction_stage.as_deref().unwrap_or("...");
                Span::styled(
                    format!("compacting ({})", stage),
                    Style::default().fg(theme.status.compacting),
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

    let cost = f64::from(app.dashboard.daily_cost_cents) / 100.0;
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
                .fg(theme.text.fg_dim)
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

fn render_context_actions(
    frame: &mut Frame,
    area: Rect,
    ctx: &ContextActionsOverlay,
    theme: &Theme,
) {
    let mut lines = vec![Line::raw("")];

    for (i, action) in ctx.actions.iter().enumerate() {
        let selected = i == ctx.cursor;
        let marker = if selected { "▸" } else { " " };

        let style = if selected {
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        lines.push(Line::from(vec![
            Span::raw(format!("  {} ", marker)),
            Span::styled(action.label, style),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Enter",
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" select  ", theme.style_muted()),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.text.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", theme.style_muted()),
    ]));

    let block = overlay_block("Actions", theme);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_diff_view(
    diff_state: &diff::DiffViewState,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
) {
    // NOTE: render immutably first to get total_lines for scroll clamping before display
    let inner_area = Rect::new(
        area.x + 1,
        area.y + 1,
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    );
    let all_lines = diff::render_diff_view_immutable(diff_state, inner_area, theme);

    let total = all_lines.len();
    let visible_height = usize::from(inner_area.height);

    let scroll = diff_state
        .scroll_offset
        .min(total.saturating_sub(visible_height));

    let block = overlay_block(&format!("Diff [{}]", diff_state.mode.label()), theme);
    let paragraph = Paragraph::new(all_lines)
        .block(block)
        .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0));
    frame.render_widget(paragraph, area);
}

fn render_session_search(
    frame: &mut Frame,
    area: Rect,
    search: &SessionSearchOverlay,
    theme: &Theme,
) {
    let key_style = Style::default()
        .fg(theme.colors.accent)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  / ", key_style),
        Span::raw(&search.query),
        Span::styled("_", theme.style_dim()),
    ]));
    lines.push(Line::raw(""));

    if search.results.is_empty() && !search.query.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No results",
            theme.style_muted(),
        )));
    }

    let visible_height = usize::from(area.height.saturating_sub(6));
    let start = if search.selected >= visible_height {
        search.selected - visible_height + 1
    } else {
        0
    };

    for (i, result) in search
        .results
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_height)
    {
        let selected = i == search.selected;
        let marker = if selected { "▸" } else { " " };

        let style = if selected {
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        let kind_tag = match &result.kind {
            SearchResultKind::SessionName => Span::styled(" [session]", theme.style_dim()),
            SearchResultKind::MessageContent { role } => {
                Span::styled(format!(" [{role}]"), theme.style_dim())
            }
        };

        lines.push(Line::from(vec![
            Span::raw(format!("  {} ", marker)),
            Span::styled(&result.session_label, style),
            kind_tag,
            Span::styled(format!("  {}", result.agent_name), theme.style_muted()),
        ]));

        if !result.snippet.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("      {}", result.snippet),
                theme.style_dim(),
            )));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Enter", key_style),
        Span::styled(" switch  ", theme.style_muted()),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.text.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", theme.style_muted()),
    ]));

    let block = overlay_block("Search Sessions", theme);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

/// Create a centered rect within the given area.
#[expect(
    clippy::indexing_slicing,
    reason = "Layout.split() returns exactly as many Rects as constraints; [1] is valid for both 3-element constraint arrays"
)]
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
