use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};

/// Width percentage for the settings popup.
const POPUP_WIDTH_PCT: u16 = 70;
/// Height percentage for the settings popup.
const POPUP_HEIGHT_PCT: u16 = 85;
/// Fixed column width for field labels (padded with spaces on the right).
const LABEL_COLUMN_WIDTH: usize = 28;
/// Fixed column width for displayed field values.
const VALUE_COLUMN_WIDTH: usize = 12;
/// Lines emitted per settings section: one blank line + one header line + one blank line.
const LINES_PER_SECTION: usize = 3;
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};

use crate::state::settings::{FieldType, SaveStatus, SettingsOverlay};
use crate::theme::Theme;

pub(crate) fn render(overlay: &SettingsOverlay, frame: &mut Frame, area: Rect, theme: &Theme) {
    let popup = centered_rect(POPUP_WIDTH_PCT, POPUP_HEIGHT_PCT, area);
    frame.render_widget(Clear, popup);

    let mut lines: Vec<Line> = Vec::new();
    let mut field_idx: usize = 0;

    for section in &overlay.sections {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!("  {}", section.name),
            Style::default()
                .fg(theme.text.fg)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::raw(""));

        for field in &section.fields {
            let selected = field_idx == overlay.cursor;
            let changed = field.value != field.original_value;
            let marker = if selected { "▸" } else { " " };

            let label_style = if selected {
                Style::default()
                    .fg(theme.colors.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                theme.style_fg()
            };

            let value_str = format_value(&field.value);
            let value_style = if changed {
                Style::default()
                    .fg(theme.status.warning)
                    .add_modifier(Modifier::BOLD)
            } else {
                theme.style_dim()
            };

            let tag = match field.field_type {
                FieldType::Bool if field.editable => "[toggle]",
                FieldType::Integer | FieldType::Text if field.editable => "[edit]",
                _ => "[readonly]",
            };
            let tag_style = if field.editable {
                theme.style_muted()
            } else {
                theme.style_dim()
            };

            let restart = if field.requires_restart { " *" } else { "" };

            if selected && let Some(ref edit) = overlay.editing {
                lines.push(Line::from(vec![
                    Span::raw(format!("  {} ", marker)),
                    Span::styled(format!("{:<LABEL_COLUMN_WIDTH$}", field.label), label_style),
                    Span::styled(
                        &edit.buffer,
                        Style::default()
                            .fg(theme.colors.accent)
                            .add_modifier(Modifier::UNDERLINED),
                    ),
                    Span::styled("▎", Style::default().fg(theme.colors.accent)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw(format!("  {} ", marker)),
                    Span::styled(format!("{:<LABEL_COLUMN_WIDTH$}", field.label), label_style),
                    Span::styled(format!("{:<VALUE_COLUMN_WIDTH$}", value_str), value_style),
                    Span::styled(tag, tag_style),
                    Span::styled(restart, Style::default().fg(theme.status.warning)),
                ]));
            }

            field_idx += 1;
        }
    }

    lines.push(Line::raw(""));
    match &overlay.save_status {
        SaveStatus::Saving => {
            lines.push(Line::from(Span::styled(
                "  Saving...",
                Style::default().fg(theme.status.spinner),
            )));
        }
        SaveStatus::Success => {
            lines.push(Line::from(Span::styled(
                "  Config saved and reloaded",
                Style::default().fg(theme.status.success),
            )));
        }
        SaveStatus::Error(msg) => {
            lines.push(Line::from(Span::styled(
                format!("  Error: {msg}"),
                theme.style_error(),
            )));
        }
        // NOTE: idle state has no status line to render
        SaveStatus::Idle => {}
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "[S]",
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("ave  ", theme.style_muted()),
        Span::styled(
            "[R]",
            Style::default()
                .fg(theme.text.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("eset  ", theme.style_muted()),
        Span::styled(
            "[Esc]",
            Style::default()
                .fg(theme.text.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", theme.style_muted()),
        Span::styled("    * requires restart", theme.style_dim()),
    ]));

    let title = " Settings ";
    let block = Block::default()
        .title(title)
        .title_style(theme.style_accent_bold())
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(theme.style_border())
        .style(Style::default().bg(theme.colors.surface));

    let inner = block.inner(popup);
    let visible_height = usize::from(inner.height);
    let total_lines = lines.len();

    let cursor_line = estimate_cursor_line(overlay);
    let scroll = if cursor_line >= overlay.scroll_offset + visible_height {
        cursor_line.saturating_sub(visible_height - 1)
    } else if cursor_line < overlay.scroll_offset {
        cursor_line
    } else {
        overlay.scroll_offset
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, popup);

    if total_lines > visible_height {
        let mut scrollbar_state =
            ScrollbarState::new(total_lines.saturating_sub(visible_height)).position(scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            popup,
            &mut scrollbar_state,
        );
    }
}

fn format_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".to_owned(),
        _ => value.to_string(),
    }
}

fn estimate_cursor_line(overlay: &SettingsOverlay) -> usize {
    let mut line = 0;
    let mut field_idx = 0;
    for section in &overlay.sections {
        line += LINES_PER_SECTION;
        for _ in &section.fields {
            if field_idx == overlay.cursor {
                return line;
            }
            line += 1;
            field_idx += 1;
        }
    }
    line
}

#[expect(
    clippy::indexing_slicing,
    reason = "Layout.split() returns exactly as many Rects as constraints; [1] is valid for both 3-element constraint arrays"
)]
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let v = Layout::default()
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
        .split(v[1])[1]
}
