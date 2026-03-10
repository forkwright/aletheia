//! Diff computation and rendering for file changes.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use similar::{ChangeTag, TextDiff};

use crate::theme::Theme;

/// Display mode for the diff viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DiffMode {
    Unified,
    SideBySide,
    WordDiff,
}

impl DiffMode {
    /// Cycle to the next mode.
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Unified => Self::SideBySide,
            Self::SideBySide => Self::WordDiff,
            Self::WordDiff => Self::Unified,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Unified => "Unified",
            Self::SideBySide => "Side-by-Side",
            Self::WordDiff => "Word Diff",
        }
    }
}

/// A single change within a hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiffChange {
    Equal(String),
    Insert(String),
    Delete(String),
    /// For word-diff mode: a line that was changed, with old and new text.
    Replace {
        old: String,
        new: String,
    },
}

/// A contiguous group of changes with context.
#[derive(Debug, Clone)]
pub(crate) struct DiffHunk {
    pub(crate) old_start: usize,
    pub(crate) new_start: usize,
    pub(crate) changes: Vec<DiffChange>,
}

/// Represents a complete diff for one file.
#[derive(Debug, Clone)]
pub(crate) struct FileDiff {
    pub(crate) path: String,
    pub(crate) hunks: Vec<DiffHunk>,
}

/// State for the diff viewer overlay/view.
#[derive(Debug, Clone)]
pub(crate) struct DiffViewState {
    pub(crate) mode: DiffMode,
    pub(crate) files: Vec<FileDiff>,
    pub(crate) scroll_offset: usize,
    /// Total rendered line count (computed during render).
    pub(crate) total_lines: usize,
}

