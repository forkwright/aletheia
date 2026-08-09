//! Diff rendering: unified, side-by-side, word-level, and view rendering.

use gramma::diff::{ChangeType, DiffFile, DiffLine};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use similar::{ChangeTag, TextDiff};
use unicode_width::UnicodeWidthStr;

use crate::text::truncate_cols_ellipsis;
use crate::theme::Theme;

use super::types::{DiffMode, DiffViewState};

pub(crate) fn render_unified(file: &DiffFile, theme: &Theme) -> Vec<Line<'static>> {
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

    for hunk in &file.hunks {
        lines.push(Line::from(vec![Span::styled(
            format!(
                "@@ -{},{} +{},{} @@",
                hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
            ),
            Style::default().fg(theme.status.info),
        )]));

        for line in &hunk.lines {
            lines.push(render_unified_line(line, theme));
        }
    }

    lines
}

fn render_unified_line(line: &DiffLine, theme: &Theme) -> Line<'static> {
    let gutter = format!(
        "{:>4} {:>4} ",
        display_no(line.old_line_no),
        display_no(line.new_line_no)
    );
    match line.change_type {
        ChangeType::Context => Line::from(vec![
            Span::styled(gutter, theme.style_dim()),
            Span::styled(format!(" {}", line.content), theme.style_dim()),
        ]),
        ChangeType::Remove => Line::from(vec![
            Span::styled(
                format!("{:>4}      ", display_no(line.old_line_no)),
                theme.style_dim(),
            ),
            Span::styled(
                format!("-{}", line.content),
                Style::default().fg(theme.status.error),
            ),
        ]),
        ChangeType::Add => Line::from(vec![
            Span::styled(
                format!("     {:>4} ", display_no(line.new_line_no)),
                theme.style_dim(),
            ),
            Span::styled(
                format!("+{}", line.content),
                Style::default().fg(theme.status.success),
            ),
        ]),
        // WHY: ChangeType is #[non_exhaustive] upstream; a future gramma
        // release could add a variant. Degrade to a dim, unprefixed line
        // rather than fail to build against a semver-compatible bump.
        _ => Line::from(vec![
            Span::styled(gutter, theme.style_dim()),
            Span::styled(line.content.clone(), theme.style_dim()),
        ]),
    }
}

/// Render hunks in a side-by-side layout.
pub(crate) fn render_side_by_side(file: &DiffFile, width: u16, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let half_width = usize::from(width) / 2;
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
        "─".repeat(usize::from(width)),
        theme.style_dim(),
    )]);
    lines.push(separator_line);

    for hunk in &file.hunks {
        lines.push(Line::from(vec![Span::styled(
            format!(
                "@@ -{},{} +{},{} @@",
                hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
            ),
            Style::default().fg(theme.status.info),
        )]));

        for line in &hunk.lines {
            lines.push(render_side_by_side_line(line, half_width, content_width, theme));
        }
    }

    lines
}

fn render_side_by_side_line(
    line: &DiffLine,
    half_width: usize,
    content_width: usize,
    theme: &Theme,
) -> Line<'static> {
    let truncated = pad_to(truncate_cols_ellipsis(&line.content, content_width), content_width);
    match line.change_type {
        ChangeType::Context => {
            let left = format!("{:>4} {truncated} ", display_no(line.old_line_no));
            let right = format!("{:>4} {truncated}", display_no(line.new_line_no));
            Line::from(vec![
                Span::styled(pad_to(left, half_width), theme.style_dim()),
                Span::styled("│", theme.style_dim()),
                Span::styled(right, theme.style_dim()),
            ])
        }
        ChangeType::Remove => {
            let left = format!("{:>4} {truncated} ", display_no(line.old_line_no));
            let right = format!("{:>4} {:<content_width$}", "", "");
            Line::from(vec![
                Span::styled(pad_to(left, half_width), Style::default().fg(theme.status.error)),
                Span::styled("│", theme.style_dim()),
                Span::styled(right, theme.style_dim()),
            ])
        }
        ChangeType::Add => {
            let left = format!("{:>4} {:<content_width$} ", "", "");
            let right = format!("{:>4} {truncated}", display_no(line.new_line_no));
            Line::from(vec![
                Span::styled(pad_to(left, half_width), theme.style_dim()),
                Span::styled("│", theme.style_dim()),
                Span::styled(right, Style::default().fg(theme.status.success)),
            ])
        }
        // WHY: ChangeType is #[non_exhaustive] upstream; a future gramma
        // release could add a variant. Degrade to a dim, unpaired line
        // rather than fail to build against a semver-compatible bump.
        _ => {
            let left = format!("{:>4} {truncated} ", display_no(line.old_line_no));
            let right = format!("{:>4} {truncated}", display_no(line.new_line_no));
            Line::from(vec![
                Span::styled(pad_to(left, half_width), theme.style_dim()),
                Span::styled("│", theme.style_dim()),
                Span::styled(right, theme.style_dim()),
            ])
        }
    }
}

