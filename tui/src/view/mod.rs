mod chat;
mod command_palette;
mod filter_bar;
mod input;
mod overlay;
pub(crate) mod settings;
mod sidebar;
mod status_bar;
mod title_bar;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::App;

const SIDEBAR_WIDTH: u16 = 22;
const MIN_CHAT_WIDTH: u16 = 40;
const MIN_SIDEBAR_TERMINAL_WIDTH: u16 = 60;
const MAX_PALETTE_SUGGESTIONS: usize = 12;
const STATUS_BAR_HEIGHT: u16 = 2;

pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let theme = &app.theme;

    let has_toast = app.error_toast.is_some();
    let palette_active = app.command_palette.active;

    // When palette is active, it replaces the status bar with a variable-height area
    let bottom_height = if palette_active {
        let suggestion_lines = app.command_palette.suggestions.len().min(MAX_PALETTE_SUGGESTIONS) as u16;
        (STATUS_BAR_HEIGHT + suggestion_lines).max(3) // input + border + suggestions, min 3
    } else {
        STATUS_BAR_HEIGHT
    };

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if has_toast {
            vec![
                Constraint::Length(1),             // title bar
                Constraint::Min(5),                // body
                Constraint::Length(bottom_height), // status bar or command palette
                Constraint::Length(1),             // error toast
            ]
        } else {
            vec![
                Constraint::Length(1),             // title bar
                Constraint::Min(5),                // body
                Constraint::Length(bottom_height), // status bar or command palette
            ]
        })
        .split(area);

    title_bar::render(app, frame, vertical[0], theme);

    // Bottom area: command palette or status bar
    if palette_active {
        command_palette::render(app, frame, vertical[2], theme);
    } else {
        status_bar::render(app, frame, vertical[2], theme);
    }

    // Error toast at bottom
    if has_toast && let Some(ref toast) = app.error_toast {
        let toast_line = ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(" ✗ ", theme.style_error_bold()),
            ratatui::text::Span::styled(&toast.message, theme.style_error()),
        ]);
        let toast_widget = ratatui::widgets::Paragraph::new(toast_line)
            .style(ratatui::style::Style::default().bg(theme.surface_dim));
        frame.render_widget(toast_widget, vertical[3]);
    }

    // Responsive: hide sidebar on narrow terminals
    let show_sidebar = app.sidebar_visible && area.width >= MIN_SIDEBAR_TERMINAL_WIDTH;

    // Body: sidebar | chat area
    if show_sidebar {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(SIDEBAR_WIDTH),
                Constraint::Min(MIN_CHAT_WIDTH),
            ])
            .split(vertical[1]);

        sidebar::render(app, frame, horizontal[0], theme);
        render_chat_area(app, frame, horizontal[1], theme);

        SIDEBAR_RECT.store_rect(horizontal[0]);
    } else {
        SIDEBAR_RECT.store_rect(Rect::ZERO);
        render_chat_area(app, frame, vertical[1], theme);
    }

    // Render overlay on top if present
    if app.overlay.is_some() {
        overlay::render(app, frame, area, theme);
    }
}

/// Thread-safe sidebar rect storage for mouse click detection.
/// Updated each frame by render(), read by App::map_terminal().
pub struct SidebarRect {
    x: std::sync::atomic::AtomicU16,
    y: std::sync::atomic::AtomicU16,
    w: std::sync::atomic::AtomicU16,
    h: std::sync::atomic::AtomicU16,
}

impl SidebarRect {
    const fn new() -> Self {
        Self {
            x: std::sync::atomic::AtomicU16::new(0),
            y: std::sync::atomic::AtomicU16::new(0),
            w: std::sync::atomic::AtomicU16::new(0),
            h: std::sync::atomic::AtomicU16::new(0),
        }
    }

    fn store_rect(&self, r: Rect) {
        use std::sync::atomic::Ordering::Relaxed;
        self.x.store(r.x, Relaxed);
        self.y.store(r.y, Relaxed);
        self.w.store(r.width, Relaxed);
        self.h.store(r.height, Relaxed);
    }

    pub fn load_rect(&self) -> Rect {
        use std::sync::atomic::Ordering::Relaxed;
        Rect::new(
            self.x.load(Relaxed),
            self.y.load(Relaxed),
            self.w.load(Relaxed),
            self.h.load(Relaxed),
        )
    }
}

pub static SIDEBAR_RECT: SidebarRect = SidebarRect::new();

fn render_chat_area(app: &App, frame: &mut Frame, area: Rect, theme: &crate::theme::ThemePalette) {
    // Dynamically size input area based on text length (wrapping)
    let prompt_len: u16 = if app.active_turn_id.is_some() { 9 } else { 2 };
    let text_len = app.input.text.len() as u16 + prompt_len;
    let content_width = area.width.max(1);
    let wrapped_lines = (text_len / content_width) + 1;
    let input_height = (wrapped_lines + 1).clamp(3, 8); // +1 for border, min 3, max 8

    let filter_height: u16 = if app.filter.editing { 1 } else { 0 };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),                // messages
            Constraint::Length(filter_height), // filter bar (when editing)
            Constraint::Length(input_height),  // input (grows with text)
        ])
        .split(area);

    chat::render(app, frame, layout[0], theme);
    if app.filter.editing {
        filter_bar::render(app, frame, layout[1], theme);
    }
    input::render(app, frame, layout[2], theme);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_rect_store_load_roundtrip() {
        let rect = SidebarRect::new();
        let r = Rect::new(5, 10, 22, 40);
        rect.store_rect(r);
        let loaded = rect.load_rect();
        assert_eq!(loaded, r);
    }

    #[test]
    fn sidebar_rect_initial_is_zero() {
        let rect = SidebarRect::new();
        let loaded = rect.load_rect();
        assert_eq!(loaded, Rect::ZERO);
    }
}
