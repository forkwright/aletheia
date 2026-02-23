mod chat;
mod input;
mod overlay;
mod sidebar;
mod status_bar;
mod title_bar;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::App;

pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let theme = &app.theme;

    // Top-level vertical split: title bar | body | status bar
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // title bar
            Constraint::Min(5),    // body
            Constraint::Length(1), // status bar
        ])
        .split(area);

    title_bar::render(app, frame, vertical[0], theme);
    status_bar::render(app, frame, vertical[2], theme);

    // Body: sidebar | chat area
    if app.sidebar_visible {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(22), // sidebar (slightly wider for padding)
                Constraint::Min(40),    // chat
            ])
            .split(vertical[1]);

        sidebar::render(app, frame, horizontal[0], theme);
        render_chat_area(app, frame, horizontal[1], theme);
    } else {
        render_chat_area(app, frame, vertical[1], theme);
    }

    // Render overlay on top if present
    if app.overlay.is_some() {
        overlay::render(app, frame, area, theme);
    }
}

fn render_chat_area(app: &App, frame: &mut Frame, area: Rect, theme: &crate::theme::ThemePalette) {
    // Dynamically size input area based on text length (wrapping)
    let prompt_len: u16 = if app.active_turn_id.is_some() { 9 } else { 2 };
    let text_len = app.input.text.len() as u16 + prompt_len;
    let content_width = area.width.saturating_sub(1).max(1);
    let wrapped_lines = (text_len / content_width) + 1;
    let input_height = (wrapped_lines + 1).clamp(3, 8); // +1 for border, min 3, max 8

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),               // messages
            Constraint::Length(input_height), // input (grows with text)
        ])
        .split(area);

    chat::render(app, frame, layout[0], theme);
    input::render(app, frame, layout[1], theme);
}