/// A hunk line prepared for word-diff rendering. gramma's `DiffLine` /
/// `ChangeType` has no `Replace` concept — each line is independently
/// Context/Add/Remove — but word-diff mode needs old+new text on hand
/// together to compute inline highlighting, so a run of `Remove` lines
/// immediately followed by a run of `Add` lines is paired 1:1, same as the
/// old koilon-local `DiffChange::Replace` collapsing did.
#[derive(Debug)]
pub(super) enum RenderLine {
    Single(DiffLine),
    Replace { old: DiffLine, new: DiffLine },
}

/// Pair adjacent remove+add runs within a hunk's lines for word-diff mode.
#[expect(
    clippy::indexing_slicing,
    reason = "while loop maintains i < lines.len() invariant; look-ahead i+1 is guarded by the preceding i+1 < lines.len() check"
)]
pub(super) fn collapse_to_replacements(lines: &[DiffLine]) -> Vec<RenderLine> {
    let mut collapsed = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].change_type == ChangeType::Remove
            && i + 1 < lines.len()
            && lines[i + 1].change_type == ChangeType::Add
        {
            collapsed.push(RenderLine::Replace {
                old: lines[i].clone(), // kanon:ignore RUST/indexing-slicing -- i bounded by the while loop condition i < lines.len()
                new: lines[i + 1].clone(), // kanon:ignore RUST/indexing-slicing -- i+1 bounded by the preceding i+1 < lines.len() check
            });
            i += 2;
            continue;
        }
        collapsed.push(RenderLine::Single(lines[i].clone())); // kanon:ignore RUST/indexing-slicing -- i bounded by the while loop condition i < lines.len()
        i += 1;
    }

    collapsed
}

/// Render hunks with inline word-level highlighting.
pub(crate) fn render_word_diff(file: &DiffFile, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::from(vec![Span::styled(
        format!("  {} ", file.path),
        theme.style_accent().add_modifier(Modifier::BOLD),
    )]));

    for hunk in &file.hunks {
        lines.push(Line::from(vec![Span::styled(
            format!("@@ -{} +{} @@", hunk.old_start, hunk.new_start),
            Style::default().fg(theme.status.info),
        )]));

        for change in collapse_to_replacements(&hunk.lines) {
            match change {
                RenderLine::Single(line) => lines.push(render_unified_line(&line, theme)),
                RenderLine::Replace { old, new } => {
                    lines.push(render_word_diff_replace_line(&old, &new, theme));
                }
            }
        }
    }

    lines
}

fn render_word_diff_replace_line(old: &DiffLine, new: &DiffLine, theme: &Theme) -> Line<'static> {
    let word_diff = TextDiff::from_words(old.content.as_str(), new.content.as_str());

    let mut spans = vec![
        Span::styled(
            format!(
                "{:>4} {:>4} ",
                display_no(old.old_line_no),
                display_no(new.new_line_no)
            ),
            theme.style_dim(),
        ),
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

    Line::from(spans)
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
        "─".repeat(usize::from(area.width)),
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
        "─".repeat(usize::from(area.width)),
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

/// `NNNN` gutter text for an optional line number, blank when absent
/// (the line was only added or only removed).
fn display_no(n: Option<u32>) -> String {
    n.map_or_else(String::new, |n| n.to_string())
}

// WHY(#6542): `width` is a terminal column budget, so every measurement here is in
// display columns. Bytes, chars and columns all disagree on non-ASCII — a CJK char
// is three bytes, one char and two columns — and the pane is composed against the
// column grid, so measuring in any other unit lands the `│` separator off its
// column. `format!("{s:<width$}")` pads by chars and so cannot be used for this.
pub(super) fn pad_to(s: String, width: usize) -> String {
    let cols = UnicodeWidthStr::width(s.as_str());
    if cols >= width {
        truncate_cols_ellipsis(&s, width)
    } else {
        format!("{s}{}", " ".repeat(width - cols))
    }
}

