use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::App;
use crate::state::{SearchResultKind, SessionSearchOverlay};
use crate::theme::Theme;

use super::overlay_block;

pub(super) fn render_agent_picker(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    cursor: usize,
    theme: &Theme,
) {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    for (i, agent) in app.dashboard.agents.iter().enumerate() {
        let selected = i == cursor;
        let marker = if selected { "▸" } else { " " };
        let emoji = agent.emoji.as_deref().unwrap_or("");

        let style = if selected {
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        lines.push(Line::from(vec![
            Span::raw(format!("  {} ", marker)),
            Span::styled(format!("{} {} ", emoji, agent.name), style),
            Span::styled(format!("({})", agent.id), theme.style_dim()),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Enter",
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" select  ", theme.style_muted()),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.text.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", theme.style_muted()),
    ]));

    let block = overlay_block("Switch Agent", theme);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

pub(super) fn render_session_picker(
    app: &App,
    frame: &mut Frame,
    area: Rect,
    picker: &crate::state::SessionPickerOverlay,
    theme: &Theme,
) {
    let agent_id = app.dashboard.focused_agent.as_ref();
    let agent = agent_id.and_then(|id| app.dashboard.agents.iter().find(|a| &a.id == id));

    let sessions: Vec<_> = match agent {
        Some(a) => {
            if picker.show_archived {
                a.sessions.iter().collect()
            } else {
                a.sessions.iter().filter(|s| s.is_interactive()).collect()
            }
        }
        None => Vec::new(),
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));

    if sessions.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No sessions found",
            theme.style_muted(),
        )));
    }

    for (i, session) in sessions.iter().enumerate() {
        let selected = i == picker.cursor;
        let marker = if selected { "▸" } else { " " };
        let is_current = app.dashboard.focused_session_id.as_ref() == Some(&session.id);

        let style = if selected {
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD)
        } else if is_current {
            Style::default().fg(theme.colors.accent)
        } else {
            theme.style_fg()
        };

        let label = session.label();
        let archived_tag = if session.is_archived() {
            " [archived]"
        } else {
            ""
        };

        lines.push(Line::from(vec![
            Span::raw(format!("  {} ", marker)),
            Span::styled(label, style),
            Span::styled(archived_tag, theme.style_dim()),
            Span::styled(
                format!("  ({} msgs)", session.message_count),
                theme.style_dim(),
            ),
        ]));
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            "Enter",
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" switch  ", theme.style_muted()),
        Span::styled(
            "n",
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("ew  ", theme.style_muted()),
        Span::styled(
            "d",
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("elete  ", theme.style_muted()),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.text.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", theme.style_muted()),
    ]));

    let agent_name = agent.map(|a| a.name.as_str()).unwrap_or("?");
    let title = if picker.show_archived {
        format!("Sessions — {} (all)", agent_name)
    } else {
        format!("Sessions — {}", agent_name)
    };
    let block = overlay_block(&title, theme);
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

pub(super) fn render_session_search(
    frame: &mut Frame,
    area: Rect,
    search: &SessionSearchOverlay,
    theme: &Theme,
) {
    let key_style = Style::default()
        .fg(theme.colors.accent)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("  / ", key_style),
        Span::raw(&search.query),
        Span::styled("_", theme.style_dim()),
    ]));
    lines.push(Line::raw(""));

    if search.results.is_empty() && !search.query.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No results",
            theme.style_muted(),
        )));
    }

    let visible_height = usize::from(area.height.saturating_sub(6));
    let start = if search.selected >= visible_height {
        search.selected - visible_height + 1
    } else {
        0
    };

    for (i, result) in search
        .results
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_height)
    {
        let selected = i == search.selected;
        let marker = if selected { "▸" } else { " " };

        let style = if selected {
            Style::default()
                .fg(theme.colors.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            theme.style_fg()
        };

        let kind_tag = match &result.kind {
            SearchResultKind::SessionName => Span::styled(" [session]", theme.style_dim()),
            SearchResultKind::MessageContent { role } => {
                Span::styled(format!(" [{role}]"), theme.style_dim())
            }
        };

        lines.push(Line::from(vec![
            Span::raw(format!("  {} ", marker)),
            Span::styled(&result.session_label, style),
            kind_tag,
            Span::styled(format!("  {}", result.agent_name), theme.style_muted()),
        ]));

        if !result.snippet.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("      {}", result.snippet),
                theme.style_dim(),
            )));
        }
    }

    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("Enter", key_style),
        Span::styled(" switch  ", theme.style_muted()),
        Span::styled(
            "Esc",
            Style::default()
                .fg(theme.text.fg_dim)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", theme.style_muted()),
    ]));

    let block = overlay_block("Search Sessions", theme);
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}
