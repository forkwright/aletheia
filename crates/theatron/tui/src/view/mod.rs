mod chat;
mod command_palette;
mod filter_bar;
pub(crate) mod image;
mod input;
mod memory;
pub(crate) mod notification;
pub(crate) mod ops;
mod overlay;
pub(crate) mod settings;
mod sidebar;
mod slash;
mod status_bar;
pub(crate) mod tab_bar;
mod title_bar;
mod views;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::App;
use crate::hyperlink::OscLink;

const SIDEBAR_WIDTH: u16 = 22;
const MIN_CHAT_WIDTH: u16 = 40;
const MIN_SIDEBAR_TERMINAL_WIDTH: u16 = 60;
const MIN_OPS_TERMINAL_WIDTH: u16 = 80;
const MAX_PALETTE_SUGGESTIONS: usize = 12;
const MAX_SLASH_SUGGESTIONS: usize = 8;
const STATUS_BAR_HEIGHT: u16 = 2;

#[tracing::instrument(skip_all)]
#[expect(
    clippy::indexing_slicing,
    reason = "Layout.split() returns exactly as many Rects as constraints; all accesses use matching fixed indices"
)]
pub(crate) fn render(app: &App, frame: &mut Frame) -> Vec<OscLink> {
    let area = frame.area();
    let theme = &app.theme;

    let has_banner = app.viewport.error_banner.is_some();
    let has_old_toast = app.viewport.error_toast.is_some() || app.viewport.success_toast.is_some();
    let has_new_toast = !app.viewport.toasts.is_empty();
    let palette_active = app.interaction.command_palette.active;
    let slash_active = app.interaction.slash_complete.active;
    let show_tabs = tab_bar::should_show(app);

    // NOTE: When palette or slash complete is active it replaces the status bar.
    let bottom_height = if palette_active {
        let suggestion_lines = app
            .interaction
            .command_palette
            .suggestions
            .len()
            .min(MAX_PALETTE_SUGGESTIONS);
        let suggestion_lines = u16::try_from(suggestion_lines).unwrap_or(u16::MAX);
        (STATUS_BAR_HEIGHT + suggestion_lines).max(3)
    } else if slash_active {
        let suggestion_lines = app
            .interaction
            .slash_complete
            .suggestions
            .len()
            .min(MAX_SLASH_SUGGESTIONS);
        let suggestion_lines = u16::try_from(suggestion_lines).unwrap_or(u16::MAX);
        (STATUS_BAR_HEIGHT + suggestion_lines).max(3)
    } else {
        STATUS_BAR_HEIGHT
    };

    let tab_height: u16 = if show_tabs { 1 } else { 0 };

    // Index offsets for the dynamic layout slots.
    let banner_offset: usize = if has_banner { 1 } else { 0 };
    let tab_offset: usize = if show_tabs { 1 } else { 0 };
    let title_idx: usize = 0;
    let body_idx: usize = 1 + banner_offset + tab_offset;
    let bottom_idx: usize = body_idx + 1;
    let old_toast_idx: usize = bottom_idx + 1;
    let new_toast_idx: usize = bottom_idx + 1 + usize::from(has_old_toast);
    // tab_idx is 1 + banner_offset; only used inside `if show_tabs`.
    let tab_idx: usize = 1 + banner_offset;

    let mut constraints = vec![Constraint::Length(1)]; // title
    if has_banner {
        constraints.push(Constraint::Length(1));
    }
    if show_tabs {
        constraints.push(Constraint::Length(tab_height));
    }
    constraints.push(Constraint::Min(5));
    constraints.push(Constraint::Length(bottom_height));
    if has_old_toast {
        constraints.push(Constraint::Length(1));
    }
    if has_new_toast {
        constraints.push(Constraint::Length(1));
    }

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    title_bar::render(app, frame, vertical[title_idx], theme);

    if has_banner {
        notification::render_banner(app, frame, vertical[1], theme);
    }

    if show_tabs {
        tab_bar::render(app, frame, vertical[tab_idx], theme);
    }

    if palette_active {
        command_palette::render(app, frame, vertical[bottom_idx], theme);
    } else if slash_active {
        slash::render(app, frame, vertical[bottom_idx], theme);
    } else {
        status_bar::render(app, frame, vertical[bottom_idx], theme);
    }

    if has_old_toast {
        if let Some(ref toast) = app.viewport.error_toast {
            let toast_line = ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(" \u{2717} ", theme.style_error_bold()),
                ratatui::text::Span::styled(&toast.message, theme.style_error()),
            ]);
            let toast_widget = ratatui::widgets::Paragraph::new(toast_line)
                .style(ratatui::style::Style::default().bg(theme.colors.surface_dim));
            frame.render_widget(toast_widget, vertical[old_toast_idx]);
        } else if let Some(ref toast) = app.viewport.success_toast {
            let toast_line = ratatui::text::Line::from(vec![
                ratatui::text::Span::styled(" \u{2713} ", theme.style_success_bold()),
                ratatui::text::Span::styled(&toast.message, theme.style_success_bold()),
            ]);
            let toast_widget = ratatui::widgets::Paragraph::new(toast_line)
                .style(ratatui::style::Style::default().bg(theme.colors.surface_dim));
            frame.render_widget(toast_widget, vertical[old_toast_idx]);
        }
    }

    if has_new_toast && let Some(toast) = app.viewport.toasts.last() {
        use crate::msg::NotificationKind;
        let (icon, icon_style) = match toast.kind {
            NotificationKind::Info => (" ℹ ", theme.style_fg()),
            NotificationKind::Warning => (
                " ⚠ ",
                ratatui::style::Style::default().fg(theme.status.warning),
            ),
            NotificationKind::Error => (" \u{2717} ", theme.style_error_bold()),
            NotificationKind::Success => (" \u{2713} ", theme.style_success_bold()),
        };
        let toast_line = ratatui::text::Line::from(vec![
            ratatui::text::Span::styled(icon, icon_style),
            ratatui::text::Span::styled(&toast.message, theme.style_fg()),
        ]);
        let toast_widget = ratatui::widgets::Paragraph::new(toast_line)
            .style(ratatui::style::Style::default().bg(theme.colors.surface_dim));
        frame.render_widget(toast_widget, vertical[new_toast_idx]);
    }

    let show_sidebar = app.layout.sidebar_visible && area.width >= MIN_SIDEBAR_TERMINAL_WIDTH;
    let show_ops = app.layout.ops.visible && area.width >= MIN_OPS_TERMINAL_WIDTH;

    let osc_links = if show_sidebar {
        let sidebar_and_rest = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(SIDEBAR_WIDTH),
                Constraint::Min(MIN_CHAT_WIDTH),
            ])
            .split(vertical[body_idx]);

        sidebar::render(app, frame, sidebar_and_rest[0], theme);
        SIDEBAR_RECT.store_rect(sidebar_and_rest[0]);

        if show_ops {
            let ops_width =
                ops::ops_pane_width(sidebar_and_rest[1].width, app.layout.ops.width_pct);
            let chat_ops = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(MIN_CHAT_WIDTH),
                    Constraint::Length(ops_width),
                ])
                .split(sidebar_and_rest[1]);

            let links = views::render_for_view(app, frame, chat_ops[0], theme);
            ops::render(app, frame, chat_ops[1], theme);
            links
        } else {
            views::render_for_view(app, frame, sidebar_and_rest[1], theme)
        }
    } else {
        SIDEBAR_RECT.store_rect(Rect::ZERO);

        if show_ops {
            let ops_width = ops::ops_pane_width(vertical[body_idx].width, app.layout.ops.width_pct);
            let chat_ops = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Min(MIN_CHAT_WIDTH),
                    Constraint::Length(ops_width),
                ])
                .split(vertical[body_idx]);

            let links = views::render_for_view(app, frame, chat_ops[0], theme);
            ops::render(app, frame, chat_ops[1], theme);
            links
        } else {
            views::render_for_view(app, frame, vertical[body_idx], theme)
        }
    };

    if app.layout.overlay.is_some() {
        overlay::render(app, frame, area, theme);
    }

    osc_links
}

