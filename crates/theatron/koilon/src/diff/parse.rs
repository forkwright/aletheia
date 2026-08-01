//! Git diff parsing and string utilities.

use unicode_width::UnicodeWidthStr;

use crate::text::truncate_cols_ellipsis;

use super::types::{DiffChange, DiffHunk, FileDiff};

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

            // NOTE: extract path from "diff --git a/path b/path" by splitting on " b/"
            let path = line.split(" b/").nth(1).unwrap_or("unknown").to_string();
            current_path = Some(path);
            in_hunk = false;
        } else if line.starts_with("@@") {
            if in_hunk {
                current_hunks.push(DiffHunk {
                    old_start,
                    new_start,
                    changes: std::mem::take(&mut current_changes),
                });
            }

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
            // NOTE: skip git metadata lines such as "\ No newline at end of file"
        }
    }

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

pub(crate) fn parse_hunk_header(line: &str) -> (usize, usize) {
    // NOTE: handles both "@@ -1,3 +1,4 @@" and "@@ -1 +1 @@" (count optional)
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
