//! Metrics dashboard view: uptime, token usage, cache hit rate, service health, per-agent stats.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;
use crate::state::metrics::{MetricsState, sparkline};
use crate::theme::Theme;

/// Minimum terminal width required to show the full header row without truncation.
const HEADER_MIN_WIDTH: u16 = 60;
/// Height of the top summary section (uptime + tokens + cache).
const SUMMARY_HEIGHT: u16 = 4;
/// Height of the service health row.
const HEALTH_HEIGHT: u16 = 3;
/// Height of the sparkline section (label + chart).
const SPARKLINE_HEIGHT: u16 = 3;
/// Minimum height for the per-agent table.
const TABLE_MIN_HEIGHT: u16 = 3;
/// Height of the status/hint bar at the bottom.
const STATUS_BAR_HEIGHT: u16 = 1;

/// Render the metrics dashboard.
#[expect(
    clippy::indexing_slicing,
    reason = "Layout.split() returns exactly as many Rects as constraints; indices match the five defined constraints"
)]
pub(crate) fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(SUMMARY_HEIGHT),
            Constraint::Length(HEALTH_HEIGHT),
            Constraint::Length(SPARKLINE_HEIGHT),
            Constraint::Min(TABLE_MIN_HEIGHT),
            Constraint::Length(STATUS_BAR_HEIGHT),
        ])
        .split(area);

    render_summary(app, frame, layout[0], theme);
    render_health(app, frame, layout[1], theme);
    render_sparkline(app, frame, layout[2], theme);
    render_agent_table(app, frame, layout[3], theme);
    render_status_bar(frame, layout[4], theme);
}

