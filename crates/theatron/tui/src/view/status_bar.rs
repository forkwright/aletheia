use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// Rows consumed by the status bar, tab bar, and input area above and below the chat pane.
/// Used to estimate the visible viewport height for scrollbar position calculation.
const CHAT_AREA_HEIGHT_OFFSET: u16 = 6;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::keybindings;
use crate::theme::Theme;

pub fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let line1 = render_keybindings(app, area.width, theme);
    let line2 = render_info_bar(app, area.width, theme);

    let bar = Paragraph::new(vec![line1, line2]).style(theme.style_surface());
    frame.render_widget(bar, area);
}

fn render_keybindings(app: &App, width: u16, theme: &Theme) -> Line<'static> {
    let hints = keybindings::status_bar_hints(app);
    let hint_str: String = hints
        .iter()
        .map(|(key, desc)| format!("{key} {desc}"))
        .fold(String::new(), |mut acc, s| {
            if !acc.is_empty() {
                acc.push_str(" \u{2502} ");
            }
            acc.push_str(&s);
            acc
        });

    let hints_width = hint_str.width();
    let pad = (width as usize).saturating_sub(hints_width + 1);
    Line::from(vec![
        Span::raw(" ".repeat(pad)),
        Span::styled(hint_str, theme.style_dim()),
    ])
}

fn render_info_bar(app: &App, width: u16, theme: &Theme) -> Line<'static> {
    let mut left_spans = Vec::new();
    let mut right_spans = Vec::new();

    left_spans.extend(agent_identity_spans(app, theme));

    left_spans.push(Span::styled(" │ ", theme.style_dim()));
    left_spans.extend(connection_indicator_spans(app, theme));

    if let Some(idx) = app.selected_message {
        left_spans.push(Span::styled(" │ ", theme.style_dim()));
        left_spans.push(Span::styled("SELECTION", theme.style_accent()));
        let total = app.messages.len();
        left_spans.push(Span::styled(
            format!(" {} of {}", idx + 1, total),
            theme.style_dim(),
        ));
    }

    if app.filter.active && !app.filter.editing && !app.filter.text.is_empty() {
        left_spans.push(Span::styled(" │ ", theme.style_dim()));
        left_spans.push(Span::styled(
            format!("/{}", app.filter.text),
            theme.style_accent(),
        ));
        left_spans.push(Span::styled(" (Esc to clear)", theme.style_dim()));
    }

    right_spans.extend(scroll_position_spans(app, theme));
    right_spans.extend(cost_spans(app, theme));
    right_spans.push(Span::styled(" │ ", theme.style_dim()));
    right_spans.extend(context_gauge_spans(app, theme));
    right_spans.push(Span::raw(" "));

    let left_width: usize = left_spans.iter().map(|s| s.content.width()).sum();
    let right_width: usize = right_spans.iter().map(|s| s.content.width()).sum();
    let total = width as usize;

    let mut spans = vec![Span::raw(" ")];
    spans.extend(left_spans);

    if left_width + right_width + 2 < total {
        let pad = total - left_width - right_width - 2;
        spans.push(Span::raw(" ".repeat(pad)));
        spans.extend(right_spans);
    }

    Line::from(spans)
}

fn agent_identity_spans(app: &App, theme: &Theme) -> Vec<Span<'static>> {
    let agent = app
        .focused_agent
        .as_ref()
        .and_then(|id| app.agents.iter().find(|a| a.id == *id));

    let (emoji, name) = agent
        .map(|a| (a.emoji.clone().unwrap_or_default(), a.name.clone()))
        .unwrap_or_else(|| (String::new(), "no agent".to_string()));

    let session_key = agent
        .and_then(|a| {
            app.focused_session_id.as_ref().and_then(|sid| {
                a.sessions
                    .iter()
                    .find(|s| s.id == *sid)
                    .map(|s| s.key.clone())
            })
        })
        .unwrap_or_else(|| "—".to_string());

    let mut spans = Vec::new();
    if !emoji.is_empty() {
        spans.push(Span::styled(format!("{emoji} "), theme.style_fg()));
    }
    spans.push(Span::styled(name, theme.style_accent()));
    spans.push(Span::styled(" · ", theme.style_dim()));
    spans.push(Span::styled(session_key, theme.style_muted()));
    spans
}

fn connection_indicator_spans(app: &App, theme: &Theme) -> Vec<Span<'static>> {
    if app.sse_connected {
        vec![Span::styled("●", theme.style_success())]
    } else if app.sse_disconnected_at.is_some() {
        vec![
            Span::styled("○", theme.style_error()),
            Span::styled(" Reconnecting…", theme.style_dim()),
        ]
    } else {
        vec![Span::styled("○", theme.style_error())]
    }
}

fn cost_spans(app: &App, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();

    if app.session_cost_cents > 0 {
        spans.push(Span::styled(
            format_cost(app.session_cost_cents),
            theme.style_fg(),
        ));
    } else {
        spans.push(Span::styled("$—", theme.style_dim()));
    }

    spans.push(Span::styled(" · ", theme.style_dim()));

    if app.daily_cost_cents > 0 {
        spans.push(Span::styled(
            format!("{}/day", format_cost(app.daily_cost_cents)),
            theme.style_fg(),
        ));
    } else {
        spans.push(Span::styled("$0.00/day", theme.style_dim()));
    }

    spans
}

fn format_cost(cents: u32) -> String {
    format!("${:.2}", cents as f64 / 100.0)
}

fn scroll_position_spans(app: &App, theme: &Theme) -> Vec<Span<'static>> {
    let viewport = app.terminal_height.saturating_sub(CHAT_AREA_HEIGHT_OFFSET);
    match app
        .virtual_scroll
        .scrollbar_position(app.scroll_offset, app.auto_scroll, viewport)
    {
        Some((offset, _size)) => {
            let pct = (offset * 100.0).round() as u16;
            vec![
                Span::styled(format!("{pct}%"), theme.style_dim()),
                Span::styled(" │ ", theme.style_dim()),
            ]
        }
        None => Vec::new(),
    }
}

fn context_gauge_spans(app: &App, theme: &Theme) -> Vec<Span<'static>> {
    const GAUGE_WIDTH: usize = 6;
    const CONTEXT_WARN_THRESHOLD: u8 = 60;
    const CONTEXT_CRITICAL_THRESHOLD: u8 = 80;

    match app.context_usage_pct {
        Some(pct) => {
            let filled = (pct as usize * GAUGE_WIDTH) / 100;
            let empty = GAUGE_WIDTH.saturating_sub(filled);

            let color = if pct <= CONTEXT_WARN_THRESHOLD {
                theme.status.success
            } else if pct <= CONTEXT_CRITICAL_THRESHOLD {
                theme.status.warning
            } else {
                theme.status.error
            };

            vec![
                Span::styled("█".repeat(filled), Style::default().fg(color)),
                Span::styled("░".repeat(empty), theme.style_dim()),
                Span::styled(format!(" {pct}%"), Style::default().fg(color)),
            ]
        }
        None => vec![
            Span::styled("░".repeat(GAUGE_WIDTH), theme.style_dim()),
            Span::styled(" —%", theme.style_dim()),
        ],
    }
}
