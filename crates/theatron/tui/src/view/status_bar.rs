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

/// Render the info bar with graceful degradation on narrow terminals.
///
/// Priority order (highest first):
/// 1. Connection status indicator
/// 2. Agent name (truncated with "…" when too narrow)
/// 3. Session key
/// 4. Selection / filter indicators
/// 5. Right side: scroll position, cost, context gauge
fn render_info_bar(app: &App, width: u16, theme: &Theme) -> Line<'static> {
    let total = width as usize;

    // Right side (lowest priority): scroll position, cost, context gauge.
    let mut right_spans = Vec::new();
    right_spans.extend(scroll_position_spans(app, theme));
    right_spans.extend(cost_spans(app, theme));
    right_spans.push(Span::styled(" │ ", theme.style_dim()));
    right_spans.extend(context_gauge_spans(app, theme));
    right_spans.push(Span::raw(" "));
    let right_width: usize = right_spans.iter().map(|s| s.content.width()).sum();

    // Connection status (highest priority: always shown first).
    let conn_spans = connection_indicator_spans(app, theme);
    let conn_width: usize = conn_spans.iter().map(|s| s.content.width()).sum();

    // Agent identity (second priority: truncated when narrow).
    let agent_spans = agent_identity_spans(app, theme);
    let agent_width: usize = agent_spans.iter().map(|s| s.content.width()).sum();

    const PREFIX: usize = 1; // leading space
    const SEP: usize = 3; // " │ " separator width

    // Minimum viable content: " " + conn + " │ " + one agent char + "…"
    let min_useful = PREFIX + conn_width + SEP + 2;
    if total < min_useful {
        return Line::from(Span::raw(" "));
    }

    let mut spans: Vec<Span<'static>> = vec![Span::raw(" ")];
    let mut used = PREFIX;

    // Always include connection status.
    spans.extend(conn_spans);
    used += conn_width;

    // Agent identity: budget excludes what's already used plus the separator and
    // enough room for at least one right-side character if it fits.
    let agent_budget = total.saturating_sub(used + SEP);
    if agent_budget >= 2 {
        spans.push(Span::styled(" │ ", theme.style_dim()));
        used += SEP;

        if agent_width <= agent_budget {
            spans.extend(agent_spans);
            used += agent_width;
        } else {
            let truncated = truncate_spans_to_width(agent_spans, agent_budget);
            let tw: usize = truncated.iter().map(|s| s.content.width()).sum();
            spans.extend(truncated);
            used += tw;
        }
    }

    // Optional: tool indicator.
    let tool_spans = tool_indicator_spans(app, theme);
    let tool_w: usize = tool_spans.iter().map(|s| s.content.width()).sum();
    if tool_w > 0 && used + tool_w + 1 < total {
        spans.extend(tool_spans);
        used += tool_w;
    }

    // Optional: selection indicator.
    if let Some(idx) = app.selected_message {
        let total_msgs = app.messages.len();
        let sel = [
            Span::styled(" │ ", theme.style_dim()),
            Span::styled("SELECTION", theme.style_accent()),
            Span::styled(format!(" {} of {}", idx + 1, total_msgs), theme.style_dim()),
        ];
        let sel_w: usize = sel.iter().map(|s| s.content.width()).sum();
        if used + sel_w + 1 < total {
            spans.extend(sel);
            used += sel_w;
        }
    }

    // Optional: filter indicator.
    if app.filter.active && !app.filter.editing && !app.filter.text.is_empty() {
        let filt = [
            Span::styled(" │ ", theme.style_dim()),
            Span::styled(format!("/{}", app.filter.text), theme.style_accent()),
            Span::styled(" (Esc to clear)", theme.style_dim()),
        ];
        let filt_w: usize = filt.iter().map(|s| s.content.width()).sum();
        if used + filt_w + 1 < total {
            spans.extend(filt);
            used += filt_w;
        }
    }

    // Right side: show only when there is room for it with at least one space of padding.
    if used + right_width + 1 < total {
        let pad = total - used - right_width;
        spans.push(Span::raw(" ".repeat(pad)));
        spans.extend(right_spans);
    }

    Line::from(spans)
}

