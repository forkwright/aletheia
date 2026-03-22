/// Slash-command autocomplete dropdown rendering.
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::theme::Theme;

/// Fixed display column width for command name labels.
const NAME_DISPLAY_WIDTH: usize = 14;

pub(crate) fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    if !app.interaction.slash_complete.active || area.height < 2 {
        return;
    }

    let slash = &app.interaction.slash_complete;
    let mut lines: Vec<Line> = Vec::new();

    // Input line: "/query_"
    lines.push(Line::from(vec![
        Span::styled(
            "/",
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&slash.query, theme.style_fg()),
    ]));

    for (i, suggestion) in slash.suggestions.iter().enumerate() {
        let selected = i == slash.cursor;
        let marker = if selected { "▸" } else { " " };

        let name_style = if selected {
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        let marker_style = if selected {
            name_style
        } else {
            theme.style_dim()
        };

        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", marker), marker_style),
            Span::styled(
                format!("{:<NAME_DISPLAY_WIDTH$}", suggestion.name),
                name_style,
            ),
            Span::styled(&suggestion.description, theme.style_muted()),
        ]));
    }

    if slash.suggestions.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No matching commands",
            theme.style_dim(),
        )));
    }

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.borders.separator));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);

    let cursor_x = area.x + 1 + u16::try_from(slash.query.len()).unwrap_or(u16::MAX);
    let cursor_y = area.y + 1;
    frame.set_cursor_position((cursor_x, cursor_y));
}
