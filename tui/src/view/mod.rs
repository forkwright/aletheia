mod chat;
mod input;
mod overlay;
mod sidebar;
mod status_bar;
mod title_bar;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::app::App;

pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();

    // Top-level vertical split: title bar | body | status bar
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // title bar
            Constraint::Min(5),    // body
            Constraint::Length(1), // status bar
        ])
        .split(area);

    title_bar::render(app, frame, vertical[0]);
    status_bar::render(app, frame, vertical[2]);

    // Body: sidebar | chat area
    if app.sidebar_visible {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(20), // sidebar
                Constraint::Min(40),   // chat
            ])
            .split(vertical[1]);

        sidebar::render(app, frame, horizontal[0]);
        render_chat_area(app, frame, horizontal[1]);
    } else {
        render_chat_area(app, frame, vertical[1]);
    }

    // Render overlay on top if present
    if app.overlay.is_some() {
        overlay::render(app, frame, area);
    }
}

fn render_chat_area(app: &App, frame: &mut Frame, area: Rect) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // messages
            Constraint::Length(3), // input
        ])
        .split(area);

    chat::render(app, frame, layout[0]);
    input::render(app, frame, layout[1]);
}
