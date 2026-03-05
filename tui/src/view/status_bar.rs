use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, SelectionContext};
use crate::theme::ThemePalette;

pub fn render(app: &App, frame: &mut Frame, area: Rect, theme: &ThemePalette) {
    let line1 = render_keybindings(app, area.width, theme);
    let line2 = render_info_bar(app, area.width, theme);

    let bar = Paragraph::new(vec![line1, line2]).style(theme.style_surface());
    frame.render_widget(bar, area);
}

fn render_keybindings(app: &App, width: u16, theme: &ThemePalette) -> Line<'static> {
    let hints = match app.selection {
        SelectionContext::None => ": command │ / filter │ ? help",
        SelectionContext::UserMessage { .. } => "e edit │ d delete │ c copy",
        SelectionContext::AgentResponse { .. } => "c copy │ y yank │ o open",
        SelectionContext::ToolCall { .. } => "a approve │ r reject │ i inspect",
        SelectionContext::SessionListItem { .. } => "Enter open │ d delete │ n new",
    };

    let hints_width = hints.width();
    let pad = (width as usize).saturating_sub(hints_width + 1);
    Line::from(vec![
        Span::raw(" ".repeat(pad)),
        Span::styled(hints.to_string(), theme.style_dim()),
    ])
}

fn render_info_bar(app: &App, width: u16, theme: &ThemePalette) -> Line<'static> {
    let mut left_spans = Vec::new();
    let mut right_spans = Vec::new();

    left_spans.extend(agent_identity_spans(app, theme));

    left_spans.push(Span::styled(" │ ", theme.style_dim()));
    left_spans.push(connection_indicator_span(app, theme));

    if let Some(ref filter) = app.active_filter {
        left_spans.push(Span::styled(" │ ", theme.style_dim()));
        left_spans.push(Span::styled(format!("/{filter}"), theme.style_accent()));
    }

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

fn agent_identity_spans(app: &App, theme: &ThemePalette) -> Vec<Span<'static>> {
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

fn connection_indicator_span(app: &App, theme: &ThemePalette) -> Span<'static> {
    if app.sse_connected {
        Span::styled("●", theme.style_success())
    } else {
        Span::styled("○", theme.style_error())
    }
}

fn cost_spans(app: &App, theme: &ThemePalette) -> Vec<Span<'static>> {
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

fn context_gauge_spans(app: &App, theme: &ThemePalette) -> Vec<Span<'static>> {
    const GAUGE_WIDTH: usize = 6;

    match app.context_usage_pct {
        Some(pct) => {
            let filled = (pct as usize * GAUGE_WIDTH) / 100;
            let empty = GAUGE_WIDTH.saturating_sub(filled);

            let color = if pct <= 60 {
                theme.success
            } else if pct <= 80 {
                theme.warning
            } else {
                theme.error
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
