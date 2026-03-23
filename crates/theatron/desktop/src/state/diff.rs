//! Diff state: unified diff parsing and structured representation.

use std::fmt;

/// Maximum tokens per line pair before falling back to whole-line highlighting.
const WORD_DIFF_TOKEN_LIMIT: usize = 500;

/// View mode for the diff viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum DiffViewMode {
    #[default]
    Unified,
    SideBySide,
}

impl DiffViewMode {
    /// Toggle between unified and side-by-side.
    #[must_use]
    pub(crate) fn toggle(self) -> Self {
        match self {
            Self::Unified => Self::SideBySide,
            Self::SideBySide => Self::Unified,
        }
    }
}

impl fmt::Display for DiffViewMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unified => f.write_str("Unified"),
            Self::SideBySide => f.write_str("Side-by-Side"),
        }
    }
}

/// Type of change for a single diff line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChangeType {
    Context,
    Add,
    Remove,
}

/// A single line in a diff hunk.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DiffLine {
    pub change_type: ChangeType,
    pub old_line_no: Option<u32>,
    pub new_line_no: Option<u32>,
    pub content: String,
    /// Word-level change spans within the line content.
    /// Empty if this is a context line or word diff was skipped.
    pub word_spans: Vec<WordSpan>,
}

/// A span within a diff line, marking whether it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WordSpan {
    pub text: String,
    pub changed: bool,
}

/// A single hunk in a diff file.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DiffHunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub context_label: String,
    pub lines: Vec<DiffLine>,
}

/// Parsed diff for a single file.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DiffFile {
    pub path: String,
    pub hunks: Vec<DiffHunk>,
    pub additions: u32,
    pub deletions: u32,
    pub mode: DiffViewMode,
}

/// Aligned row for side-by-side display.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SideBySideRow {
    pub left: Option<DiffLine>,
    pub right: Option<DiffLine>,
}

/// Parse a unified diff string into a `DiffFile`.
#[must_use]
pub(crate) fn parse_unified_diff(path: &str, raw: &str) -> DiffFile {
    let mut hunks = Vec::new();
    let mut additions: u32 = 0;
    let mut deletions: u32 = 0;

    let mut current_hunk: Option<HunkBuilder> = None;

    for line in raw.lines() {
        if let Some(hunk_header) = parse_hunk_header(line) {
            if let Some(builder) = current_hunk.take() {
                hunks.push(builder.build());
            }
            current_hunk = Some(HunkBuilder::new(hunk_header));
            continue;
        }

        // NOTE: Skip file-level headers (---, +++, diff, index).
        if line.starts_with("---")
            || line.starts_with("+++")
            || line.starts_with("diff ")
            || line.starts_with("index ")
        {
            continue;
        }

        if let Some(ref mut builder) = current_hunk {
            if let Some(stripped) = line.strip_prefix('+') {
                additions += 1;
                builder.add_line(ChangeType::Add, stripped);
            } else if let Some(stripped) = line.strip_prefix('-') {
                deletions += 1;
                builder.add_line(ChangeType::Remove, stripped);
            } else if let Some(stripped) = line.strip_prefix(' ') {
                builder.add_line(ChangeType::Context, stripped);
            } else if line == "\\ No newline at end of file" {
                // NOTE: Git marker, not actual content.
            } else {
                // WHY: Some diffs omit the leading space for context lines.
                builder.add_line(ChangeType::Context, line);
            }
        }
    }

    if let Some(builder) = current_hunk {
        hunks.push(builder.build());
    }

    // NOTE: Compute word-level diffs for adjacent remove+add pairs.
    for hunk in &mut hunks {
        compute_word_diffs(&mut hunk.lines);
    }

    DiffFile {
        path: path.to_string(),
        hunks,
        additions,
        deletions,
        mode: DiffViewMode::default(),
    }
}

/// Align hunk lines for side-by-side display.
#[must_use]
pub(crate) fn align_side_by_side(lines: &[DiffLine]) -> Vec<SideBySideRow> {
    let mut rows = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        match lines[i].change_type {
            ChangeType::Context => {
                rows.push(SideBySideRow {
                    left: Some(lines[i].clone()),
                    right: Some(lines[i].clone()),
                });
                i += 1;
            }
            ChangeType::Remove => {
                // WHY: Collect consecutive removes, then pair with consecutive adds.
                let remove_start = i;
                while i < lines.len() && lines[i].change_type == ChangeType::Remove {
                    i += 1;
                }
                let removes = &lines[remove_start..i];

                let add_start = i;
                while i < lines.len() && lines[i].change_type == ChangeType::Add {
                    i += 1;
                }
                let adds = &lines[add_start..i];

                let max_len = removes.len().max(adds.len());
                for j in 0..max_len {
                    rows.push(SideBySideRow {
                        left: removes.get(j).cloned(),
                        right: adds.get(j).cloned(),
                    });
                }
            }
            ChangeType::Add => {
                rows.push(SideBySideRow {
                    left: None,
                    right: Some(lines[i].clone()),
                });
                i += 1;
            }
        }
    }

    rows
}

