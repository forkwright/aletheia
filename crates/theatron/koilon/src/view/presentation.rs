//! Presentation projections shared across koilon view modules.
//!
//! WHY(#7031): `push_mutation_status`, integer token-count abbreviation, and
//! the "Esc back" status bar were each duplicated verbatim across sibling
//! view modules. Consolidated here so a new `ControlMutationStatus` variant
//! or a formatting change produces one compile/test failure path.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::state::ControlMutationStatus;
use crate::theme::Theme;

/// Append the pending/confirmed/failed lines for a control-mutation status
/// to an in-progress overlay line buffer. No-op when idle.
pub(crate) fn push_mutation_status<'a>(
    lines: &mut Vec<Line<'a>>,
    status: &'a ControlMutationStatus,
    theme: &Theme,
) {
    match status {
        ControlMutationStatus::Idle => {}
        ControlMutationStatus::Pending { action_id } => {
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("  Pending: ", theme.style_muted()),
                Span::styled(action_id.clone(), theme.style_warning()),
            ]));
        }
        ControlMutationStatus::Succeeded { action_id } => {
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("  Confirmed: ", theme.style_muted()),
                Span::styled(action_id.clone(), theme.style_success()),
            ]));
        }
        ControlMutationStatus::Failed { action_id, message } => {
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("  Failed: ", theme.style_muted()),
                Span::styled(action_id.clone(), theme.style_error_bold()),
            ]));
            lines.push(Line::from(Span::styled(
                format!("  {message}"),
                theme.style_error(),
            )));
        }
    }
}

/// Abbreviate a token count to an integer `K`/`M` suffix form (e.g. `1500`
/// -> `"1K"`, `2_000_000` -> `"2M"`). Distinct from
/// `proskenion::state::metrics::format_tokens`, which renders one decimal
/// place — the two clients intentionally differ here.
pub(crate) fn format_token_count(n: u32) -> String {
    if n >= 1_000_000 {
        format!("{}M", n / 1_000_000)
    } else if n >= 1_000 {
        format!("{}K", n / 1_000)
    } else {
        format!("{n}")
    }
}

/// Render the shared "Esc back" status bar line used by back-navigable views.
pub(crate) fn render_status_bar(frame: &mut Frame, area: Rect, theme: &Theme) {
    let line = Line::from(vec![
        Span::raw("  "),
        Span::styled("Esc", theme.style_accent()),
        Span::styled(" back", theme.style_dim()),
    ]);
    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_token_count_boundary_values() {
        assert_eq!(format_token_count(0), "0");
        assert_eq!(format_token_count(999), "999");
        assert_eq!(format_token_count(1_000), "1K");
        assert_eq!(format_token_count(1_500), "1K");
        assert_eq!(format_token_count(999_999), "999K");
        assert_eq!(format_token_count(1_000_000), "1M");
        assert_eq!(format_token_count(2_500_000), "2M");
    }

    #[test]
    fn push_mutation_status_idle_appends_nothing() {
        let theme = Theme::detect();
        let mut lines = Vec::new();
        push_mutation_status(&mut lines, &ControlMutationStatus::Idle, &theme);
        assert!(lines.is_empty());
    }

    #[test]
    fn push_mutation_status_pending_appends_two_lines() {
        let theme = Theme::detect();
        let mut lines = Vec::new();
        let status = ControlMutationStatus::Pending {
            action_id: "abc".to_string(),
        };
        push_mutation_status(&mut lines, &status, &theme);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn push_mutation_status_failed_appends_three_lines() {
        let theme = Theme::detect();
        let mut lines = Vec::new();
        let status = ControlMutationStatus::Failed {
            action_id: "abc".to_string(),
            message: "boom".to_string(),
        };
        push_mutation_status(&mut lines, &status, &theme);
        assert_eq!(lines.len(), 3);
    }
}
