//! Ratatui rendering for the setup wizard.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::theme::Theme;
use crate::wizard::state::{
    EditState, FieldKind, STEP_LABELS, StepState, TOTAL_STEPS, WizardState,
};

/// Fixed width for field labels in the field list.
const LABEL_WIDTH: usize = 24;
/// Character used to mask secret field values.
const MASK_CHAR: char = '●';

/// Render the entire wizard into `area`.
pub(crate) fn render(state: &WizardState, frame: &mut Frame, area: Rect, theme: &Theme) {
    let [header_area, progress_area, body_area, footer_area] = split_vertical(area, &[3, 3, 0, 3]);

    render_header(state, frame, header_area, theme);
    render_progress(state, frame, progress_area, theme);
    render_body(state, frame, body_area, theme);
    render_footer(state, frame, footer_area, theme);
}

// ─── Header ─────────────────────────────────────────────────────────────────

fn render_header(state: &WizardState, frame: &mut Frame, area: Rect, theme: &Theme) {
    let step_num = state.step + 1;
    let right = format!(" Step {step_num} / {TOTAL_STEPS} ");
    let title = " ◆ Aletheia Setup Wizard ";

    // Pad title to fill available width, pushing step counter to the right
    let inner_width = usize::from(area.width.saturating_sub(2));
    let pad = inner_width.saturating_sub(title.len() + right.len());
    let full_title = format!("{}{:pad$}{}", title, "", right, pad = pad);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(theme.style_border())
        .style(Style::default().bg(theme.colors.surface));

    let para = Paragraph::new(Line::from(vec![
        Span::styled(title, theme.style_accent_bold()),
        Span::styled(
            format!("{:pad$}{}", "", right, pad = pad),
            theme.style_muted(),
        ),
    ]))
    .block(block);
    let _ = full_title; // consumed above via pad calculation
    frame.render_widget(para, area);
}

// ─── Progress indicator ──────────────────────────────────────────────────────

fn render_progress(state: &WizardState, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut spans: Vec<Span> = vec![Span::raw("  ")];

    for (i, label) in STEP_LABELS.iter().enumerate() {
        let style = if i < state.step {
            // completed
            Style::default()
                .fg(theme.status.success)
                .add_modifier(Modifier::BOLD)
        } else if i == state.step {
            // current
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            // upcoming
            theme.style_dim()
        };

        let bullet = if i < state.step {
            "✓"
        } else if i == state.step {
            "●"
        } else {
            "○"
        };
        spans.push(Span::styled(format!("{bullet} {label}"), style));

        if i + 1 < STEP_LABELS.len() {
            spans.push(Span::styled("  ─  ", theme.style_dim()));
        }
    }

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(theme.style_border())
        .style(Style::default().bg(theme.colors.surface));

    let para = Paragraph::new(Line::from(spans)).block(block);
    frame.render_widget(para, area);
}

// ─── Body (current step fields) ──────────────────────────────────────────────