// -- Hunk header parsing ------------------------------------------------------

struct HunkHeader {
    old_start: u32,
    old_count: u32,
    new_start: u32,
    new_count: u32,
    context_label: String,
}

/// Parse `@@ -old,count +new,count @@ context`.
fn parse_hunk_header(line: &str) -> Option<HunkHeader> {
    let line = line.strip_prefix("@@ ")?;
    let rest = line.strip_prefix('-')?;
    let at_idx = rest.find(" +")?;
    let old_part = &rest[..at_idx];
    let rest = &rest[at_idx + 2..];

    let end_idx = rest.find(" @@")?;
    let new_part = &rest[..end_idx];
    let context_label = rest.get(end_idx + 3..).unwrap_or("").trim().to_string();

    let (old_start, old_count) = parse_range(old_part);
    let (new_start, new_count) = parse_range(new_part);

    Some(HunkHeader {
        old_start,
        old_count,
        new_start,
        new_count,
        context_label,
    })
}

/// Parse `start,count` or `start` (count defaults to 1).
fn parse_range(s: &str) -> (u32, u32) {
    if let Some((start, count)) = s.split_once(',') {
        (start.parse().unwrap_or(1), count.parse().unwrap_or(1))
    } else {
        (s.parse().unwrap_or(1), 1)
    }
}

// -- Hunk builder -------------------------------------------------------------

struct HunkBuilder {
    old_start: u32,
    old_count: u32,
    new_start: u32,
    new_count: u32,
    context_label: String,
    lines: Vec<DiffLine>,
    old_line: u32,
    new_line: u32,
}

impl HunkBuilder {
    fn new(header: HunkHeader) -> Self {
        Self {
            old_start: header.old_start,
            old_count: header.old_count,
            new_start: header.new_start,
            new_count: header.new_count,
            context_label: header.context_label,
            lines: Vec::new(),
            old_line: header.old_start,
            new_line: header.new_start,
        }
    }

    fn add_line(&mut self, change_type: ChangeType, content: &str) {
        let (old_line_no, new_line_no) = match change_type {
            ChangeType::Context => {
                let old = self.old_line;
                let new = self.new_line;
                self.old_line += 1;
                self.new_line += 1;
                (Some(old), Some(new))
            }
            ChangeType::Add => {
                let new = self.new_line;
                self.new_line += 1;
                (None, Some(new))
            }
            ChangeType::Remove => {
                let old = self.old_line;
                self.old_line += 1;
                (Some(old), None)
            }
        };

        self.lines.push(DiffLine {
            change_type,
            old_line_no,
            new_line_no,
            content: content.to_string(),
            word_spans: Vec::new(),
        });
    }

    fn build(self) -> DiffHunk {
        DiffHunk {
            old_start: self.old_start,
            old_count: self.old_count,
            new_start: self.new_start,
            new_count: self.new_count,
            context_label: self.context_label,
            lines: self.lines,
        }
    }
}

// -- Word-level diff ----------------------------------------------------------

/// Compute word-level diffs for adjacent remove+add line pairs within a hunk.
fn compute_word_diffs(lines: &mut [DiffLine]) {
    let mut i = 0;
    while i < lines.len() {
        if lines[i].change_type == ChangeType::Remove {
            // NOTE: Find the run of removes followed by adds.
            let remove_start = i;
            while i < lines.len() && lines[i].change_type == ChangeType::Remove {
                i += 1;
            }
            let add_start = i;
            while i < lines.len() && lines[i].change_type == ChangeType::Add {
                i += 1;
            }
            let add_end = i;

            let remove_count = add_start - remove_start;
            let add_count = add_end - add_start;
            let pairs = remove_count.min(add_count);

            for p in 0..pairs {
                let ri = remove_start + p;
                let ai = add_start + p;
                let old_content = lines[ri].content.clone();
                let new_content = lines[ai].content.clone();

                let old_tokens = tokenize(&old_content);
                let new_tokens = tokenize(&new_content);

                if old_tokens.len() + new_tokens.len() > WORD_DIFF_TOKEN_LIMIT {
                    continue;
                }

                let (old_spans, new_spans) = diff_tokens(&old_tokens, &new_tokens);
                lines[ri].word_spans = old_spans;
                lines[ai].word_spans = new_spans;
            }
        } else {
            i += 1;
        }
    }
}