/// Thread-safe sidebar rect storage for mouse click detection.
/// Updated each frame by render(), read by App::map_terminal().
pub(crate) struct SidebarRect {
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

    pub(crate) fn load_rect(&self) -> Rect {
        use std::sync::atomic::Ordering::Relaxed;
        Rect::new(
            self.x.load(Relaxed),
            self.y.load(Relaxed),
            self.w.load(Relaxed),
            self.h.load(Relaxed),
        )
    }
}

pub(crate) static SIDEBAR_RECT: SidebarRect = SidebarRect::new();

#[expect(
    clippy::indexing_slicing,
    reason = "Layout.split() returns exactly as many Rects as constraints; all accesses use matching fixed indices"
)]
fn render_chat_area(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    theme: &crate::theme::Theme,
) -> Vec<OscLink> {
    let prompt_len: usize = if app.connection.active_turn_id.is_some() {
        9
    } else {
        2
    };
    let content_width = usize::from(area.width.max(1));
    let first_line_avail = content_width.saturating_sub(prompt_len).max(1);
    let wrapped_lines =
        input::word_wrap_lines(&app.interaction.input.text, first_line_avail, content_width).len();
    let wrapped_lines = u16::try_from(wrapped_lines).unwrap_or(u16::MAX);
    let input_height = (wrapped_lines + 1).clamp(3, 8);

    let filter_height: u16 = if app.interaction.filter.editing { 1 } else { 0 };

    let available_body = area
        .height
        .saturating_sub(input_height)
        .saturating_sub(filter_height);

    // WHY: virtual scroll cache gives content height without re-rendering. When it's stale
    // or filter is active (filter bypasses virtual scroll), fall back to full-height so the
    // spacer calculation is never applied to a mismatched estimate.
    let wrap_width = area.width.saturating_sub(2).max(1);
    let cache_fresh = app.viewport.render.virtual_scroll.len() == app.dashboard.messages.len()
        && (app.dashboard.messages.is_empty()
            || app.viewport.render.virtual_scroll.cached_width() == wrap_width);
    let filter_active = app.interaction.filter.active && !app.interaction.filter.text.is_empty();

    let (spacer_height, messages_height) = if cache_fresh && !filter_active {
        // WHY: +1 for the top padding line chat::render always emits before message content.
        let content_rows = u16::try_from(app.viewport.render.virtual_scroll.total_height())
            .unwrap_or(u16::MAX)
            .saturating_add(1);
        if content_rows < available_body {
            (available_body - content_rows, content_rows.max(3))
        } else {
            (0, available_body.max(3))
        }
    } else {
        (0, available_body.max(3))
    };

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(spacer_height),
            Constraint::Length(messages_height),
            Constraint::Length(filter_height),
            Constraint::Length(input_height),
        ])
        .split(area);

    let osc_links = chat::render(app, frame, layout[1], theme);
    if app.interaction.filter.editing {
        filter_bar::render(app, frame, layout[2], theme);
    }
    input::render(app, frame, layout[3], theme);
    osc_links
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

    #[test]
    fn min_ops_terminal_width_is_80() {
        assert_eq!(MIN_OPS_TERMINAL_WIDTH, 80);
    }
}
