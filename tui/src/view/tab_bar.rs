//! Tab bar rendering — shows open tabs at the top of the main area.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::theme::Theme;

pub(crate) fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    if app.tab_bar.tabs.is_empty() || area.width < 4 {
        return;
    }

    let tabs = &app.tab_bar.tabs;
    let active = app.tab_bar.active;
    let width = area.width as usize;

    // Calculate how much space we have for tab titles
    // Each tab: " [title] |" or " [title] " for last
    let separator = " | ";
    let plus_label = " + ";

    let total_tabs = tabs.len();
    let separators_width = if total_tabs > 1 {
        (total_tabs - 1) * separator.len()
    } else {
        0
    };
    let plus_width = plus_label.len();
    let available = width.saturating_sub(separators_width + plus_width + 1); // +1 for leading space

    // Calculate max title length per tab
    let max_per_tab = if total_tabs > 0 {
        (available / total_tabs).max(3)
    } else {
        available
    };

    let mut spans: Vec<Span> = Vec::new();

    for (idx, tab) in tabs.iter().enumerate() {
        let is_active = idx == active;

        // Build title with truncation
        let title = truncate_title(&tab.title, max_per_tab);

        // Unread indicator
        let prefix = if tab.unread && !is_active { "* " } else { " " };

        let style = if is_active {
            Style::default()
                .fg(theme.text.fg)
                .bg(theme.colors.surface)
                .add_modifier(Modifier::BOLD)
        } else if tab.unread {
            Style::default()
                .fg(theme.borders.selected)
                .bg(theme.colors.surface_dim)
        } else {
            Style::default()
                .fg(theme.text.fg_dim)
                .bg(theme.colors.surface_dim)
        };

        spans.push(Span::styled(prefix, style));
        spans.push(Span::styled(title, style));
        spans.push(Span::styled(" ", style));

        if idx < total_tabs - 1 {
            spans.push(Span::styled(
                "\u{2502}",
                Style::default().fg(theme.text.fg_dim),
            ));
        }
    }

    // "+" button
    spans.push(Span::styled(
        plus_label,
        Style::default().fg(theme.text.fg_dim),
    ));

    // Pad remaining width
    let rendered_width: usize = spans.iter().map(|s| s.width()).sum();
    if rendered_width < width {
        spans.push(Span::styled(
            " ".repeat(width - rendered_width),
            Style::default().bg(theme.colors.surface_dim),
        ));
    }

    let line = Line::from(spans);
    let bar = Paragraph::new(line).style(Style::default().bg(theme.colors.surface_dim));
    frame.render_widget(bar, area);
}

/// Truncate a title to fit within max_width characters, adding ellipsis if needed.
fn truncate_title(title: &str, max_width: usize) -> String {
    if title.len() <= max_width {
        return title.to_string();
    }
    if max_width <= 3 {
        return title.chars().take(max_width).collect();
    }
    let mut end = max_width - 1; // leave room for ellipsis char
    while end > 0 && !title.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\u{2026}", &title[..end])
}

/// Whether the tab bar should be shown (more than 1 tab or always-on).
pub(crate) fn should_show(app: &App) -> bool {
    app.tab_bar.len() > 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_title_unchanged() {
        assert_eq!(truncate_title("main", 10), "main");
    }

    #[test]
    fn truncate_exact_length() {
        assert_eq!(truncate_title("main", 4), "main");
    }

    #[test]
    fn truncate_long_title_adds_ellipsis() {
        let result = truncate_title("research-task-long-name", 10);
        assert!(result.len() <= 12); // accounting for multibyte ellipsis
        assert!(result.ends_with('\u{2026}'));
    }

    #[test]
    fn truncate_very_short_max() {
        let result = truncate_title("hello", 2);
        assert_eq!(result, "he");
    }

    #[test]
    fn should_show_with_multiple_tabs() {
        let mut app = crate::app::test_helpers::test_app();
        app.tab_bar
            .create_tab(crate::id::NousId::from("syn"), "tab1");
        app.tab_bar
            .create_tab(crate::id::NousId::from("syn"), "tab2");
        assert!(should_show(&app));
    }

    #[test]
    fn should_not_show_with_single_tab() {
        let mut app = crate::app::test_helpers::test_app();
        app.tab_bar
            .create_tab(crate::id::NousId::from("syn"), "tab1");
        assert!(!should_show(&app));
    }

    #[test]
    fn should_not_show_with_no_tabs() {
        let app = crate::app::test_helpers::test_app();
        assert!(!should_show(&app));
    }
}
