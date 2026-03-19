//! Diff computation and rendering for file changes.

use similar::{ChangeTag, TextDiff};

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
            // NOTE: 1-indexed per unified diff spec
            old_start: old_start + 1,
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
#[expect(
    clippy::indexing_slicing,
    reason = "while loop maintains i < changes.len() invariant; look-ahead i+1 is guarded by the preceding i+1 < changes.len() check"
)]
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
                        // WHY: look ahead for adjacent Insert to merge into a Replace
                        if i + 1 < changes.len()
                            && let DiffChange::Insert(new_text) = &changes[i + 1]
                        {
                            collapsed.push(DiffChange::Replace {
                                old: old_text.clone(),
                                new: new_text.clone(),
                            });
                            i += 2;
                            continue;
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