fn render_summary(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let metrics = &app.layout.metrics;
    let uptime = metrics.uptime_string();
    let input = MetricsState::format_tokens(metrics.total_input_tokens);
    let output = MetricsState::format_tokens(metrics.total_output_tokens);
    let cache_rate = format!("{:.0}%", metrics.cache_hit_rate() * 100.0);
    let cache_abs = MetricsState::format_tokens(metrics.total_cache_read_tokens);

    let cost_str = format!("${:.2}", f64::from(app.dashboard.daily_cost_cents) / 100.0);

    let wide = area.width >= HEADER_MIN_WIDTH;

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    if wide {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Uptime  ", theme.style_dim()),
            Span::styled(uptime, theme.style_fg()),
            Span::raw("    "),
            Span::styled("In  ", theme.style_dim()),
            Span::styled(input, theme.style_accent()),
            Span::raw("  "),
            Span::styled("Out  ", theme.style_dim()),
            Span::styled(output, theme.style_accent()),
            Span::raw("    "),
            Span::styled("Cache  ", theme.style_dim()),
            Span::styled(cache_rate, theme.style_success()),
            Span::styled(format!(" ({cache_abs})"), theme.style_muted()),
            Span::raw("    "),
            Span::styled("Today  ", theme.style_dim()),
            Span::styled(cost_str, theme.style_warning()),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Up: ", theme.style_dim()),
            Span::styled(uptime, theme.style_fg()),
            Span::raw("  "),
            Span::styled("In: ", theme.style_dim()),
            Span::styled(input, theme.style_accent()),
            Span::raw("  "),
            Span::styled("Out: ", theme.style_dim()),
            Span::styled(output, theme.style_accent()),
        ]));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Cache: ", theme.style_dim()),
            Span::styled(cache_rate, theme.style_success()),
            Span::raw("  "),
            Span::styled("Today: ", theme.style_dim()),
            Span::styled(cost_str, theme.style_warning()),
        ]));
    }

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_health(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let metrics = &app.layout.metrics;

    let (api_icon, api_style, api_label) = match metrics.api_healthy {
        Some(true) => ("●", theme.style_success(), "API online"),
        Some(false) => ("●", theme.style_error(), "API offline"),
        None => ("○", theme.style_muted(), "API unknown"),
    };

    // NOTE: SSE connection status serves as the websocket/streaming service indicator.
    let (sse_icon, sse_style, sse_label) = if app.connection.sse_connected {
        ("●", theme.style_success(), "Events live")
    } else {
        ("●", theme.style_error(), "Events disconnected")
    };

    let lines = vec![
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Services  ", theme.style_dim().add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(api_icon, api_style),
            Span::raw(" "),
            Span::styled(api_label, theme.style_fg()),
            Span::raw("    "),
            Span::styled(sse_icon, sse_style),
            Span::raw(" "),
            Span::styled(sse_label, theme.style_fg()),
        ]),
        Line::raw(""),
    ];

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_sparkline(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let metrics = &app.layout.metrics;

    let chart_width = usize::from(area.width.saturating_sub(4).max(1));
    let totals: Vec<u32> = metrics
        .turn_history
        .iter()
        .map(|t| t.input + t.output + t.cache_read)
        .collect();
    let chart = sparkline(&totals, chart_width);

    let count = metrics.turn_history.len();
    let label = if count == 0 {
        "Token usage  (no turns yet)".to_string()
    } else {
        format!("Token usage  (last {count} turns)")
    };

    let lines = vec![
        Line::from(vec![
            Span::raw("  "),
            Span::styled(label, theme.style_dim()),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(chart, theme.style_accent()),
        ]),
        Line::raw(""),
    ];

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

#[expect(
    clippy::indexing_slicing,
    reason = "visible range start..end is guaranteed valid: start = scroll_offset clamped to len, end = min(start+h, len)"
)]
fn render_agent_table(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let metrics = &app.layout.metrics;
    let agents = &app.dashboard.agents;

    let header_style = theme.style_dim().add_modifier(Modifier::BOLD);
    let mut lines = vec![Line::from(vec![
        Span::styled("  ", header_style),
        Span::styled(format!("{:<18}", "Agent"), header_style),
        Span::styled(format!("{:>6}", "Turns"), header_style),
        Span::styled(format!("{:>9}", "Input"), header_style),
        Span::styled(format!("{:>9}", "Output"), header_style),
        Span::styled(format!("{:>8}", "Cache"), header_style),
    ])];

    if agents.is_empty() {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("No agents.", theme.style_dim()),
        ]));
    } else {
        let visible_height = usize::from(area.height.saturating_sub(1));
        let start = metrics.scroll_offset.min(agents.len().saturating_sub(1));
        let end = (start + visible_height).min(agents.len());

        for (i, agent) in agents[start..end].iter().enumerate() {
            let row_idx = start + i;
            let is_selected = row_idx == metrics.selected_agent;
            let marker = if is_selected { "▸ " } else { "  " };
            let marker_style = if is_selected {
                theme.style_accent().add_modifier(Modifier::BOLD)
            } else {
                theme.style_dim()
            };

            let agent_metrics = metrics.agent_stats.get(&agent.id);
            let turns = agent_metrics.map_or(0, |m| m.turns);
            let input_tok = agent_metrics.map_or(0, |m| m.input_tokens);
            let output_tok = agent_metrics.map_or(0, |m| m.output_tokens);
            let cache_tok = agent_metrics.map_or(0, |m| m.cache_read_tokens);

            let name = truncate_str(&agent.name, 16);
            let name_style = if is_selected {
                theme.style_fg().add_modifier(Modifier::BOLD)
            } else {
                theme.style_fg()
            };

            lines.push(Line::from(vec![
                Span::styled(marker, marker_style),
                Span::styled(format!("{name:<16}"), name_style),
                Span::styled(format!("{turns:>6}"), theme.style_fg()),
                Span::styled(
                    format!("{:>9}", MetricsState::format_tokens(input_tok)),
                    theme.style_accent(),
                ),
                Span::styled(
                    format!("{:>9}", MetricsState::format_tokens(output_tok)),
                    theme.style_accent(),
                ),
                Span::styled(
                    format!("{:>8}", MetricsState::format_tokens(cache_tok)),
                    theme.style_muted(),
                ),
            ]));
        }
    }

    let block = Block::default().borders(Borders::NONE);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_status_bar(frame: &mut Frame, area: Rect, theme: &Theme) {
    let spans = vec![
        Span::raw("  "),
        Span::styled("j/k", theme.style_accent()),
        Span::styled(" navigate  ", theme.style_dim()),
        Span::styled("r", theme.style_accent()),
        Span::styled(" refresh  ", theme.style_dim()),
        Span::styled("Esc", theme.style_accent()),
        Span::styled(" close", theme.style_dim()),
    ];
    let line = Line::from(spans);
    let paragraph = Paragraph::new(vec![line]);
    frame.render_widget(paragraph, area);
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let mut result = String::with_capacity(max_chars + 1);
    let mut count = 0;
    while count < max_chars {
        match chars.next() {
            Some(c) => {
                result.push(c);
                count += 1;
            }
            None => break,
        }
    }
    if chars.next().is_some() {
        // Replace last char with ellipsis indicator.
        result.pop();
        result.push('…');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn truncate_str_long() {
        let result = truncate_str("hello world", 8);
        assert!(result.ends_with('…'));
        assert_eq!(result.chars().count(), 8);
    }

    #[test]
    fn truncate_str_empty() {
        assert_eq!(truncate_str("", 5), "");
    }
}
