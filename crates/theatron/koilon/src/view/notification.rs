/// Rendering for error banners, toast stack, and notification history overlay.
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::App;
use crate::msg::NotificationKind;
use crate::theme::Theme;

/// Height of the notification history popup as a percentage of the screen.
const NOTIF_POPUP_HEIGHT_PCT: u16 = 70;
/// Width of the notification history popup as a percentage of the screen.
const NOTIF_POPUP_WIDTH_PCT: u16 = 60;

/// Render the persistent error banner at the top of the viewport.
pub(crate) fn render_banner(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let banner = match &app.viewport.error_banner {
        Some(b) => b,
        None => return,
    };
    let line = Line::from(vec![
        Span::styled(" ⚠ ", theme.style_error_bold()),
        Span::styled(&banner.message, theme.style_error()),
        Span::styled(
            "  [Ctrl+D dismiss]",
            Style::default()
                .fg(theme.text.fg_dim)
                .add_modifier(Modifier::DIM),
        ),
    ]);
    let widget = Paragraph::new(line).style(Style::default().bg(theme.colors.surface_dim));
    frame.render_widget(widget, area);
}

/// Render the notification history overlay.
pub(crate) fn render_history(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    scroll: usize,
    theme: &Theme,
) {
    let popup_area =
        super::overlay::centered_rect_pub(NOTIF_POPUP_WIDTH_PCT, NOTIF_POPUP_HEIGHT_PCT, area);
    frame.render_widget(Clear, popup_area);

    let unread = app.layout.notifications.unread_count();
    let title = if unread == 0 {
        "Notifications".to_string()
    } else {
        format!("Notifications — {} unread", unread)
    };

    let mut lines: Vec<Line> = vec![Line::raw("")];

    if app.layout.notifications.items.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No notifications yet",
            theme.style_muted(),
        )));
    }

    let items: Vec<_> = app.layout.notifications.items.iter().rev().collect();
    let visible_height = usize::from(popup_area.height.saturating_sub(5));
    let start = scroll.min(items.len().saturating_sub(1));

    for notif in items.iter().skip(start).take(visible_height) {
        let (icon, icon_style) = kind_icon(notif.kind, theme);
        let read_marker = if notif.read { " " } else { "●" };
        let read_style = if notif.read {
            theme.style_dim()
        } else {
            Style::default().fg(theme.colors.accent)
        };

        let agent_label = notif
            .nous_id
            .as_ref()
            .map(|id| format!(" [{}]", id))
            .unwrap_or_default();

        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(read_marker, read_style),
            Span::raw(" "),
            Span::styled(icon, icon_style),
            Span::raw(" "),
            Span::styled(&notif.message, theme.style_fg()),
            Span::styled(agent_label, theme.style_dim()),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "↑↓",
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" scroll  ", theme.style_muted()),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.text.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" close", theme.style_muted()),
    ]));

    let block = Block::default()
        .title(format!(" {} ", title.trim()))
        .title_style(theme.style_accent_bold())
        .borders(Borders::ALL)
        .border_set(symbols::border::ROUNDED)
        .border_style(theme.style_border())
        .style(Style::default().bg(theme.colors.surface));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, popup_area);
}

fn kind_icon(kind: NotificationKind, theme: &Theme) -> (&'static str, Style) {
    match kind {
        NotificationKind::Info => ("ℹ", theme.style_fg()),
        NotificationKind::Warning => ("⚠", Style::default().fg(theme.status.warning)),
        NotificationKind::Error => ("✗", theme.style_error()),
        NotificationKind::Success => ("✓", theme.style_success()),
    }
}
