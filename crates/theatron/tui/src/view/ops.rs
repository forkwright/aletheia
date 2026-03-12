/// Render the operations pane — right-side panel showing thinking, tool calls, and diffs.
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;
use crate::state::ops::{FocusedPane, OpsToolStatus};
use crate::theme::{self, Theme};

const THINKING_TRUNCATE_LINES: usize = 20;
const TOOL_OUTPUT_TRUNCATE_LINES: usize = 15;
const JSON_TRUNCATE_BYTES: usize = 300;
const MS_PER_SECOND: u64 = 1000;

pub fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let ops = &app.ops;
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
            " ops ",
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
    let inner_width = inner.width.saturating_sub(1) as usize;
    let mut item_idx: usize = 0;

    // --- Thinking block ---
    if !ops.thinking.text.is_empty() {
        let is_selected = ops.selected_item == Some(item_idx);
        render_thinking_block(
            &ops.thinking,
            &mut lines,
            inner_width,
            theme,
            is_selected,
            app.tick_count,
        );
        item_idx += 1;
    }

    // --- Tool calls ---
    for tc in &ops.tool_calls {
        let is_selected = ops.selected_item == Some(item_idx);
        render_tool_call(
            tc,
            &mut lines,
            inner_width,
            theme,
            is_selected,
            app.tick_count,
        );
        item_idx += 1;
    }

    // --- File diffs ---
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

    // --- Empty state ---
    if lines.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::raw(" "),
            Span::styled("no operations", theme.style_dim()),
        ]));
        if !app.ops.visible {
            return;
        }
    }

    // Calculate scroll
    let visible_height = inner.height as usize;
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
        .scroll((scroll as u16, 0));

    frame.render_widget(paragraph, inner);
}

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

    // Header
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
                &line[..inner_width.saturating_sub(6)]
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

        // Show streaming indicator if thinking text is still growing
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

    // Status indicator
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

    // Duration
    let duration_str = tc.duration_ms.map(|ms| {
        if ms >= MS_PER_SECOND {
            format!(" ({:.1}s)", ms as f64 / MS_PER_SECOND as f64)
        } else {
            format!(" ({}ms)", ms)
        }
    });

    // Header: marker + status + tool name (bold) + duration
    let mut header = vec![
        Span::styled(marker, marker_style),
        Span::styled(format!(" {status_icon}"), status_style),
        Span::styled(
            format!(" {}", tc.name),
            Style::default()
                .fg(theme.text.fg)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(ref dur) = duration_str {
        header.push(Span::styled(dur.clone(), theme.style_dim()));
    }

    lines.push(Line::from(header));

    // Input parameters (collapsed by default, shown when expanded)
    if tc.expanded {
        if let Some(ref input) = tc.input_json {
            lines.push(Line::from(vec![
                Span::raw("   "),
                Span::styled("input:", theme.style_muted()),
            ]));
            let truncated_input = if input.len() > JSON_TRUNCATE_BYTES {
                format!("{}...", &input[..JSON_TRUNCATE_BYTES])
            } else {
                input.clone()
            };
            for json_line in truncated_input.lines() {
                let display = if json_line.len() > inner_width.saturating_sub(4) {
                    &json_line[..inner_width.saturating_sub(7)]
                } else {
                    json_line
                };
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(display.to_string(), theme.style_dim()),
                ]));
            }
        }

        // Output (truncated)
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
                    &out_line[..inner_width.saturating_sub(7)]
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
    // File path header
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("\u{2500}\u{2500} {}", diff.file_path),
            theme.style_accent(),
        ),
    ]));

    // Deletions (red)
    for del in &diff.deletions {
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("- {del}"), Style::default().fg(theme.status.error)),
        ]));
    }

    // Additions (green)
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

/// Calculate the width for the ops pane in columns, respecting minimum widths.
pub fn ops_pane_width(total_width: u16, pct: u16) -> u16 {
    let min_chat = 40u16;
    let min_ops = 20u16;
    let available = total_width.saturating_sub(min_chat);
    let desired = (total_width as u32 * pct as u32 / 100) as u16;
    desired.clamp(min_ops, available.max(min_ops))
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
