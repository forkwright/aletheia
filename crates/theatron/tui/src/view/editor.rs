//! File editor view: tree sidebar, tab bar, syntax-highlighted content, status line.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::App;
use crate::state::editor::{EditorState, GitFileStatus};
use crate::theme::Theme;

const TREE_WIDTH: u16 = 24;
const LINE_NUMBER_WIDTH: usize = 5;

#[expect(
    clippy::indexing_slicing,
    reason = "Layout.split() returns exactly as many Rects as constraints; all accesses use matching fixed indices"
)]
pub(crate) fn render(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let editor = &app.layout.editor;

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // tab bar
            Constraint::Min(3),    // content area
            Constraint::Length(1), // status line
        ])
        .split(area);

    render_tab_bar(editor, frame, layout[0], theme);

    if editor.tree_visible {
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(TREE_WIDTH), Constraint::Min(20)])
            .split(layout[1]);

        render_file_tree(editor, frame, body[0], theme);
        render_content(app, frame, body[1], theme);
    } else {
        render_content(app, frame, layout[1], theme);
    }

    render_status_line(editor, frame, layout[2], theme);

    if editor.confirm_delete.is_some() {
        render_delete_confirm(editor, frame, area, theme);
    } else if editor.rename_input.is_some() {
        render_modal_input(editor, frame, area, theme, "Rename:");
    } else if editor.new_file_input.is_some() {
        render_modal_input(editor, frame, area, theme, "New file:");
    }
}

fn render_tab_bar(editor: &EditorState, frame: &mut Frame, area: Rect, theme: &Theme) {
    if editor.tabs.is_empty() {
        let line = Line::from(Span::styled("  No files open", theme.style_dim()));
        frame.render_widget(
            Paragraph::new(line).style(Style::default().bg(theme.colors.surface_dim)),
            area,
        );
        return;
    }

    let mut spans: Vec<Span> = Vec::new();
    for (i, tab) in editor.tabs.iter().enumerate() {
        let is_active = i == editor.active_tab;
        let dirty_mark = if tab.dirty { "*" } else { "" };
        let label = format!(" {}{dirty_mark} ", tab.file_name());

        let style = if is_active {
            theme
                .style_accent()
                .add_modifier(Modifier::BOLD)
                .bg(theme.colors.surface)
        } else {
            theme.style_dim().bg(theme.colors.surface_dim)
        };
        spans.push(Span::styled(label, style));
        spans.push(Span::styled(
            " ",
            Style::default().bg(theme.colors.surface_dim),
        ));
    }

    let line = Line::from(spans);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme.colors.surface_dim)),
        area,
    );
}