/// Split text into tokens on whitespace and punctuation boundaries.
fn tokenize(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = 0;
    let bytes = s.as_bytes();

    for (i, &b) in bytes.iter().enumerate() {
        let is_boundary = b == b' ' || b == b'\t' || b.is_ascii_punctuation();
        if is_boundary {
            if start < i {
                tokens.push(&s[start..i]);
            }
            tokens.push(&s[i..i + 1]);
            start = i + 1;
        }
    }
    if start < s.len() {
        tokens.push(&s[start..]);
    }
    tokens
}

/// Compute LCS-based word diff, returning spans for old and new lines.
fn diff_tokens(old: &[&str], new: &[&str]) -> (Vec<WordSpan>, Vec<WordSpan>) {
    let lcs = lcs_table(old, new);
    let mut old_spans = Vec::new();
    let mut new_spans = Vec::new();

    let mut i = old.len();
    let mut j = new.len();

    // NOTE: Backtrace from LCS table to build diff spans.
    let mut old_rev = Vec::new();
    let mut new_rev = Vec::new();

    while i > 0 || j > 0 {
        if i > 0 && j > 0 && old[i - 1] == new[j - 1] {
            old_rev.push(WordSpan {
                text: old[i - 1].to_string(),
                changed: false,
            });
            new_rev.push(WordSpan {
                text: new[j - 1].to_string(),
                changed: false,
            });
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || lcs_val(&lcs, i, j - 1) >= lcs_val(&lcs, i - 1, j)) {
            new_rev.push(WordSpan {
                text: new[j - 1].to_string(),
                changed: true,
            });
            j -= 1;
        } else if i > 0 {
            old_rev.push(WordSpan {
                text: old[i - 1].to_string(),
                changed: true,
            });
            i -= 1;
        }
    }

    old_rev.reverse();
    new_rev.reverse();

    // NOTE: Merge adjacent spans with the same `changed` flag for cleaner output.
    merge_spans(&old_rev, &mut old_spans);
    merge_spans(&new_rev, &mut new_spans);

    (old_spans, new_spans)
}

/// Build LCS length table (m+1 x n+1).
fn lcs_table(old: &[&str], new: &[&str]) -> Vec<Vec<u32>> {
    let m = old.len();
    let n = new.len();
    let mut table = vec![vec![0u32; n + 1]; m + 1];

    for i in 1..=m {
        for j in 1..=n {
            if old[i - 1] == new[j - 1] {
                table[i][j] = table[i - 1][j - 1] + 1;
            } else {
                table[i][j] = table[i - 1][j].max(table[i][j - 1]);
            }
        }
    }

    table
}

fn lcs_val(table: &[Vec<u32>], i: usize, j: usize) -> u32 {
    table[i][j]
}

