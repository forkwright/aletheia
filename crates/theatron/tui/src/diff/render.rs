//! Diff rendering: unified, side-by-side, word-level, and view rendering.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use similar::{ChangeTag, TextDiff};

use crate::theme::Theme;

use super::parse::{pad_to, truncate_str};
use super::types::{DiffChange, DiffMode, DiffViewState, FileDiff, collapse_to_replacements};

pub(crate) fn render_unified(file: &FileDiff, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::from(vec![Span::styled(
        format!("--- a/{}", file.path),
        theme.style_error().add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![Span::styled(
        format!("+++ b/{}", file.path),
        Style::default()
            .fg(theme.status.success)
            .add_modifier(Modifier::BOLD),
    )]));

    let mut old_line: usize;
    let mut new_line: usize;

    for hunk in &file.hunks {
        old_line = hunk.old_start;
        new_line = hunk.new_start;

        let old_count = hunk
            .changes
            .iter()
            .filter(|c| matches!(c, DiffChange::Equal(_) | DiffChange::Delete(_)))
            .count();
        let new_count = hunk
            .changes
            .iter()
            .filter(|c| matches!(c, DiffChange::Equal(_) | DiffChange::Insert(_)))
            .count();

        lines.push(Line::from(vec![Span::styled(
            format!(
                "@@ -{},{} +{},{} @@",
                hunk.old_start, old_count, hunk.new_start, new_count
            ),
            Style::default().fg(theme.status.info),
        )]));

        for change in &hunk.changes {
            match change {
                DiffChange::Equal(text) => {
                    let display = text.trim_end_matches('\n');
                    lines.push(Line::from(vec![
                        Span::styled(format!("{old_line:>4} {new_line:>4} "), theme.style_dim()),
                        Span::styled(format!(" {display}"), theme.style_dim()),
                    ]));
                    old_line += 1;
                    new_line += 1;
                }
                DiffChange::Delete(text) => {
                    let display = text.trim_end_matches('\n');
                    lines.push(Line::from(vec![
                        Span::styled(format!("{old_line:>4}      "), theme.style_dim()),
                        Span::styled(
                            format!("-{display}"),
                            Style::default().fg(theme.status.error),
                        ),
                    ]));
                    old_line += 1;
                }
                DiffChange::Insert(text) => {
                    let display = text.trim_end_matches('\n');
                    lines.push(Line::from(vec![
                        Span::styled(format!("     {new_line:>4} "), theme.style_dim()),
                        Span::styled(
                            format!("+{display}"),
                            Style::default().fg(theme.status.success),
                        ),
                    ]));
                    new_line += 1;
                }
                DiffChange::Replace { old, new } => {
                    let old_disp = old.trim_end_matches('\n');
                    let new_disp = new.trim_end_matches('\n');
                    lines.push(Line::from(vec![
                        Span::styled(format!("{old_line:>4}      "), theme.style_dim()),
                        Span::styled(
                            format!("-{old_disp}"),
                            Style::default().fg(theme.status.error),
                        ),
                    ]));
                    old_line += 1;
                    lines.push(Line::from(vec![
                        Span::styled(format!("     {new_line:>4} "), theme.style_dim()),
                        Span::styled(
                            format!("+{new_disp}"),
                            Style::default().fg(theme.status.success),
                        ),
                    ]));
                    new_line += 1;
                }
            }
        }
    }

    lines
}

/// Render hunks in a side-by-side layout.
pub(crate) fn render_side_by_side(
    file: &FileDiff,
    width: u16,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let half_width = (width as usize) / 2;
    // NOTE: 6-char line-number gutter "NNNN "
    let gutter = 6;
    let content_width = half_width.saturating_sub(gutter + 2);

    let header = format!("  {} ", file.path);
    lines.push(Line::from(vec![Span::styled(
        header,
        theme.style_accent().add_modifier(Modifier::BOLD),
    )]));

    lines.push(Line::from(vec![
        Span::styled(format!("{:^half_width$}", "Old"), theme.style_dim()),
        Span::styled(format!("{:^half_width$}", "New"), theme.style_dim()),
    ]));

    let separator_line = Line::from(vec![Span::styled(
        "─".repeat(width as usize),
        theme.style_dim(),
    )]);
    lines.push(separator_line);

    let mut old_line: usize;
    let mut new_line: usize;

    for hunk in &file.hunks {
        old_line = hunk.old_start;
        new_line = hunk.new_start;

        lines.push(Line::from(vec![Span::styled(
            format!(
                "@@ -{},{} +{},{} @@",
                hunk.old_start,
                hunk.changes
                    .iter()
                    .filter(|c| matches!(c, DiffChange::Equal(_) | DiffChange::Delete(_)))
                    .count(),
                hunk.new_start,
                hunk.changes
                    .iter()
                    .filter(|c| matches!(c, DiffChange::Equal(_) | DiffChange::Insert(_)))
                    .count(),
            ),
            Style::default().fg(theme.status.info),
        )]));

        for change in &hunk.changes {
            match change {
                DiffChange::Equal(text) => {
                    let display = text.trim_end_matches('\n');
                    let truncated = truncate_str(display, content_width);
                    let left = format!("{old_line:>4} {truncated:<content_width$} ");
                    let right = format!("{new_line:>4} {truncated:<content_width$}");
                    lines.push(Line::from(vec![
                        Span::styled(pad_to(left, half_width), theme.style_dim()),
                        Span::styled("│", theme.style_dim()),
                        Span::styled(right, theme.style_dim()),
                    ]));
                    old_line += 1;
                    new_line += 1;
                }
                DiffChange::Delete(text) => {
                    let display = text.trim_end_matches('\n');
                    let truncated = truncate_str(display, content_width);
                    let left = format!("{old_line:>4} {truncated:<content_width$} ");
                    let right = format!("{:>4} {:<content_width$}", "", "");
                    lines.push(Line::from(vec![
                        Span::styled(
                            pad_to(left, half_width),
                            Style::default().fg(theme.status.error),
                        ),
                        Span::styled("│", theme.style_dim()),
                        Span::styled(right, theme.style_dim()),
                    ]));
                    old_line += 1;
                }
                DiffChange::Insert(text) => {
                    let display = text.trim_end_matches('\n');
                    let truncated = truncate_str(display, content_width);
                    let left = format!("{:>4} {:<content_width$} ", "", "");
                    let right = format!("{new_line:>4} {truncated:<content_width$}");
                    lines.push(Line::from(vec![
                        Span::styled(pad_to(left, half_width), theme.style_dim()),
                        Span::styled("│", theme.style_dim()),
                        Span::styled(right, Style::default().fg(theme.status.success)),
                    ]));
                    new_line += 1;
                }
                DiffChange::Replace { old, new } => {
                    let old_disp = truncate_str(old.trim_end_matches('\n'), content_width);
                    let new_disp = truncate_str(new.trim_end_matches('\n'), content_width);
                    let left = format!("{old_line:>4} {old_disp:<content_width$} ");
                    let right = format!("{new_line:>4} {new_disp:<content_width$}");
                    lines.push(Line::from(vec![
                        Span::styled(
                            pad_to(left, half_width),
                            Style::default().fg(theme.status.error),
                        ),
                        Span::styled("│", theme.style_dim()),
                        Span::styled(right, Style::default().fg(theme.status.success)),
                    ]));
                    old_line += 1;
                    new_line += 1;
                }
            }
        }
    }

    lines
}

/// Render hunks with inline word-level highlighting.
pub(crate) fn render_word_diff(file: &FileDiff, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::from(vec![Span::styled(
        format!("  {} ", file.path),
        theme.style_accent().add_modifier(Modifier::BOLD),
    )]));

    let collapsed = collapse_to_replacements(&file.hunks);
    let mut old_line: usize;
    let mut new_line: usize;

    for hunk in &collapsed {
        old_line = hunk.old_start;
        new_line = hunk.new_start;

        lines.push(Line::from(vec![Span::styled(
            format!("@@ -{} +{} @@", hunk.old_start, hunk.new_start),
            Style::default().fg(theme.status.info),
        )]));

        for change in &hunk.changes {
            match change {
                DiffChange::Equal(text) => {
                    let display = text.trim_end_matches('\n');
                    lines.push(Line::from(vec![
                        Span::styled(format!("{old_line:>4} {new_line:>4} "), theme.style_dim()),
                        Span::styled(format!(" {display}"), theme.style_dim()),
                    ]));
                    old_line += 1;
                    new_line += 1;
                }
                DiffChange::Delete(text) => {
                    let display = text.trim_end_matches('\n');
                    lines.push(Line::from(vec![
                        Span::styled(format!("{old_line:>4}      "), theme.style_dim()),
                        Span::styled(
                            format!("-{display}"),
                            Style::default().fg(theme.status.error),
                        ),
                    ]));
                    old_line += 1;
                }
                DiffChange::Insert(text) => {
                    let display = text.trim_end_matches('\n');
                    lines.push(Line::from(vec![
                        Span::styled(format!("     {new_line:>4} "), theme.style_dim()),
                        Span::styled(
                            format!("+{display}"),
                            Style::default().fg(theme.status.success),
                        ),
                    ]));
                    new_line += 1;
                }
                DiffChange::Replace { old, new } => {
                    let old_trimmed = old.trim_end_matches('\n');
                    let new_trimmed = new.trim_end_matches('\n');
                    let word_diff = TextDiff::from_words(old_trimmed, new_trimmed);

                    let mut spans = vec![
                        Span::styled(format!("{old_line:>4} {new_line:>4} "), theme.style_dim()),
                        Span::styled("~", Style::default().fg(theme.status.warning)),
                    ];

                    for change_op in word_diff.iter_all_changes() {
                        let val = change_op.value().to_string();
                        match change_op.tag() {
                            ChangeTag::Equal => {
                                spans.push(Span::styled(val, theme.style_fg()));
                            }
                            ChangeTag::Delete => {
                                spans.push(Span::styled(
                                    val,
                                    Style::default()
                                        .fg(Color::White)
                                        .bg(theme.status.error)
                                        .add_modifier(Modifier::CROSSED_OUT),
                                ));
                            }
                            ChangeTag::Insert => {
                                spans.push(Span::styled(
                                    val,
                                    Style::default().fg(Color::White).bg(theme.status.success),
                                ));
                            }
                        }
                    }

                    lines.push(Line::from(spans));
                    old_line += 1;
                    new_line += 1;
                }
            }
        }
    }

    lines
}