fn render_file_tree(editor: &EditorState, frame: &mut Frame, area: Rect, theme: &Theme) {
    let tree = &editor.tree;

    let border_style = if editor.tree_focused {
        Style::default().fg(theme.borders.selected)
    } else {
        Style::default().fg(theme.borders.normal)
    };

    let block = Block::default()
        .title(" Files ")
        .borders(Borders::RIGHT)
        .border_style(border_style);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    let start = tree.scroll_offset;
    let end = (start + usize::from(inner.height)).min(tree.entries.len());

    for i in start..end {
        if let Some(entry) = tree.entries.get(i) {
            let is_selected = i == tree.selected;
            let indent = "  ".repeat(entry.depth);

            let icon = if entry.is_dir {
                if editor.tree.expanded.contains(&entry.path) {
                    "\u{25be} "
                } else {
                    "\u{25b8} "
                }
            } else {
                "  "
            };

            let name_style = if is_selected && editor.tree_focused {
                theme
                    .style_fg()
                    .add_modifier(Modifier::BOLD)
                    .bg(theme.colors.surface)
            } else if is_selected {
                theme.style_fg().bg(theme.colors.surface_dim)
            } else if entry.is_dir {
                theme.style_accent()
            } else {
                theme.style_fg()
            };

            let marker = if is_selected { "\u{25b8}" } else { " " };
            let marker_style = if is_selected && editor.tree_focused {
                Style::default().fg(theme.borders.selected)
            } else {
                theme.style_dim()
            };

            let mut spans = vec![
                Span::styled(marker, marker_style),
                Span::styled(indent, theme.style_fg()),
                Span::styled(icon, theme.style_dim()),
                Span::styled(&entry.name, name_style),
            ];

            if let Some(ref status) = entry.git_status {
                let badge_color = match status {
                    GitFileStatus::Modified => theme.status.warning,
                    GitFileStatus::Added | GitFileStatus::Untracked => theme.status.success,
                    GitFileStatus::Deleted => theme.status.error,
                    GitFileStatus::Renamed => theme.text.fg_dim,
                };
                spans.push(Span::styled(
                    format!(" {}", status.badge()),
                    Style::default().fg(badge_color),
                ));
            }

            lines.push(Line::from(spans));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn render_content(app: &App, frame: &mut Frame, area: Rect, theme: &Theme) {
    let editor = &app.layout.editor;

    let Some(tab) = editor.active_tab() else {
        let msg = Line::from(vec![
            Span::raw("  "),
            Span::styled("Select a file from the tree to edit", theme.style_dim()),
        ]);
        let block = Block::default()
            .borders(Borders::NONE)
            .style(Style::default().bg(theme.colors.surface));
        frame.render_widget(Paragraph::new(msg).block(block), area);
        return;
    };

    let viewport_height = usize::from(area.height);
    let start_row = tab.scroll_row;
    let end_row = (start_row + viewport_height).min(tab.content.len());

    let code_to_highlight: String = tab.content[start_row..end_row].join("\n");
    let highlighted = app.highlighter.highlight(&code_to_highlight, &tab.language);

    let mut lines: Vec<Line> = Vec::new();
    for (i, hl_line) in highlighted.into_iter().enumerate() {
        let line_num = start_row + i + 1;
        let is_cursor_line = start_row + i == tab.cursor_row;

        let num_style = if is_cursor_line {
            theme.style_accent().add_modifier(Modifier::BOLD)
        } else {
            theme.style_dim()
        };

        let mut spans = vec![Span::styled(
            format!("{line_num:>width$} ", width = LINE_NUMBER_WIDTH - 1),
            num_style,
        )];
        spans.extend(hl_line.spans);
        lines.push(Line::from(spans));
    }

    let border_style = if !editor.tree_focused {
        Style::default().fg(theme.borders.selected)
    } else {
        Style::default().fg(theme.borders.normal)
    };

    let block = Block::default()
        .borders(Borders::NONE)
        .border_style(border_style);

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_status_line(editor: &EditorState, frame: &mut Frame, area: Rect, theme: &Theme) {
    let mut spans: Vec<Span> = vec![Span::styled(" ", theme.style_fg())];

    if let Some(tab) = editor.active_tab() {
        spans.push(Span::styled(
            format!("Ln {}, Col {} ", tab.cursor_row + 1, tab.cursor_col + 1),
            theme.style_fg(),
        ));
        spans.push(Span::styled("\u{2502} ", theme.style_dim()));
        spans.push(Span::styled(&tab.language, theme.style_accent()));
        spans.push(Span::styled(" \u{2502} ", theme.style_dim()));

        if tab.dirty {
            spans.push(Span::styled(
                "Modified ",
                Style::default().fg(theme.status.warning),
            ));
        } else {
            spans.push(Span::styled("Saved ", theme.style_dim()));
        }

        spans.push(Span::styled("\u{2502} ", theme.style_dim()));
        spans.push(Span::styled(
            format!("{}", tab.path.display()),
            theme.style_dim(),
        ));
    } else {
        spans.push(Span::styled("No file open", theme.style_dim()));
    }

    let line = Line::from(spans);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme.colors.surface_dim)),
        area,
    );
}

fn render_delete_confirm(editor: &EditorState, frame: &mut Frame, area: Rect, theme: &Theme) {
    let path_display = editor
        .confirm_delete
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_default();

    let lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Delete ", theme.style_error()),
            Span::styled(&path_display, theme.style_fg().add_modifier(Modifier::BOLD)),
            Span::styled("?", theme.style_error()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("y", theme.style_accent()),
            Span::styled(" confirm  ", theme.style_dim()),
            Span::styled("n/Esc", theme.style_accent()),
            Span::styled(" cancel", theme.style_dim()),
        ]),
    ];

    let width = 50u16.min(area.width.saturating_sub(4));
    let height = 6u16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    frame.render_widget(ratatui::widgets::Clear, popup_area);
    let block = Block::default()
        .title(" Confirm Delete ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.status.error));
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, popup_area);
}

fn render_modal_input(
    editor: &EditorState,
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    label: &str,
) {
    let input_text = editor
        .rename_input
        .as_deref()
        .or(editor.new_file_input.as_deref())
        .unwrap_or("");

    let lines = vec![
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(label, theme.style_fg().add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(input_text, theme.style_accent()),
            Span::styled("\u{2588}", theme.style_accent()),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Enter", theme.style_accent()),
            Span::styled(" confirm  ", theme.style_dim()),
            Span::styled("Esc", theme.style_accent()),
            Span::styled(" cancel", theme.style_dim()),
        ]),
    ];

    let width = 50u16.min(area.width.saturating_sub(4));
    let height = 6u16.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup_area = Rect::new(x, y, width, height);

    frame.render_widget(ratatui::widgets::Clear, popup_area);
    let block = Block::default()
        .title(format!(" {label} "))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.borders.selected));
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, popup_area);
}
