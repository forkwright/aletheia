mod pickers;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::{AgentStatus, App, BackendHealth, ContextActionsOverlay, Overlay};
use crate::diff;
use crate::keybindings;
use crate::theme::Theme;
use crate::view::centered_rect;
use crate::view::presentation::{format_token_count, push_mutation_status};

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
        Overlay::Help { scroll } => render_help(app, frame, popup_area, *scroll, theme),
        Overlay::AgentPicker { cursor } => {
            pickers::render_agent_picker(app, frame, popup_area, *cursor, theme)
        }
        Overlay::SessionPicker(picker) => {
            pickers::render_session_picker(app, frame, popup_area, picker, theme)
        }
        Overlay::ToolApproval(approval) => render_tool_approval(frame, popup_area, approval, theme),
        Overlay::ContextActions(ctx) => {
            let compact_area =
                centered_rect(COMPACT_POPUP_WIDTH_PCT, COMPACT_POPUP_HEIGHT_PCT, area);
            frame.render_widget(Clear, compact_area);
            render_context_actions(frame, compact_area, ctx, theme);
        }
        Overlay::SystemStatus => render_system_status(app, frame, popup_area, theme),
        Overlay::ContextBudget => render_context_budget(app, frame, popup_area, theme),
        Overlay::Settings(settings) => super::settings::render(settings, frame, area, theme),
        Overlay::SessionSearch(search) => {
            pickers::render_session_search(frame, popup_area, search, theme)
        }
        Overlay::DiffView(diff_state) => {
            let diff_area = centered_rect(DIFF_POPUP_WIDTH_PCT, DIFF_POPUP_HEIGHT_PCT, area);
            frame.render_widget(Clear, diff_area);
            render_diff_view(diff_state, frame, diff_area, theme);
        }
        Overlay::NotificationHistory { scroll } => {
            super::notification::render_history(app, frame, area, *scroll, theme);
        }
    }
}

/// Standard overlay block with rounded borders.
pub(super) fn overlay_block<'a>(title: &str, theme: &Theme) -> Block<'a> {
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

/// Margin around the help overlay in columns.
const HELP_OVERLAY_MARGIN: u16 = 4; // 2 chars each side
/// Minimum gap (in columns) reserved between the key column and the
/// description, no matter how long the longest key label is (#7221: the old
/// fixed `HELP_KEY_COLUMN_WIDTH = 13` was shorter than several real compound
/// labels -- e.g. `"Ctrl+E / Ctrl+G"` at 16 chars -- so Rust's `{:<13}`
/// padding, which pads but never truncates, ran the key straight into the
/// description with zero separator: `"Ctrl+GOpen $EDITOR"`).
const HELP_COLUMN_GAP: usize = 2;
/// Rows reserved for the overlay's own border (top + bottom).
const HELP_BORDER_ROWS: u16 = 2;

#[expect(
    clippy::string_slice,
    reason = "desc_max_width < description.len() checked before slicing"
)]
fn render_help(app: &App, frame: &mut Frame, area: Rect, scroll: usize, theme: &Theme) {
    let key_style = Style::default()
        .fg(theme.colors.accent)
        .add_modifier(Modifier::BOLD);
    let desc_style = theme.style_fg();
    let section_style = Style::default()
        .fg(theme.text.fg)
        .add_modifier(Modifier::BOLD);

    let contexts = keybindings::current_contexts(app);
    let groups = keybindings::grouped_keybindings(&contexts);

    // WHY(#7221): computed from the longest label actually being rendered
    // (not a hand-picked constant that silently falls behind a new,
    // longer registry entry) plus a fixed gap, so the description can never
    // collide with the key column regardless of what `all_keybindings()`
    // grows to contain.
    let key_col_width = groups
        .iter()
        .flat_map(|(_, bindings)| bindings.iter())
        .map(|kb| kb.keys.chars().count())
        .max()
        .unwrap_or(0);
    let key_column_total_width = key_col_width + HELP_COLUMN_GAP;

    let mut lines: Vec<Line> = Vec::new();

    let max_width = usize::from(area.width.saturating_sub(HELP_OVERLAY_MARGIN).max(1));
    let desc_max_width = max_width.saturating_sub(key_column_total_width + 2); // +2 for leading indent

    for (section_label, bindings) in &groups {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!("  {section_label}"),
            section_style,
        )));
        lines.push(Line::raw(""));
        for kb in bindings {
            let key_span =
                Span::styled(format!("  {:<key_column_total_width$}", kb.keys), key_style);
            let desc = if kb.description.len() > desc_max_width && desc_max_width > 3 {
                // kanon:ignore RUST/indexing-slicing — slice end is clamped and guarded by len() > desc_max_width > 3
                // kanon:ignore RUST/string-slice — slice end is clamped and guarded by len() > desc_max_width > 3
                format!("{}...", &kb.description[..desc_max_width.saturating_sub(3)])
            } else {
                kb.description.to_string()
            };
            lines.push(Line::from(vec![key_span, Span::styled(desc, desc_style)]));
        }
    }

    lines.push(Line::raw(""));

    // WHY(#7221): clamp at render time so `scroll` (a raw line offset that
    // Up/Down/PageUp/PageDown only ever increment/decrement, mirroring
    // `render_diff_view`'s pattern) can never scroll past the point where
    // the last line is still visible -- the same trick used there to avoid
    // needing the line count inside the update handler.
    let total_lines = lines.len();
    let visible_height = usize::from(area.height.saturating_sub(HELP_BORDER_ROWS));
    let clamped_scroll = scroll.min(total_lines.saturating_sub(visible_height));

    let label = keybindings::context_label(app);
    let title = format!("Help — {label}");
    let block = overlay_block(&title, theme);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((u16::try_from(clamped_scroll).unwrap_or(u16::MAX), 0));
    frame.render_widget(paragraph, area);
}

