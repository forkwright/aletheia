/// Render the operations pane: right-side panel showing thinking, tool calls, and diffs.
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;
use crate::state::ops::{FocusedPane, OpsToolStatus, ToolCategory};
use crate::theme::{self, Theme};

const THINKING_TRUNCATE_LINES: usize = 20;
const TOOL_OUTPUT_TRUNCATE_LINES: usize = 15;
const JSON_TRUNCATE_BYTES: usize = 300;
const MS_PER_SECOND: u64 = 1000;
/// Minimum column width reserved for the chat pane when calculating ops pane size.
const MIN_CHAT_PANE_WIDTH: u16 = 40;
/// Minimum column width for the operations pane.
const MIN_OPS_PANE_WIDTH: u16 = 20;

pub(crate) fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let ops = &app.layout.ops;
    let focused = ops.focused_pane == FocusedPane::Operations;

    let border_style = if focused {
        Style::default().fg(theme.borders.focused)
    } else {
        Style::default().fg(theme.borders.normal)
    };

    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(border_style)
        .title(Span::styled(
            " Activity ",
            if focused {
                theme.style_accent_bold()
            } else {
                theme.style_dim()
            },
        ));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    let inner_width = usize::from(inner.width.saturating_sub(1));
    let mut item_idx: usize = 0;

    // Summary row: total calls, errors, elapsed time.
    if ops.summary.total_calls > 0 || ops.turn_started_at.is_some() {
        render_summary_row(ops, &mut lines, theme);
        lines.push(Line::raw(""));
    }

    if !ops.thinking.text.is_empty() {
        let is_selected = ops.selected_item == Some(item_idx);
        render_thinking_block(
            &ops.thinking,
            &mut lines,
            inner_width,
            theme,
            is_selected,
            app.viewport.tick_count,
        );
        item_idx += 1;
    }

    let mut hidden_count = 0usize;
    for tc in &ops.tool_calls {
        let is_selected = ops.selected_item == Some(item_idx);
        if tc.status == OpsToolStatus::Complete && !ops.show_all_successful {
            hidden_count += 1;
        } else {
            render_tool_call(
                tc,
                &mut lines,
                inner_width,
                theme,
                is_selected,
                app.viewport.tick_count,
            );
        }
        item_idx += 1;
    }
    if hidden_count > 0 {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("▸ {hidden_count} successful  s=show all"),
                theme.style_dim(),
            ),
        ]));
    }

    if !ops.diffs.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("diffs", theme.style_accent_bold()),
        ]));
        for diff in &ops.diffs {
            render_diff(diff, &mut lines, theme);
        }
    }

    if lines.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("no operations", theme.style_dim()),
        ]));
        if !app.layout.ops.visible {
            return;
        }
    }

    let visible_height = usize::from(inner.height);
    let total_lines = lines.len();
    let scroll = if total_lines > visible_height {
        total_lines
            .saturating_sub(visible_height)
            .saturating_sub(ops.scroll_offset)
    } else {
        0
    };

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0));

    frame.render_widget(paragraph, inner);
}