fn render_body(state: &WizardState, frame: &mut Frame, area: Rect, theme: &Theme) {
    let Some(step) = state.current_step() else {
        return;
    };

    let step_label = STEP_LABELS.get(state.step).copied().unwrap_or("Setup");

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        format!("  {step_label}"),
        Style::default()
            .fg(theme.text.fg)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        format!(
            "  {}",
            "─".repeat(usize::from(area.width).saturating_sub(4))
        ),
        theme.style_dim(),
    )));
    lines.push(Line::raw(""));

    render_fields(step, &mut lines, theme);

    // Ready-step confirmation hint
    if state.step == TOTAL_STEPS - 1 {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("Press Enter", theme.style_accent_bold()),
            Span::styled(" to create your instance  or  ", theme.style_muted()),
            Span::styled("b", theme.style_accent_bold()),
            Span::styled(" to go back", theme.style_muted()),
        ]));
    }

    let block = Block::default().style(Style::default().bg(theme.colors.surface));
    let para = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn render_fields(step: &StepState, lines: &mut Vec<Line>, theme: &Theme) {
    for (i, field) in step.fields.iter().enumerate() {
        let selected = i == step.cursor;
        let marker = if selected { "▸" } else { " " };

        let label_style = if selected {
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        let label_padded = format!("{:<LABEL_WIDTH$}", field.label);

        let value_span: Span = match &field.kind {
            FieldKind::Text { secret } => {
                if selected {
                    if let Some(ref edit) = step.editing {
                        // Active edit mode
                        editing_span(edit, *secret, theme)
                    } else {
                        idle_text_span(&field.value, *secret, theme)
                    }
                } else {
                    idle_text_span(&field.value, *secret, theme)
                }
            }
            FieldKind::Select { options } => {
                let label = options
                    .iter()
                    .find(|o| o.value == field.value)
                    .map(|o| o.label)
                    .unwrap_or(&field.value);

                let style = if selected {
                    Style::default()
                        .fg(theme.colors.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    theme.style_fg()
                };

                Span::styled(format!("◀ {label} ▶"), style)
            }
            FieldKind::ReadOnly => Span::styled(field.value.clone(), theme.style_muted()),
        };

        let hint_span = if field.hint.is_empty() {
            Span::raw("")
        } else {
            Span::styled(format!("  {}", field.hint), theme.style_dim())
        };

        let mut row_spans = vec![
            Span::raw(format!("  {marker} ")),
            Span::styled(label_padded, label_style),
            value_span,
            hint_span,
        ];

        // Append inline cursor character for active text edit
        if selected
            && let Some(ref edit) = step.editing
            && let FieldKind::Text { .. } = field.kind
        {
            // Cursor marker already embedded in editing_span; add closing bracket
            row_spans.push(Span::styled("]", theme.style_dim()));
            let _ = edit; // borrow used above
        }

        lines.push(Line::from(row_spans));
    }
}

/// Render a text field that is currently being edited.
fn editing_span(edit: &EditState, secret: bool, theme: &Theme) -> Span<'static> {
    let display = if secret {
        let mut s = MASK_CHAR.to_string().repeat(edit.cursor);
        s.push('▎');
        s
    } else {
        let before = &edit.buffer[..edit.cursor];
        format!("{before}▎")
    };

    Span::styled(
        format!("[{display}"),
        Style::default()
            .fg(theme.colors.accent)
            .add_modifier(Modifier::UNDERLINED),
    )
}

/// Render a text field in idle (non-editing) state.
fn idle_text_span(value: &str, secret: bool, theme: &Theme) -> Span<'static> {
    let display = if secret && !value.is_empty() {
        MASK_CHAR.to_string().repeat(value.chars().count())
    } else {
        value.to_owned()
    };

    Span::styled(format!("[{display}]"), theme.style_dim())
}

// ─── Footer ──────────────────────────────────────────────────────────────────

fn render_footer(state: &WizardState, frame: &mut Frame, area: Rect, theme: &Theme) {
    let is_editing = state
        .current_step()
        .map(|s| s.editing.is_some())
        .unwrap_or(false);

    let is_on_select = state
        .current_step()
        .and_then(|s| s.current_field())
        .is_some_and(|f| matches!(f.kind, FieldKind::Select { .. }));

    let key_style = Style::default()
        .fg(theme.colors.accent)
        .add_modifier(Modifier::BOLD);
    let sep_style = theme.style_muted();
    let esc_style = Style::default()
        .fg(theme.text.fg_dim)
        .add_modifier(Modifier::BOLD);

    let mut spans: Vec<Span> = vec![Span::raw("  ")];

    if is_editing {
        spans.extend([
            Span::styled("Enter", key_style),
            Span::styled(" confirm  ", sep_style),
            Span::styled("Esc", esc_style),
            Span::styled(" cancel edit", sep_style),
        ]);
    } else if is_on_select {
        spans.extend([
            Span::styled("Enter", key_style),
            Span::styled("/", sep_style),
            Span::styled("←→", key_style),
            Span::styled(" cycle  ", sep_style),
            Span::styled("↑↓", key_style),
            Span::styled(" move  ", sep_style),
        ]);
        spans.extend(keybinding_next_back_spans(state, key_style, sep_style));
    } else {
        spans.extend([
            Span::styled("↑↓", key_style),
            Span::styled(" move  ", sep_style),
            Span::styled("Enter", key_style),
            Span::styled(" edit  ", sep_style),
        ]);
        spans.extend(keybinding_next_back_spans(state, key_style, sep_style));
    }

    spans.extend([
        Span::styled("  ", sep_style),
        Span::styled("Esc", esc_style),
        Span::styled(" abort", sep_style),
    ]);

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(theme.style_border())
        .style(Style::default().bg(theme.colors.surface));

    let para = Paragraph::new(Line::from(spans)).block(block);
    frame.render_widget(para, area);
}

