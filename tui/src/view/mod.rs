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

    // Top-level vertical split: title bar | body | status bar | (optional toast)
    let has_toast = app.error_toast.is_some();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if has_toast {
            vec![
                Constraint::Length(1), // title bar
                Constraint::Min(5),    // body
                Constraint::Length(2), // status bar
                Constraint::Length(1), // error toast
            ]
        } else {
            vec![
                Constraint::Length(1), // title bar
                Constraint::Min(5),    // body
                Constraint::Length(2), // status bar
            ]
        })
        .split(area);

    title_bar::render(app, frame, vertical[0], theme);
    status_bar::render(app, frame, vertical[2], theme);

    // Error toast at bottom
    if has_toast {
        if let Some(ref toast) = app.error_toast {
            let toast_line = ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(" ✗ ", theme.style_error_bold()),
                ratatui::text::Span::styled(&toast.message, theme.style_error()),
            ]);
            let toast_widget = ratatui::widgets::Paragraph::new(toast_line)
                .style(ratatui::style::Style::default().bg(theme.surface_dim));
            frame.render_widget(toast_widget, vertical[3]);
        }
    }

    // Responsive: hide sidebar on narrow terminals (< 60 cols)
    let show_sidebar = app.sidebar_visible && area.width >= 60;

    // Body: sidebar | chat area
    if show_sidebar {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(22), // sidebar (slightly wider for padding)
                Constraint::Min(40),    // chat
            ])
            .split(vertical[1]);

        sidebar::render(app, frame, horizontal[0], theme);
        render_chat_area(app, frame, horizontal[1], theme);

        // Store sidebar area for mouse click detection (mutable through interior pattern)
        // We can't mutate app here since view takes &self, so store in a separate mechanism
        // Actually we'll update sidebar_area before render in the event loop
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