impl DiffViewState {
    pub(crate) fn new(files: Vec<FileDiff>) -> Self {
        Self {
            mode: DiffMode::Unified,
            files,
            scroll_offset: 0,
            total_lines: 0,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.files.is_empty() || self.files.iter().all(|f| f.hunks.is_empty())
    }

    pub(crate) fn cycle_mode(&mut self) {
        self.mode = self.mode.next();
        self.scroll_offset = 0;
    }

    pub(crate) fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    pub(crate) fn scroll_down(&mut self, lines: usize) {
        let max = self.total_lines.saturating_sub(1);
        self.scroll_offset = (self.scroll_offset + lines).min(max);
    }
}

/// Compute a diff between old and new text for a single file.
pub(crate) fn compute_diff(path: &str, old: &str, new: &str) -> FileDiff {
    let text_diff = TextDiff::from_lines(old, new);
    let mut hunks = Vec::new();

    for group in text_diff.grouped_ops(3) {
        let mut changes = Vec::new();
        let old_start = group.first().map(|op| op.old_range().start).unwrap_or(0);
        let new_start = group.first().map(|op| op.new_range().start).unwrap_or(0);

        for op in &group {
            for change in text_diff.iter_changes(op) {
                let value = change.value().to_string();
                match change.tag() {
                    ChangeTag::Equal => changes.push(DiffChange::Equal(value)),
                    ChangeTag::Insert => changes.push(DiffChange::Insert(value)),
                    ChangeTag::Delete => changes.push(DiffChange::Delete(value)),
                }
            }
        }

        hunks.push(DiffHunk {
            old_start: old_start + 1, // 1-indexed
            new_start: new_start + 1,
            changes,
        });
    }

    FileDiff {
        path: path.to_string(),
        hunks,
    }
}

/// Collapse adjacent Delete+Insert pairs into Replace for word-diff mode.
pub(crate) fn collapse_to_replacements(hunks: &[DiffHunk]) -> Vec<DiffHunk> {
    hunks
        .iter()
        .map(|hunk| {
            let mut collapsed = Vec::new();
            let mut i = 0;
            let changes = &hunk.changes;

            while i < changes.len() {
                match &changes[i] {
                    DiffChange::Delete(old_text) => {
                        // Look ahead for adjacent Insert
                        if i + 1 < changes.len() {
                            if let DiffChange::Insert(new_text) = &changes[i + 1] {
                                collapsed.push(DiffChange::Replace {
                                    old: old_text.clone(),
                                    new: new_text.clone(),
                                });
                                i += 2;
                                continue;
                            }
                        }
                        collapsed.push(changes[i].clone());
                    }
                    _ => collapsed.push(changes[i].clone()),
                }
                i += 1;
            }

            DiffHunk {
                old_start: hunk.old_start,
                new_start: hunk.new_start,
                changes: collapsed,
            }
        })
        .collect()
}

/// Render hunks in unified diff format.
pub(crate) fn render_unified(file: &FileDiff, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // File header
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

        // Count lines per side for hunk header
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

        // Hunk header
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
    let gutter = 6; // "NNNN " line-number gutter
    let content_width = half_width.saturating_sub(gutter + 2); // -2 for borders

    // File header spanning full width
    let header = format!("  {} ", file.path);
    lines.push(Line::from(vec![Span::styled(
        header,
        theme.style_accent().add_modifier(Modifier::BOLD),
    )]));

    // Column headers
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

        // Hunk separator
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

    // File header
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

        // Hunk header
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
                    // Word-level diff within the line
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

// ---------------------------------------------------------------------------
// Full render dispatch
// ---------------------------------------------------------------------------

/// Render a complete diff view state into ratatui Lines (mutable — updates total_lines).
#[cfg(test)]
pub(crate) fn render_diff_view(
    state: &mut DiffViewState,
    area: Rect,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut all_lines: Vec<Line<'static>> = Vec::new();

    // Mode indicator header
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

    // Mode indicator header
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
        all_lines.push(Line::raw("")); // spacer between files
    }

    all_lines
}

// ---------------------------------------------------------------------------
// Parse git-style unified diff output
// ---------------------------------------------------------------------------

/// Parse `git diff` output into a list of `FileDiff`.
pub(crate) fn parse_git_diff(raw: &str) -> Vec<FileDiff> {
    let mut files = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_hunks: Vec<DiffHunk> = Vec::new();
    let mut current_changes: Vec<DiffChange> = Vec::new();
    let mut old_start: usize = 1;
    let mut new_start: usize = 1;
    let mut in_hunk = false;

    for line in raw.lines() {
        if line.starts_with("diff --git") {
            // Flush previous file
            if let Some(path) = current_path.take() {
                if in_hunk {
                    current_hunks.push(DiffHunk {
                        old_start,
                        new_start,
                        changes: std::mem::take(&mut current_changes),
                    });
                }
                files.push(FileDiff {
                    path,
                    hunks: std::mem::take(&mut current_hunks),
                });
            }

            // Extract path from "diff --git a/path b/path"
            let path = line.split(" b/").nth(1).unwrap_or("unknown").to_string();
            current_path = Some(path);
            in_hunk = false;
        } else if line.starts_with("@@") {
            // Flush previous hunk
            if in_hunk {
                current_hunks.push(DiffHunk {
                    old_start,
                    new_start,
                    changes: std::mem::take(&mut current_changes),
                });
            }

            // Parse "@@ -old,count +new,count @@"
            let (os, ns) = parse_hunk_header(line);
            old_start = os;
            new_start = ns;
            in_hunk = true;
        } else if in_hunk {
            if let Some(rest) = line.strip_prefix('+') {
                current_changes.push(DiffChange::Insert(format!("{rest}\n")));
            } else if let Some(rest) = line.strip_prefix('-') {
                current_changes.push(DiffChange::Delete(format!("{rest}\n")));
            } else if let Some(rest) = line.strip_prefix(' ') {
                current_changes.push(DiffChange::Equal(format!("{rest}\n")));
            } else if line.is_empty() {
                current_changes.push(DiffChange::Equal("\n".to_string()));
            }
            // Skip lines like "\ No newline at end of file"
        }
    }

    // Flush final file
    if let Some(path) = current_path {
        if in_hunk {
            current_hunks.push(DiffHunk {
                old_start,
                new_start,
                changes: std::mem::take(&mut current_changes),
            });
        }
        files.push(FileDiff {
            path,
            hunks: current_hunks,
        });
    }

    files
}

fn parse_hunk_header(line: &str) -> (usize, usize) {
    // "@@ -1,3 +1,4 @@" or "@@ -1 +1 @@"
    let stripped = line
        .trim_start_matches("@@ ")
        .split(" @@")
        .next()
        .unwrap_or("");

    let mut parts = stripped.split(' ');

    let old_part = parts.next().unwrap_or("-1");
    let new_part = parts.next().unwrap_or("+1");

    let old_start = old_part
        .trim_start_matches('-')
        .split(',')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let new_start = new_part
        .trim_start_matches('+')
        .split(',')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    (old_start, new_start)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        let mut end = max_chars.min(s.len());
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        let truncated = &s[..end];
        if end >= 2 {
            format!("{}…", &truncated[..truncated.len().saturating_sub(1)])
        } else {
            truncated.to_string()
        }
    }
}

