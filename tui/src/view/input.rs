use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let is_streaming = app.active_turn_id.is_some();

    let prompt = if is_streaming {
        Span::styled("queued › ", Style::default().fg(Color::Yellow))
    } else {
        Span::styled("› ", Style::default().fg(Color::Cyan))
    };

    let input_text = &app.input.text;

    let line = Line::from(vec![
        prompt,
        Span::raw(input_text.clone()),
    ]);

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));

    let paragraph = Paragraph::new(line).block(block);
    frame.render_widget(paragraph, area);

    // Place cursor
    let prompt_width = if is_streaming { 9 } else { 2 };
    let cursor_x = area.x + prompt_width + app.input.cursor as u16;
    let cursor_y = area.y + 1; // +1 for border
    frame.set_cursor_position((cursor_x, cursor_y));
}