/// Render a complete diff view state into ratatui Lines (mutable: updates total_lines).
#[cfg(test)]
pub(crate) fn render_diff_view(
    state: &mut DiffViewState,
    area: Rect,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut all_lines: Vec<Line<'static>> = Vec::new();

    all_lines.push(Line::from(vec![
        Span::styled(
            " Diff Viewer ",
            theme.style_accent().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("[{}]", state.mode.label()),
            Style::default().fg(theme.status.info),
        ),
        Span::styled("  m", theme.style_accent()),
        Span::styled(": cycle mode  ", theme.style_dim()),
        Span::styled("Esc", theme.style_accent()),
        Span::styled(": close  ", theme.style_dim()),
        Span::styled("↑↓", theme.style_accent()),
        Span::styled(": scroll", theme.style_dim()),
    ]));
    all_lines.push(Line::from(vec![Span::styled(
        "─".repeat(area.width as usize),
        theme.style_dim(),
    )]));

    if state.is_empty() {
        all_lines.push(Line::from(vec![Span::styled(
            "  No changes.",
            theme.style_dim(),
        )]));
        state.total_lines = all_lines.len();
        return all_lines;
    }

    for file in &state.files {
        if file.hunks.is_empty() {
            continue;
        }

        let file_lines = match state.mode {
            DiffMode::Unified => render_unified(file, theme),
            DiffMode::SideBySide => render_side_by_side(file, area.width, theme),
            DiffMode::WordDiff => render_word_diff(file, theme),
        };
        all_lines.extend(file_lines);
        all_lines.push(Line::raw("")); // spacer between files
    }

    state.total_lines = all_lines.len();
    all_lines
}