#[expect(
    clippy::indexing_slicing,
    reason = "THINKING_TRUNCATE_LINES / TOOL_OUTPUT_TRUNCATE_LINES are checked before slicing via the `truncated` boolean"
)]
fn render_thinking_block(
    thinking: &crate::state::ops::OpsThinkingBlock,
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    theme: &Theme,
    is_selected: bool,
    tick_count: u64,
) {
    let marker = if is_selected { "▸" } else { " " };
    let marker_style = if is_selected {
        Style::default().fg(theme.borders.selected)
    } else {
        Style::default()
    };

    let collapse_icon = if thinking.collapsed { "▶" } else { "▼" };

    lines.push(Line::from(vec![
        Span::styled(marker, marker_style),
        Span::styled(
            format!(" {} thinking", collapse_icon),
            Style::default()
                .fg(theme.thinking.fg)
                .add_modifier(Modifier::ITALIC),
        ),
    ]));

    if thinking.collapsed {
        let preview_line = thinking
            .text
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(inner_width.saturating_sub(4))
            .collect::<String>();
        lines.push(Line::from(vec![
            Span::raw("   "),
            Span::styled(
                format!("{preview_line}..."),
                Style::default().fg(theme.thinking.fg),
            ),
        ]));
    } else {
        let text_lines: Vec<&str> = thinking.text.lines().collect();
        let truncated = text_lines.len() > THINKING_TRUNCATE_LINES;
        let display_lines = if truncated {
            &text_lines[..THINKING_TRUNCATE_LINES]
        } else {
            &text_lines
        };

        for line in display_lines {
            let display = if line.len() > inner_width.saturating_sub(3) {
                line.get(..inner_width.saturating_sub(6)).unwrap_or(line)
            } else {
                line
            };
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(
                    display.to_string(),
                    Style::default()
                        .fg(theme.thinking.fg)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));
        }

        if truncated {
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled(
                    format!(
                        "... ({} more lines)",
                        text_lines.len() - THINKING_TRUNCATE_LINES
                    ),
                    theme.style_dim(),
                ),
            ]));
        }

        let ch = theme::spinner_frame(tick_count);
        lines.push(Line::from(vec![
            Span::raw("   "),
            Span::styled(
                ch.to_string(),
                Style::default()
                    .fg(theme.status.spinner)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    lines.push(Line::raw(""));
}

#[expect(
    clippy::indexing_slicing,
    reason = "TOOL_OUTPUT_TRUNCATE_LINES < output_lines.len() is checked by the `truncated` boolean before slicing"
)]
#[expect(
    clippy::cast_precision_loss,
    reason = "millisecond durations never approach f64 precision limits"
)]
fn render_tool_call(
    tc: &crate::state::ops::OpsToolCall,
    lines: &mut Vec<Line<'static>>,
    inner_width: usize,
    theme: &Theme,
    is_selected: bool,
    tick_count: u64,
) {
    let marker = if is_selected { "▸" } else { " " };
    let marker_style = if is_selected {
        Style::default().fg(theme.borders.selected)
    } else {
        Style::default()
    };

    let (status_icon, status_style) = match tc.status {
        OpsToolStatus::Running => {
            let ch = theme::spinner_frame(tick_count);
            (
                ch.to_string(),
                Style::default()
                    .fg(theme.status.spinner)
                    .add_modifier(Modifier::BOLD),
            )
        }
        OpsToolStatus::Complete => (
            "\u{2705}".to_string(), // checkmark
            theme.style_success(),
        ),
        OpsToolStatus::Failed => (
            "\u{274C}".to_string(), // cross
            theme.style_error(),
        ),
    };

    let duration_str = if let Some(ms) = tc.duration_ms {
        if ms >= MS_PER_SECOND {
            Some(format!(" ({:.1}s)", ms as f64 / MS_PER_SECOND as f64))
        } else {
            Some(format!(" ({}ms)", ms))
        }
    } else if tc.status == OpsToolStatus::Running {
        let secs = tc.started_at.elapsed().as_secs();
        if secs > 0 {
            Some(format!(" ({secs}s)"))
        } else {
            None
        }
    } else {
        None
    };

    let icon = tc.category.icon();
    let icon_style = if tc.category.is_destructive() {
        theme.style_error()
    } else if tc.category.is_read_only() {
        theme.style_dim()
    } else {
        theme.style_muted()
    };

    let mut header = vec![
        Span::styled(marker, marker_style),
        Span::styled(format!(" {status_icon}"), status_style),
        Span::styled(format!(" {icon}"), icon_style),
        Span::styled(
            format!(" {}", tc.name),
            Style::default()
                .fg(theme.text.fg)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(ref arg) = tc.primary_arg {
        header.push(Span::styled(format!(" {arg}"), theme.style_muted()));
    }

    if let Some(ref dur) = duration_str {
        header.push(Span::styled(dur.clone(), theme.style_dim()));
    }

    lines.push(Line::from(header));

    if let Some(ref err) = tc.error_message {
        lines.push(Line::from(vec![
            Span::raw("   "),
            Span::styled(format!("\u{2514} {err}"), theme.style_error()),
        ]));
    }

    if tc.expanded {
        if let Some(ref input) = tc.input_json {
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled("input:", theme.style_muted()),
            ]));
            let truncated_input = if input.len() > JSON_TRUNCATE_BYTES {
                format!("{}...", input.get(..JSON_TRUNCATE_BYTES).unwrap_or(input))
            } else {
                input.clone()
            };
            for json_line in truncated_input.lines() {
                let display = if json_line.len() > inner_width.saturating_sub(4) {
                    json_line
                        .get(..inner_width.saturating_sub(7))
                        .unwrap_or(json_line)
                } else {
                    json_line
                };
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(display.to_string(), theme.style_dim()),
                ]));
            }
        }

        if let Some(ref output) = tc.output {
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled("output:", theme.style_muted()),
            ]));
            let output_lines: Vec<&str> = output.lines().collect();
            let truncated = output_lines.len() > TOOL_OUTPUT_TRUNCATE_LINES;
            let display_lines = if truncated {
                &output_lines[..TOOL_OUTPUT_TRUNCATE_LINES]
            } else {
                &output_lines
            };

            for out_line in display_lines {
                let display = if out_line.len() > inner_width.saturating_sub(4) {
                    out_line
                        .get(..inner_width.saturating_sub(7))
                        .unwrap_or(out_line)
                } else {
                    out_line
                };
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(display.to_string(), theme.style_dim()),
                ]));
            }

            if truncated {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(
                        format!(
                            "... ({} more lines)",
                            output_lines.len() - TOOL_OUTPUT_TRUNCATE_LINES
                        ),
                        theme.style_dim(),
                    ),
                ]));
            }
        }
    }
}