fn pad_to(s: String, width: usize) -> String {
    if s.len() >= width {
        s[..width].to_string()
    } else {
        format!("{s:<width$}")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_theme() -> Theme {
        Theme::detect()
    }

    // --- DiffMode ---

    #[test]
    fn mode_cycles_correctly() {
        assert_eq!(DiffMode::Unified.next(), DiffMode::SideBySide);
        assert_eq!(DiffMode::SideBySide.next(), DiffMode::WordDiff);
        assert_eq!(DiffMode::WordDiff.next(), DiffMode::Unified);
    }

    #[test]
    fn mode_labels() {
        assert_eq!(DiffMode::Unified.label(), "Unified");
        assert_eq!(DiffMode::SideBySide.label(), "Side-by-Side");
        assert_eq!(DiffMode::WordDiff.label(), "Word Diff");
    }

    // --- compute_diff ---

    #[test]
    fn compute_diff_no_changes() {
        let diff = compute_diff("test.rs", "hello\n", "hello\n");
        assert!(diff.hunks.is_empty());
    }

    #[test]
    fn compute_diff_simple_addition() {
        let diff = compute_diff("test.rs", "line1\n", "line1\nline2\n");
        assert!(!diff.hunks.is_empty());
        let changes = &diff.hunks[0].changes;
        assert!(changes.iter().any(|c| matches!(c, DiffChange::Insert(_))));
    }

    #[test]
    fn compute_diff_simple_deletion() {
        let diff = compute_diff("test.rs", "line1\nline2\n", "line1\n");
        assert!(!diff.hunks.is_empty());
        let changes = &diff.hunks[0].changes;
        assert!(changes.iter().any(|c| matches!(c, DiffChange::Delete(_))));
    }

    #[test]
    fn compute_diff_modification() {
        let diff = compute_diff("test.rs", "old line\n", "new line\n");
        assert!(!diff.hunks.is_empty());
        let changes = &diff.hunks[0].changes;
        assert!(changes.iter().any(|c| matches!(c, DiffChange::Delete(_))));
        assert!(changes.iter().any(|c| matches!(c, DiffChange::Insert(_))));
    }

    #[test]
    fn compute_diff_preserves_path() {
        let diff = compute_diff("src/main.rs", "a\n", "b\n");
        assert_eq!(diff.path, "src/main.rs");
    }

    // --- collapse_to_replacements ---

    #[test]
    fn collapse_pairs_delete_insert_to_replace() {
        let hunks = vec![DiffHunk {
            old_start: 1,
            new_start: 1,
            changes: vec![
                DiffChange::Delete("old\n".to_string()),
                DiffChange::Insert("new\n".to_string()),
            ],
        }];
        let collapsed = collapse_to_replacements(&hunks);
        assert_eq!(collapsed[0].changes.len(), 1);
        assert!(matches!(
            &collapsed[0].changes[0],
            DiffChange::Replace { .. }
        ));
    }

    #[test]
    fn collapse_leaves_standalone_delete() {
        let hunks = vec![DiffHunk {
            old_start: 1,
            new_start: 1,
            changes: vec![DiffChange::Delete("removed\n".to_string())],
        }];
        let collapsed = collapse_to_replacements(&hunks);
        assert!(matches!(&collapsed[0].changes[0], DiffChange::Delete(_)));
    }

    #[test]
    fn collapse_leaves_standalone_insert() {
        let hunks = vec![DiffHunk {
            old_start: 1,
            new_start: 1,
            changes: vec![DiffChange::Insert("added\n".to_string())],
        }];
        let collapsed = collapse_to_replacements(&hunks);
        assert!(matches!(&collapsed[0].changes[0], DiffChange::Insert(_)));
    }

    // --- Unified rendering ---

    #[test]
    fn unified_render_has_file_header() {
        let theme = test_theme();
        let diff = compute_diff("src/lib.rs", "old\n", "new\n");
        let lines = render_unified(&diff, &theme);
        let header_text: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(header_text.contains("a/src/lib.rs"));
    }

    #[test]
    fn unified_render_has_hunk_header() {
        let theme = test_theme();
        let diff = compute_diff("test.rs", "old\n", "new\n");
        let lines = render_unified(&diff, &theme);
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.to_string())
            .collect();
        assert!(all_text.contains("@@"));
    }

    #[test]
    fn unified_render_shows_plus_minus() {
        let theme = test_theme();
        let diff = compute_diff("test.rs", "old_line\n", "new_line\n");
        let lines = render_unified(&diff, &theme);
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.to_string())
            .collect();
        assert!(all_text.contains("-old_line"));
        assert!(all_text.contains("+new_line"));
    }

    // --- Side-by-side rendering ---

    #[test]
    fn side_by_side_render_has_header() {
        let theme = test_theme();
        let diff = compute_diff("test.rs", "old\n", "new\n");
        let lines = render_side_by_side(&diff, 80, &theme);
        let header_text: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(header_text.contains("test.rs"));
    }

    #[test]
    fn side_by_side_render_at_various_widths() {
        let theme = test_theme();
        let diff = compute_diff("test.rs", "old content\n", "new content\n");
        for width in [40, 60, 80, 120, 200] {
            let lines = render_side_by_side(&diff, width, &theme);
            assert!(!lines.is_empty(), "Failed at width {width}");
        }
    }

    // --- Word diff rendering ---

    #[test]
    fn word_diff_render_has_header() {
        let theme = test_theme();
        let diff = compute_diff("test.rs", "old word\n", "new word\n");
        let lines = render_word_diff(&diff, &theme);
        let header_text: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert!(header_text.contains("test.rs"));
    }

    #[test]
    fn word_diff_highlights_changed_tokens() {
        let theme = test_theme();
        let diff = compute_diff("test.rs", "let x = 42;\n", "let x = 99;\n");
        // Collapse to get Replace variants
        let collapsed_file = FileDiff {
            path: diff.path,
            hunks: collapse_to_replacements(&diff.hunks),
        };
        let lines = render_word_diff(&collapsed_file, &theme);
        // Should have word-level spans with different styles
        let all_spans: Vec<&Span> = lines.iter().flat_map(|l| l.spans.iter()).collect();
        assert!(
            all_spans.len() > 2,
            "Expected multiple spans for word-level diff"
        );
    }

    // --- DiffViewState ---

    #[test]
    fn diff_view_state_empty() {
        let state = DiffViewState::new(vec![]);
        assert!(state.is_empty());
    }

    #[test]
    fn diff_view_state_not_empty_with_hunks() {
        let diff = compute_diff("test.rs", "a\n", "b\n");
        let state = DiffViewState::new(vec![diff]);
        assert!(!state.is_empty());
    }

    #[test]
    fn diff_view_state_cycle_mode() {
        let mut state = DiffViewState::new(vec![]);
        assert_eq!(state.mode, DiffMode::Unified);
        state.cycle_mode();
        assert_eq!(state.mode, DiffMode::SideBySide);
        state.cycle_mode();
        assert_eq!(state.mode, DiffMode::WordDiff);
        state.cycle_mode();
        assert_eq!(state.mode, DiffMode::Unified);
    }

    #[test]
    fn diff_view_state_scroll() {
        let mut state = DiffViewState::new(vec![]);
        state.total_lines = 50;
        state.scroll_down(10);
        assert_eq!(state.scroll_offset, 10);
        state.scroll_up(3);
        assert_eq!(state.scroll_offset, 7);
        state.scroll_up(100);
        assert_eq!(state.scroll_offset, 0);
    }

    #[test]
    fn diff_view_state_scroll_clamps_at_max() {
        let mut state = DiffViewState::new(vec![]);
        state.total_lines = 20;
        state.scroll_down(100);
        assert_eq!(state.scroll_offset, 19); // total_lines - 1
    }

    // --- parse_git_diff ---

    #[test]
    fn parse_git_diff_basic() {
        let raw = "\
diff --git a/src/main.rs b/src/main.rs
index abc1234..def5678 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
-    println!(\"hello\");
+    println!(\"hello world\");
+    println!(\"goodbye\");
 }
";
        let files = parse_git_diff(raw);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/main.rs");
        assert!(!files[0].hunks.is_empty());
        let changes = &files[0].hunks[0].changes;
        assert!(changes.iter().any(|c| matches!(c, DiffChange::Delete(_))));
        assert!(changes.iter().any(|c| matches!(c, DiffChange::Insert(_))));
        assert!(changes.iter().any(|c| matches!(c, DiffChange::Equal(_))));
    }

    #[test]
    fn parse_git_diff_multiple_files() {
        let raw = "\
diff --git a/a.rs b/a.rs
--- a/a.rs
+++ b/a.rs
@@ -1,2 +1,2 @@
-old a
+new a
diff --git a/b.rs b/b.rs
--- a/b.rs
+++ b/b.rs
@@ -1,2 +1,2 @@
-old b
+new b
";
        let files = parse_git_diff(raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "a.rs");
        assert_eq!(files[1].path, "b.rs");
    }

    #[test]
    fn parse_git_diff_empty() {
        let files = parse_git_diff("");
        assert!(files.is_empty());
    }

    #[test]
    fn parse_hunk_header_standard() {
        assert_eq!(parse_hunk_header("@@ -10,5 +20,7 @@"), (10, 20));
    }

    #[test]
    fn parse_hunk_header_no_count() {
        assert_eq!(parse_hunk_header("@@ -1 +1 @@"), (1, 1));
    }

    // --- render_diff_view ---

    #[test]
    fn render_diff_view_empty_shows_no_changes() {
        let theme = test_theme();
        let mut state = DiffViewState::new(vec![]);
        let area = Rect::new(0, 0, 80, 24);
        let lines = render_diff_view(&mut state, area, &theme);
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.to_string())
            .collect();
        assert!(all_text.contains("No changes"));
    }

    #[test]
    fn render_diff_view_updates_total_lines() {
        let theme = test_theme();
        let diff = compute_diff("test.rs", "old line\n", "new line\n");
        let mut state = DiffViewState::new(vec![diff]);
        let area = Rect::new(0, 0, 80, 24);
        render_diff_view(&mut state, area, &theme);
        assert!(state.total_lines > 0);
    }

    // --- Large diff ---

    #[test]
    fn large_diff_renders_without_panic() {
        let theme = test_theme();
        let old: String = (0..500).map(|i| format!("line {i}\n")).collect();
        let new: String = (0..500)
            .map(|i| {
                if i % 10 == 0 {
                    format!("modified line {i}\n")
                } else {
                    format!("line {i}\n")
                }
            })
            .collect();
        let diff = compute_diff("big.rs", &old, &new);
        let mut state = DiffViewState::new(vec![diff]);
        let area = Rect::new(0, 0, 120, 40);
        let lines = render_diff_view(&mut state, area, &theme);
        assert!(lines.len() > 10);
        assert!(state.total_lines > 10);
    }

    // --- truncate_str ---

    #[test]
    fn truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn truncate_str_long() {
        let result = truncate_str("hello world", 6);
        assert!(result.len() <= 8); // may have ellipsis char
    }

    // --- File path display ---

    #[test]
    fn file_path_displayed_in_all_modes() {
        let theme = test_theme();
        let diff = compute_diff("src/important.rs", "old\n", "new\n");

        for render_fn in [
            |f: &FileDiff, t: &Theme| render_unified(f, t),
            |f: &FileDiff, t: &Theme| render_word_diff(f, t),
        ] {
            let lines = render_fn(&diff, &theme);
            let all_text: String = lines
                .iter()
                .flat_map(|l| l.spans.iter())
                .map(|s| s.content.to_string())
                .collect();
            assert!(
                all_text.contains("important.rs"),
                "File path missing in render output"
            );
        }

        let sbs_lines = render_side_by_side(&diff, 80, &theme);
        let sbs_text: String = sbs_lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.to_string())
            .collect();
        assert!(sbs_text.contains("important.rs"));
    }
}