/// Merge adjacent spans with the same `changed` flag.
fn merge_spans(input: &[WordSpan], output: &mut Vec<WordSpan>) {
    for span in input {
        if let Some(last) = output.last_mut() {
            if last.changed == span.changed {
                last.text.push_str(&span.text);
                continue;
            }
        }
        output.push(span.clone());
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use super::*;

    const SAMPLE_DIFF: &str = "\
diff --git a/src/main.rs b/src/main.rs
index abc1234..def5678 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,5 +1,6 @@ fn main() {
     let x = 1;
-    let y = 2;
+    let y = 3;
+    let z = 4;
     println!(\"{x}\");
 }
";

    #[test]
    fn parses_hunk_header_with_context() {
        let h = parse_hunk_header("@@ -1,5 +1,6 @@ fn main() {").unwrap();
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 5);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.new_count, 6);
        assert_eq!(h.context_label, "fn main() {");
    }

    #[test]
    fn parses_hunk_header_no_count() {
        let h = parse_hunk_header("@@ -1 +1 @@").unwrap();
        assert_eq!(h.old_start, 1);
        assert_eq!(h.old_count, 1);
        assert_eq!(h.new_start, 1);
        assert_eq!(h.new_count, 1);
        assert_eq!(h.context_label, "");
    }

    #[test]
    fn parse_unified_diff_basic() {
        let diff = parse_unified_diff("src/main.rs", SAMPLE_DIFF);
        assert_eq!(diff.path, "src/main.rs");
        assert_eq!(diff.additions, 2);
        assert_eq!(diff.deletions, 1);
        assert_eq!(diff.hunks.len(), 1);

        let hunk = &diff.hunks[0];
        assert_eq!(hunk.old_start, 1);
        assert_eq!(hunk.new_start, 1);
        assert_eq!(hunk.lines.len(), 6);
    }

    #[test]
    fn diff_line_numbers_tracked_correctly() {
        let diff = parse_unified_diff("test.rs", SAMPLE_DIFF);
        let lines = &diff.hunks[0].lines;

        // First context line: old=1, new=1
        assert_eq!(lines[0].change_type, ChangeType::Context);
        assert_eq!(lines[0].old_line_no, Some(1));
        assert_eq!(lines[0].new_line_no, Some(1));

        // Removed line: old=2, no new
        assert_eq!(lines[1].change_type, ChangeType::Remove);
        assert_eq!(lines[1].old_line_no, Some(2));
        assert_eq!(lines[1].new_line_no, None);

        // First added line: no old, new=2
        assert_eq!(lines[2].change_type, ChangeType::Add);
        assert_eq!(lines[2].old_line_no, None);
        assert_eq!(lines[2].new_line_no, Some(2));

        // Second added line: no old, new=3
        assert_eq!(lines[3].change_type, ChangeType::Add);
        assert_eq!(lines[3].old_line_no, None);
        assert_eq!(lines[3].new_line_no, Some(3));
    }

    #[test]
    fn word_diff_detects_changed_segments() {
        let old_tokens = tokenize("let y = 2;");
        let new_tokens = tokenize("let y = 3;");
        let (old_spans, new_spans) = diff_tokens(&old_tokens, &new_tokens);

        // The changed segment should be "2" vs "3"
        let old_changed: Vec<_> = old_spans.iter().filter(|s| s.changed).collect();
        let new_changed: Vec<_> = new_spans.iter().filter(|s| s.changed).collect();
        assert!(!old_changed.is_empty(), "old should have changed spans");
        assert!(!new_changed.is_empty(), "new should have changed spans");
        assert!(
            old_changed.iter().any(|s| s.text.contains('2')),
            "old changed should contain '2'"
        );
        assert!(
            new_changed.iter().any(|s| s.text.contains('3')),
            "new changed should contain '3'"
        );
    }

    #[test]
    fn word_diff_all_same_produces_no_changed() {
        let tokens = tokenize("hello world");
        let (old_spans, new_spans) = diff_tokens(&tokens, &tokens);
        assert!(
            old_spans.iter().all(|s| !s.changed),
            "identical lines should have no changed spans"
        );
        assert!(
            new_spans.iter().all(|s| !s.changed),
            "identical lines should have no changed spans"
        );
    }

    #[test]
    fn side_by_side_alignment_with_additions() {
        let lines = vec![
            DiffLine {
                change_type: ChangeType::Context,
                old_line_no: Some(1),
                new_line_no: Some(1),
                content: "context".to_string(),
                word_spans: vec![],
            },
            DiffLine {
                change_type: ChangeType::Remove,
                old_line_no: Some(2),
                new_line_no: None,
                content: "old".to_string(),
                word_spans: vec![],
            },
            DiffLine {
                change_type: ChangeType::Add,
                old_line_no: None,
                new_line_no: Some(2),
                content: "new1".to_string(),
                word_spans: vec![],
            },
            DiffLine {
                change_type: ChangeType::Add,
                old_line_no: None,
                new_line_no: Some(3),
                content: "new2".to_string(),
                word_spans: vec![],
            },
        ];

        let rows = align_side_by_side(&lines);
        assert_eq!(rows.len(), 3, "context + 2 aligned rows");

        // Row 0: context on both sides
        assert!(rows[0].left.is_some());
        assert!(rows[0].right.is_some());

        // Row 1: remove paired with first add
        assert_eq!(rows[1].left.as_ref().unwrap().content, "old");
        assert_eq!(rows[1].right.as_ref().unwrap().content, "new1");

        // Row 2: no left (blank), second add on right
        assert!(rows[2].left.is_none());
        assert_eq!(rows[2].right.as_ref().unwrap().content, "new2");
    }

    #[test]
    fn tokenize_splits_on_punctuation() {
        let tokens = tokenize("fn foo(bar: u32)");
        assert!(tokens.contains(&"fn"));
        assert!(tokens.contains(&"foo"));
        assert!(tokens.contains(&"("));
        assert!(tokens.contains(&"bar"));
        assert!(tokens.contains(&":"));
        assert!(tokens.contains(&"u32"));
        assert!(tokens.contains(&")"));
    }

    #[test]
    fn parse_empty_diff_produces_no_hunks() {
        let diff = parse_unified_diff("empty.rs", "");
        assert!(diff.hunks.is_empty());
        assert_eq!(diff.additions, 0);
        assert_eq!(diff.deletions, 0);
    }

    #[test]
    fn merge_spans_combines_adjacent_same_flag() {
        let input = vec![
            WordSpan {
                text: "a".to_string(),
                changed: true,
            },
            WordSpan {
                text: "b".to_string(),
                changed: true,
            },
            WordSpan {
                text: "c".to_string(),
                changed: false,
            },
        ];
        let mut output = Vec::new();
        merge_spans(&input, &mut output);
        assert_eq!(output.len(), 2);
        assert_eq!(output[0].text, "ab");
        assert!(output[0].changed);
        assert_eq!(output[1].text, "c");
        assert!(!output[1].changed);
    }
}