/// Build next/back keybinding spans for the footer.
fn keybinding_next_back_spans(
    state: &WizardState,
    key_style: Style,
    sep_style: Style,
) -> Vec<Span<'static>> {
    if state.step == 0 {
        vec![
            Span::styled("n", key_style),
            Span::styled(" next", sep_style),
        ]
    } else if state.step == TOTAL_STEPS - 1 {
        vec![
            Span::styled("b", key_style),
            Span::styled(" back", sep_style),
        ]
    } else {
        vec![
            Span::styled("n", key_style),
            Span::styled(" next  ", sep_style),
            Span::styled("b", key_style),
            Span::styled(" back", sep_style),
        ]
    }
}

// ─── Layout helper ───────────────────────────────────────────────────────────

/// Split `area` vertically into fixed-height rows.  A height of `0` means fill remaining space.
#[expect(
    clippy::indexing_slicing,
    reason = "slice built from caller-supplied array; len matches N, so indexing [0..N] is valid"
)]
fn split_vertical<const N: usize>(area: Rect, heights: &[u16; N]) -> [Rect; N] {
    let constraints: Vec<Constraint> = heights
        .iter()
        .map(|&h| {
            if h == 0 {
                Constraint::Fill(1)
            } else {
                Constraint::Length(h)
            }
        })
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut result = [Rect::default(); N];
    for i in 0..N {
        result[i] = chunks[i];
    }
    result
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use crate::wizard::state::WizardState;

    #[test]
    fn split_vertical_fills_remaining() {
        let area = Rect::new(0, 0, 80, 24);
        let [header, progress, body, footer] = split_vertical(area, &[3, 3, 0, 3]);
        assert_eq!(header.height, 3);
        assert_eq!(progress.height, 3);
        assert_eq!(footer.height, 3);
        assert_eq!(body.height, 15);
    }

    #[test]
    fn editing_span_masks_secret_at_cursor() {
        let edit = EditState {
            buffer: "abcde".to_owned(),
            cursor: 3,
        };
        let theme = crate::theme::Theme::default();
        let span = editing_span(&edit, true, &theme);
        // Should have 3 mask chars + cursor marker
        assert!(span.content.contains('▎'));
        assert!(!span.content.contains('a'));
    }

    #[test]
    fn editing_span_shows_text_for_non_secret() {
        let edit = EditState {
            buffer: "hello".to_owned(),
            cursor: 3,
        };
        let theme = crate::theme::Theme::default();
        let span = editing_span(&edit, false, &theme);
        assert!(span.content.contains("hel"));
        assert!(span.content.contains('▎'));
    }

    #[test]
    fn idle_text_span_masks_secret() {
        let theme = crate::theme::Theme::default();
        let span = idle_text_span("abc", true, &theme);
        assert!(!span.content.contains('a'));
        assert!(span.content.contains(MASK_CHAR));
    }

    #[test]
    fn idle_text_span_shows_plain_text() {
        let theme = crate::theme::Theme::default();
        let span = idle_text_span("hello", false, &theme);
        assert!(span.content.contains("hello"));
    }

    #[test]
    fn wizard_state_current_step_returns_correct_step() {
        let state = WizardState::new(None, None);
        assert!(state.current_step().is_some());
    }
}
