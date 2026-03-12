use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::theme::Theme;

pub fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let width = area.width as usize;

    let cursor_char = if app.tick_count % 6 < 3 { "█" } else { " " };
    let (before_cursor, after_cursor) = app.filter.text.split_at(app.filter.cursor);

    let mut left_spans = vec![
        Span::styled("/", theme.style_accent()),
        Span::styled(before_cursor, theme.style_fg()),
        Span::styled(cursor_char, theme.style_accent()),
        Span::styled(after_cursor, theme.style_fg()),
    ];

    let right_text = if app.filter.text.is_empty() {
        String::new()
    } else {
        format!(
            "{}/{} matches",
            app.filter.match_count, app.filter.total_count
        )
    };

    let left_width: usize = left_spans.iter().map(|s| s.content.width()).sum();
    let right_width = right_text.width();

    if !right_text.is_empty() && left_width + right_width + 2 < width {
        let pad = width.saturating_sub(left_width + right_width + 2);
        left_spans.push(Span::raw(" ".repeat(pad)));
        left_spans.push(Span::styled(right_text, theme.style_dim()));
        left_spans.push(Span::raw(" "));
    }

    let line = Line::from(left_spans);
    let bar = Paragraph::new(line).style(theme.style_surface());
    frame.render_widget(bar, area);
}
