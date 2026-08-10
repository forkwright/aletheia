//! Diff view state and koilon-local text-to-diff computation.
//!
//! Structural diff parsing and types are gramma's (`gramma::diff`); this
//! module keeps only what is genuinely koilon-specific: the 3-way view mode
//! (gramma's `DiffViewMode` has 2 — `WordDiff` is TUI-only and unmodeled
//! upstream), scroll state, and a `similar`-backed text-to-diff adapter
//! gramma has no equivalent for (gramma parses an *existing* unified-diff
//! string; it does not compute one from two raw text blobs).

use gramma::diff::{ChangeType, DiffFile, DiffHunk, DiffLine};
use similar::{ChangeTag, TextDiff};

/// Display mode for the diff viewer.
///
/// `Unified` and `SideBySide` mirror `gramma::diff::DiffViewMode`;
/// `WordDiff` is a koilon-only third mode gramma does not model.
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

/// State for the diff viewer overlay/view.
#[derive(Debug, Clone)]
pub(crate) struct DiffViewState {
    pub(crate) mode: DiffMode,
    pub(crate) files: Vec<DiffFile>,
    pub(crate) scroll_offset: usize,
    /// Total rendered line count (computed during render).
    pub(crate) total_lines: usize,
}

impl DiffViewState {
    pub(crate) fn new(files: Vec<DiffFile>) -> Self {
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

/// Compute a diff between old and new text for a single file, in gramma's
/// `DiffFile` shape.
///
/// Koilon-local: gramma's parser only handles an *existing* unified-diff
/// string (`parse_unified_diff` / `parse_git_diff`); this adapts the
/// `similar` crate's text-to-text diff into gramma's model so the render
/// layer has exactly one data shape to consume regardless of whether the
/// diff came from `git diff` output or a before/after tool-result pair.
pub(crate) fn compute_diff(path: &str, old: &str, new: &str) -> DiffFile {
    let text_diff = TextDiff::from_lines(old, new);
    let mut hunks = Vec::new();
    let mut additions: u32 = 0;
    let mut deletions: u32 = 0;

    for group in text_diff.grouped_ops(3) {
        let mut lines = Vec::new();
        let old_start = saturating_u32(group.first().map_or(0, |op| op.old_range().start));
        let new_start = saturating_u32(group.first().map_or(0, |op| op.new_range().start));
        let mut old_line = old_start;
        let mut new_line = new_start;
        let mut old_count: u32 = 0;
        let mut new_count: u32 = 0;

        for op in &group {
            for change in text_diff.iter_changes(op) {
                let content = change.value().trim_end_matches('\n').to_string();
                match change.tag() {
                    ChangeTag::Equal => {
                        lines.push(DiffLine::new(
                            ChangeType::Context,
                            Some(old_line.saturating_add(1)),
                            Some(new_line.saturating_add(1)),
                            content,
                            Vec::new(),
                        ));
                        old_line = old_line.saturating_add(1);
                        new_line = new_line.saturating_add(1);
                        old_count = old_count.saturating_add(1);
                        new_count = new_count.saturating_add(1);
                    }
                    ChangeTag::Delete => {
                        lines.push(DiffLine::new(
                            ChangeType::Remove,
                            Some(old_line.saturating_add(1)),
                            None,
                            content,
                            Vec::new(),
                        ));
                        old_line = old_line.saturating_add(1);
                        old_count = old_count.saturating_add(1);
                        deletions = deletions.saturating_add(1);
                    }
                    ChangeTag::Insert => {
                        lines.push(DiffLine::new(
                            ChangeType::Add,
                            None,
                            Some(new_line.saturating_add(1)),
                            content,
                            Vec::new(),
                        ));
                        new_line = new_line.saturating_add(1);
                        new_count = new_count.saturating_add(1);
                        additions = additions.saturating_add(1);
                    }
                }
            }
        }

        hunks.push(DiffHunk::new(
            old_start.saturating_add(1),
            old_count,
            new_start.saturating_add(1),
            new_count,
            String::new(),
            lines,
        ));
    }

    DiffFile {
        path: path.to_string(),
        hunks,
        additions,
        deletions,
        mode: gramma::diff::DiffViewMode::default(),
    }
}

/// Widen a `similar` range offset (`usize`) to `u32`, saturating rather
/// than panicking or wrapping on the (practically unreachable) case of a
/// diff exceeding 4 billion lines.
fn saturating_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
