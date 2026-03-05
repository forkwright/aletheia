use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::App;
use crate::command::CommandCategory;
use crate::theme::ThemePalette;

pub fn render(app: &App, frame: &mut Frame, area: Rect, theme: &ThemePalette) {
    if !app.command_palette.active || area.height < 2 {
        return;
    }

    let palette = &app.command_palette;
    let mut lines: Vec<Line> = Vec::new();

    // Input line: `:` prompt + typed text
    lines.push(Line::from(vec![
        Span::styled(
            ":",
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&palette.input, theme.style_fg()),
    ]));

    // Suggestion lines (max 8)
    for (i, suggestion) in palette.suggestions.iter().enumerate() {
        let selected = i == palette.selected;
        let marker = if selected { "▸" } else { " " };

        let name_style = if selected {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        let category_color = match suggestion.category {
            CommandCategory::Navigation => theme.accent,
            CommandCategory::Action => theme.warning,
            CommandCategory::Query => theme.success,
            CommandCategory::Agent => theme.streaming,
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
            Span::styled(format!(" {:<12}", suggestion.label), name_style),
            Span::styled(&suggestion.description, theme.style_muted()),
        ];

        if let Some(shortcut) = suggestion.shortcut {
            spans.push(Span::styled(format!("  [{shortcut}]"), theme.style_dim()));
        }

        if !suggestion.aliases.is_empty() {
            let alias_str = suggestion
                .aliases
                .iter()
                .map(|a| format!(":{a}"))
                .collect::<Vec<_>>()
                .join(" ");
            spans.push(Span::styled(format!("  {alias_str}"), theme.style_dim()));
        }

        lines.push(Line::from(spans));
    }

    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.separator));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);

    // Position cursor in the input area
    let cursor_x = area.x + 1 + palette.cursor as u16;
    let cursor_y = area.y + 1; // +1 for top border
    frame.set_cursor_position((cursor_x, cursor_y));
}