fn render_diff(
    diff: &crate::state::ops::OpsDiffEntry,
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
) {
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("\u{2500}\u{2500} {}", diff.file_path),
            theme.style_accent(),
        ),
    ]));

    for del in &diff.deletions {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("- {del}"), Style::default().fg(theme.status.error)),
        ]));
    }

    for add in &diff.additions {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("+ {add}"),
                Style::default().fg(theme.status.success),
            ),
        ]));
    }
}

/// Category display order for consistent KPI rendering.
const CATEGORY_ORDER: &[ToolCategory] = &[
    ToolCategory::Read,
    ToolCategory::Write,
    ToolCategory::Search,
    ToolCategory::Exec,
    ToolCategory::Http,
    ToolCategory::Other,
];

fn render_summary_row(
    ops: &crate::state::ops::OpsState,
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
) {
    let elapsed = ops
        .turn_started_at
        .map(|t| {
            let secs = t.elapsed().as_secs();
            if secs < 60 {
                format!("{secs}s")
            } else {
                format!("{}m{}s", secs / 60, secs % 60)
            }
        })
        .unwrap_or_default();

    let summary = &ops.summary;
    let mut spans = vec![
        Span::styled(" ", Style::default()),
        Span::styled(summary.total_calls.to_string(), theme.style_accent_bold()),
        Span::styled(" calls", theme.style_muted()),
    ];

    if summary.total_errors > 0 {
        spans.push(Span::styled(" · ", theme.style_dim()));
        spans.push(Span::styled(
            summary.total_errors.to_string(),
            theme.style_error(),
        ));
        spans.push(Span::styled(" err", theme.style_error()));
    }

    if !elapsed.is_empty() {
        spans.push(Span::styled(" · ", theme.style_dim()));
        spans.push(Span::styled(elapsed, theme.style_dim()));
    }

    lines.push(Line::from(spans));

    // Per-category rows with tallies and percentiles.
    for cat in CATEGORY_ORDER {
        if let Some(stats) = summary.categories.get(cat) {
            let icon_style = if cat.is_destructive() {
                theme.style_error()
            } else if cat.is_read_only() {
                theme.style_dim()
            } else {
                theme.style_muted()
            };
            let mut cat_spans = vec![
                Span::styled("  ", Style::default()),
                Span::styled(cat.icon(), icon_style),
                Span::styled(format!(" {}", cat.display_name()), theme.style_muted()),
                Span::styled(" ", Style::default()),
                Span::styled(stats.success.to_string(), theme.style_success()),
                Span::styled("/", theme.style_dim()),
            ];

            if stats.fail > 0 {
                cat_spans.push(Span::styled(stats.fail.to_string(), theme.style_error()));
            } else {
                cat_spans.push(Span::styled("0", theme.style_dim()));
            }

            if let Some(p50) = stats.percentile(50) {
                cat_spans.push(Span::styled(format!("  p50={p50}ms"), theme.style_dim()));
            }
            if let Some(p95) = stats.percentile(95) {
                cat_spans.push(Span::styled(format!(" p95={p95}ms"), theme.style_dim()));
            }

            lines.push(Line::from(cat_spans));
        }
    }
}

/// Calculate the width for the ops pane in columns, respecting minimum widths.
pub(crate) fn ops_pane_width(total_width: u16, pct: u16) -> u16 {
    let available = total_width.saturating_sub(MIN_CHAT_PANE_WIDTH);
    let desired = u16::try_from(u32::from(total_width) * u32::from(pct) / 100).unwrap_or(u16::MAX);
    desired.clamp(MIN_OPS_PANE_WIDTH, available.max(MIN_OPS_PANE_WIDTH))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_pane_width_normal_terminal() {
        let w = ops_pane_width(120, 40);
        assert_eq!(w, 48); // 120 * 40 / 100
    }

    #[test]
    fn ops_pane_width_narrow_terminal() {
        let w = ops_pane_width(80, 40);
        assert_eq!(w, 32); // 80 * 40 / 100
    }

    #[test]
    fn ops_pane_width_very_narrow() {
        let w = ops_pane_width(60, 40);
        // 60 * 40 / 100 = 24, available = 60 - 40 = 20, min_ops = 20
        assert_eq!(w, 20);
    }

    #[test]
    fn ops_pane_width_respects_min() {
        let w = ops_pane_width(100, 10);
        // 100 * 10 / 100 = 10, clamped to min_ops = 20
        assert_eq!(w, 20);
    }

    #[test]
    fn ops_pane_width_respects_max() {
        let w = ops_pane_width(100, 80);
        // 100 * 80 / 100 = 80, available = 100 - 40 = 60
        assert_eq!(w, 60);
    }
}
