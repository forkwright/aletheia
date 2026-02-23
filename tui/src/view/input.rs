use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::App;
use crate::theme::ThemePalette;

pub fn render(app: &App, frame: &mut Frame, area: Rect, theme: &ThemePalette) {
    let is_streaming = app.active_turn_id.is_some();

    let prompt_str = if is_streaming { "queued › " } else { "› " };
    let prompt_color = if is_streaming { theme.warning } else { theme.accent };

    let prompt = Span::styled(prompt_str, Style::default().fg(prompt_color));
    let input_text = &app.input.text;

    let line = Line::from(vec![prompt, Span::styled(input_text.clone(), theme.style_fg())]);

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.separator));

    let paragraph = Paragraph::new(line).block(block).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);

    // Calculate cursor position with wrapping
    let prompt_width = prompt_str.len() as u16;
    let content_width = area.width.saturating_sub(1); // account for possible border
    let total_offset = prompt_width + app.input.cursor as u16;
    let cursor_y = area.y + 1 + (total_offset / content_width); // +1 for top border
    let cursor_x = area.x + (total_offset % content_width);
    frame.set_cursor_position((cursor_x, cursor_y));
}