fn render_tool_approval(
    frame: &mut Frame,
    area: Rect,
    approval: &crate::app::ToolApprovalOverlay,
    theme: &Theme,
) {
    let risk_lower = approval.risk.to_lowercase();
    let risk_style = if risk_lower.contains("high") || risk_lower.contains("destructive") {
        theme.style_error_bold()
    } else if risk_lower.contains("medium") || risk_lower.contains("irreversible") {
        Style::default()
            .fg(theme.status.warning)
            .add_modifier(Modifier::BOLD)
    } else {
        theme.style_error()
    };
    let risk_icon = if risk_lower.contains("destructive") || risk_lower.contains("high") {
        "⚠ "
    } else {
        "! "
    };

    let humanized_name = approval
        .tool_name
        .replace('_', " ")
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => {
                    let mut s = f.to_uppercase().collect::<String>();
                    s.push_str(c.as_str());
                    s
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let mut lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Tool: ", theme.style_muted()),
            Span::styled(
                humanized_name,
                Style::default()
                    .fg(theme.status.warning)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Risk: ", theme.style_muted()),
            Span::styled(format!("{risk_icon}{}", approval.risk), risk_style),
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

    // kanon:ignore RUST/no-result-unwrap-or-default — serde_json::to_string_pretty on Value should never fail; empty fallback is harmless
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

    push_mutation_status(&mut lines, &approval.status, theme);

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("[A]", theme.style_success_bold()),
        Span::styled("pprove  ", theme.style_muted()),
        Span::styled("[D]", theme.style_error_bold()),
        Span::styled("eny  ", theme.style_muted()),
        Span::styled(
            "[L]",
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" Always allow  ", theme.style_muted()),
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
            AgentStatus::AwaitingApproval => Span::styled(
                "awaiting approval",
                Style::default().fg(theme.status.warning),
            ),
        };

        let emoji = agent.emoji.as_deref().unwrap_or("");
        let session_count = agent.sessions.len();
        let backend_str = match agent.backend_health {
            BackendHealth::Healthy => None,
            BackendHealth::Dormant => Some(("dormant", theme.style_dim())),
            BackendHealth::Degraded => {
                Some(("degraded", Style::default().fg(theme.status.warning)))
            }
            BackendHealth::Unknown => Some(("unknown", theme.style_dim())),
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!("  {} {} ", emoji, agent.name),
                theme.style_accent_bold(),
            ),
            Span::styled(format!("({}) ", agent.id), theme.style_dim()),
            status_str,
        ]));
        let mut detail = vec![Span::styled(
            format!("     {} sessions", session_count),
            theme.style_dim(),
        )];
        if let Some((label, style)) = backend_str {
            detail.push(Span::styled(format!(" · backend: {label}"), style));
        }
        lines.push(Line::from(detail));
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

    let block = overlay_block("System Status — F4", theme);
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

fn render_context_budget(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    const GAUGE_WIDTH: usize = 30;
    const WARN_THRESHOLD: u8 = 70;
    const CRITICAL_THRESHOLD: u8 = 90;

    let block = overlay_block("Context Budget", theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let pct = app.dashboard.context_usage_pct.unwrap_or(0);
    let used = app.dashboard.context_tokens_used;
    let total = app.dashboard.context_tokens_total;

    let color = if pct <= WARN_THRESHOLD {
        theme.status.success
    } else if pct <= CRITICAL_THRESHOLD {
        theme.status.warning
    } else {
        theme.status.error
    };

    let filled = (usize::from(pct) * GAUGE_WIDTH) / 100;
    let empty = GAUGE_WIDTH.saturating_sub(filled);
    let bar = format!("[{}{}]", "=".repeat(filled), ".".repeat(empty));

    let token_line = match (used, total) {
        (Some(u), Some(t)) => format!(
            "{pct}%  {bar}  ({} / {} tokens)",
            format_token_count(u),
            format_token_count(t)
        ),
        _ => format!("{pct}%  {bar}"),
    };

    let warn_badge = if pct > WARN_THRESHOLD {
        if pct > CRITICAL_THRESHOLD {
            "  ⚠ approaching limit"
        } else {
            "  ⚠ high usage"
        }
    } else {
        ""
    };

    let lines = vec![
        Line::from(vec![Span::styled(
            "Context window usage",
            theme.style_accent_bold(),
        )]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(token_line, ratatui::style::Style::default().fg(color)),
            Span::styled(warn_badge, theme.style_warning()),
        ]),
        Line::raw(""),
        Line::from(vec![Span::styled(
            "  Used tokens include input + cache reads.",
            theme.style_muted(),
        )]),
        Line::from(vec![Span::styled(
            "  Total capacity based on current model (200K).",
            theme.style_muted(),
        )]),
        Line::raw(""),
        Line::from(vec![Span::styled("  Esc  close", theme.style_dim())]),
    ];

    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .style(ratatui::style::Style::default());
    frame.render_widget(para, inner);
}
