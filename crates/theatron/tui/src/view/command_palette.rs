use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthStr;

/// Fixed column width for command labels in the suggestion list.
const COMMAND_LABEL_DISPLAY_WIDTH: usize = 12;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::command::CommandCategory;
use crate::theme::Theme;

pub(crate) fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    if !app.interaction.command_palette.active || area.height < 2 {
        return;
    }

    let palette = &app.interaction.command_palette;
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(vec![
        Span::styled(
            ":",
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&palette.input, theme.style_fg()),
    ]));

    for (i, suggestion) in palette.suggestions.iter().enumerate() {
        let selected = i == palette.selected;
        let marker = if selected { "▸" } else { " " };

        let name_style = if selected {
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        let category_color = match suggestion.category {
            CommandCategory::Navigation => theme.colors.accent,
            CommandCategory::Action => theme.status.warning,
            CommandCategory::Query => theme.status.success,
            CommandCategory::Agent => theme.status.streaming,
        };

        let mut spans = vec![
            Span::styled(
                format!(" {} ", marker),
                if selected {
                    name_style
                } else {
                    theme.style_dim()
                },
            ),
            Span::styled("●", Style::default().fg(category_color)),
            Span::styled(
                format!(" {:<COMMAND_LABEL_DISPLAY_WIDTH$}", suggestion.label),
                name_style,
            ),
            Span::styled(&suggestion.description, theme.style_muted()),
        ];

        if let Some(shortcut) = suggestion.shortcut {
            spans.push(Span::styled(format!("  [{shortcut}]"), theme.style_dim()));
        }

        if !suggestion.aliases.is_empty() {
            let alias_str = suggestion.aliases.iter().map(|a| format!(":{a}")).fold(
                String::new(),
                |mut acc, s| {
                    if !acc.is_empty() {
                        acc.push(' ');
                    }
                    acc.push_str(&s);
                    acc
                },
            );
            spans.push(Span::styled(format!("  {alias_str}"), theme.style_dim()));
        }

        lines.push(Line::from(spans));
    }

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.borders.separator));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);

    let input_before_cursor = palette
        .input
        .get(..palette.cursor)
        .unwrap_or(&palette.input);
    let input_display_width = UnicodeWidthStr::width(input_before_cursor);
    let cursor_x = area.x + 1 + u16::try_from(input_display_width).unwrap_or(u16::MAX);
    let cursor_y = area.y + 1;
    frame.set_cursor_position((cursor_x, cursor_y));
}