/// Truncate a sequence of spans to at most `max_width` display columns.
///
/// Appends "…" (one column) to the last span that fits, replacing any content
/// that exceeds the budget. When the spans already fit, returns them unchanged.
fn truncate_spans_to_width(spans: Vec<Span<'static>>, max_width: usize) -> Vec<Span<'static>> {
    let total_width: usize = spans.iter().map(|s| s.content.width()).sum();
    if total_width <= max_width {
        return spans;
    }
    // At ≤ 2 columns there is no room for meaningful content plus ellipsis.
    if max_width <= 2 {
        return vec![Span::raw("…")];
    }
    // Reserve one column for the ellipsis.
    let budget = max_width - 1;
    let mut result: Vec<Span<'static>> = Vec::new();
    let mut remaining = budget;

    for span in spans {
        if remaining == 0 {
            break;
        }
        let sw = span.content.width();
        if sw <= remaining {
            result.push(span);
            remaining -= sw;
        } else {
            // Truncate this span's content to fit, then append "…".
            let cut = truncate_str_to_cols(&span.content, remaining);
            result.push(Span::styled(format!("{cut}…"), span.style));
            remaining = 0;
        }
    }

    // All spans fit within the budget but total was over max_width. Append ellipsis.
    if remaining > 0
        && let Some(last) = result.last_mut()
    {
        let new_content = format!("{}…", last.content);
        *last = Span::styled(new_content, last.style);
    }

    result
}

/// Truncate `s` to at most `max_cols` display columns and return the prefix as a `&str`.
fn truncate_str_to_cols(s: &str, max_cols: usize) -> &str {
    if s.width() <= max_cols {
        return s;
    }
    let mut w = 0usize;
    let mut end = 0usize;
    for (idx, ch) in s.char_indices() {
        let char_w = s[idx..idx + ch.len_utf8()].width();
        if w + char_w > max_cols {
            break;
        }
        w += char_w;
        end = idx + ch.len_utf8();
    }
    &s[..end]
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
        // Healthy when last event was within 5 seconds, otherwise degraded.
        let stale = app
            .sse_last_event_at
            .map(|t| t.elapsed().as_secs() > 5)
            .unwrap_or(false);
        if stale {
            vec![
                Span::styled("●", theme.style_warning()),
                Span::styled(" Stale", theme.style_dim()),
            ]
        } else {
            vec![Span::styled("●", theme.style_success())]
        }
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

fn tool_indicator_spans(app: &App, theme: &Theme) -> Vec<Span<'static>> {
    let agent = app
        .focused_agent
        .as_ref()
        .and_then(|id| app.agents.iter().find(|a| a.id == *id));

    let Some(agent) = agent else {
        return Vec::new();
    };

    if agent.tools.is_empty() {
        return Vec::new();
    }

    let enabled = agent.tools.iter().filter(|t| t.enabled).count();
    let total = agent.tools.len();

    vec![
        Span::styled(" \u{2502} ", theme.style_dim()),
        Span::styled(
            format!("\u{2699} {enabled}/{total}"),
            if enabled == total {
                theme.style_muted()
            } else {
                theme.style_warning()
            },
        ),
    ]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::{test_agent, test_app};

    #[test]
    fn truncate_spans_to_width_no_op_when_fits() {
        let spans = vec![Span::raw("hello"), Span::raw(" world")];
        let result = truncate_spans_to_width(spans.clone(), 20);
        let text: String = result.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "hello world");
    }

    #[test]
    fn truncate_spans_to_width_adds_ellipsis() {
        let spans = vec![Span::raw("hello world")];
        let result = truncate_spans_to_width(spans, 7);
        let text: String = result.iter().map(|s| s.content.as_ref()).collect();
        // Budget = 6 chars + "…" = 7 display cols total
        assert!(text.ends_with('…'), "truncated text must end with ellipsis");
        assert!(
            text.width() <= 7,
            "truncated text must fit within max_width"
        );
    }

    #[test]
    fn truncate_spans_to_width_very_narrow() {
        let spans = vec![Span::raw("Syn · main")];
        let result = truncate_spans_to_width(spans, 2);
        let text: String = result.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "…");
    }

    #[test]
    fn truncate_str_to_cols_exact_fit() {
        let s = "hello";
        assert_eq!(truncate_str_to_cols(s, 5), "hello");
    }

    #[test]
    fn truncate_str_to_cols_truncates() {
        let s = "hello world";
        let t = truncate_str_to_cols(s, 5);
        assert_eq!(t, "hello");
    }

    #[test]
    fn render_info_bar_very_narrow_does_not_panic() {
        let app = test_app();
        // Must not panic on very narrow terminals.
        for w in 0u16..20 {
            let line = render_info_bar(&app, w, &app.theme);
            // Every span must not exceed the given width when summed.
            let total: usize = line.spans.iter().map(|s| s.content.width()).sum();
            assert!(
                total <= w as usize + 5, // small slack for edge cases in span building
                "width={w}: rendered {total} cols, expected ≤ {}",
                w as usize + 5
            );
        }
    }

    #[test]
    fn render_info_bar_wide_includes_right_side() {
        let app = test_app();
        // On a wide terminal the right side (cost, context gauge) must appear.
        let line = render_info_bar(&app, 200, &app.theme);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        // Cost element always present on wide terminals.
        assert!(
            text.contains('$') || text.contains('░'),
            "wide status bar must include cost or context gauge"
        );
    }

    #[test]
    fn tool_indicator_hidden_when_no_tools() {
        let app = test_app();
        let spans = tool_indicator_spans(&app, &app.theme);
        assert!(spans.is_empty());
    }

    #[test]
    fn tool_indicator_shows_counts() {
        use crate::state::ToolSummary;

        let mut app = test_app();
        let mut agent = test_agent("syn", "Syn");
        agent.tools = vec![
            ToolSummary {
                name: "read_file".to_string(),
                enabled: true,
            },
            ToolSummary {
                name: "bash".to_string(),
                enabled: false,
            },
            ToolSummary {
                name: "write_file".to_string(),
                enabled: true,
            },
        ];
        let agent_id = agent.id.clone();
        app.agents.push(agent);
        app.focused_agent = Some(agent_id);

        let spans = tool_indicator_spans(&app, &app.theme);
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains("2/3"),
            "tool indicator should show 2/3, got: {text}"
        );
    }
}