/// Immutable variant for rendering from view code (which has &App, not &mut App).
pub(crate) fn render_diff_view_immutable(
    state: &DiffViewState,
    area: Rect,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut all_lines: Vec<Line<'static>> = Vec::new();

    all_lines.push(Line::from(vec![
        Span::styled(
            " Diff Viewer ",
            theme.style_accent().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("[{}]", state.mode.label()),
            Style::default().fg(theme.status.info),
        ),
        Span::styled("  m", theme.style_accent()),
        Span::styled(": cycle mode  ", theme.style_dim()),
        Span::styled("Esc", theme.style_accent()),
        Span::styled(": close  ", theme.style_dim()),
        Span::styled("↑↓", theme.style_accent()),
        Span::styled(": scroll", theme.style_dim()),
    ]));
    all_lines.push(Line::from(vec![Span::styled(
        "─".repeat(area.width as usize),
        theme.style_dim(),
    )]));

    if state.is_empty() {
        all_lines.push(Line::from(vec![Span::styled(
            "  No changes.",
            theme.style_dim(),
        )]));
        return all_lines;
    }

    for file in &state.files {
        if file.hunks.is_empty() {
            continue;
        }

        let file_lines = match state.mode {
            DiffMode::Unified => render_unified(file, theme),
            DiffMode::SideBySide => render_side_by_side(file, area.width, theme),
            DiffMode::WordDiff => render_word_diff(file, theme),
        };
        all_lines.extend(file_lines);
        all_lines.push(Line::raw(""));
    }

    all_lines
}
