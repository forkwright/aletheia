//! Tool execution diff viewer.
//!
//! Structural diff parsing and types are adopted from `gramma` (the
//! theatron crate that owns git-diff parsing); this module keeps only
//! what's koilon-specific: rendering (`render.rs`), the 3-way view mode
//! and scroll state (`types.rs`), and a `similar`-backed text-to-diff
//! adapter for the "diff a tool result's before/after content" path that
//! gramma has no equivalent for.

mod render;
mod types;

pub(crate) use gramma::diff::parse_git_diff;
#[cfg(test)]
pub(crate) use render::render_diff_view;
pub(crate) use render::render_diff_view_immutable;
pub(crate) use types::{DiffViewState, compute_diff};

#[cfg(test)]
use crate::theme::Theme;
#[cfg(test)]
use render::pad_to;
#[cfg(test)]
pub(crate) use render::{
    RenderLine, collapse_to_replacements, render_side_by_side, render_unified, render_word_diff,
};
#[cfg(test)]
use types::DiffMode;

#[cfg(test)]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing for clarity"
)]
mod tests {
    use gramma::diff::{ChangeType, DiffFile, DiffLine};
    use ratatui::layout::Rect;
    use ratatui::text::Span;
    use unicode_width::UnicodeWidthStr;

    use super::*;

    fn default_theme() -> Theme {
        Theme::detect()
    }

    // ── DiffMode ──

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

    // ── compute_diff ──

    #[test]
    fn compute_diff_no_changes() {
        let diff = compute_diff("test.rs", "hello\n", "hello\n");
        assert!(diff.hunks.is_empty());
    }

    #[test]
    fn compute_diff_simple_addition() {
        let diff = compute_diff("test.rs", "line1\n", "line1\nline2\n");
        assert!(!diff.hunks.is_empty());
        let lines = &diff.hunks[0].lines;
        assert!(lines.iter().any(|l| l.change_type == ChangeType::Add));
    }

    #[test]
    fn compute_diff_simple_deletion() {
        let diff = compute_diff("test.rs", "line1\nline2\n", "line1\n");
        assert!(!diff.hunks.is_empty());
        let lines = &diff.hunks[0].lines;
        assert!(lines.iter().any(|l| l.change_type == ChangeType::Remove));
    }

    #[test]
    fn compute_diff_modification() {
        let diff = compute_diff("test.rs", "old line\n", "new line\n");
        assert!(!diff.hunks.is_empty());
        let lines = &diff.hunks[0].lines;
        assert!(lines.iter().any(|l| l.change_type == ChangeType::Remove));
        assert!(lines.iter().any(|l| l.change_type == ChangeType::Add));
    }

    #[test]
    fn compute_diff_preserves_path() {
        let diff = compute_diff("src/main.rs", "a\n", "b\n");
        assert_eq!(diff.path, "src/main.rs");
    }

    // ── collapse_to_replacements ──

    fn diff_line(change_type: ChangeType, content: &str) -> DiffLine {
        DiffLine::new(change_type, Some(1), Some(1), content, Vec::new())
    }

    #[test]
    fn collapse_pairs_delete_insert_to_replace() {
        let lines = vec![
            diff_line(ChangeType::Remove, "old"),
            diff_line(ChangeType::Add, "new"),
        ];
        let collapsed = collapse_to_replacements(&lines);
        assert_eq!(collapsed.len(), 1);
        assert!(matches!(collapsed[0], RenderLine::Replace { .. }));
    }

    #[test]
    fn collapse_leaves_standalone_delete() {
        let lines = vec![diff_line(ChangeType::Remove, "removed")];
        let collapsed = collapse_to_replacements(&lines);
        assert!(matches!(&collapsed[0], RenderLine::Single(l) if l.change_type == ChangeType::Remove));
    }

    #[test]
    fn collapse_leaves_standalone_insert() {
        let lines = vec![diff_line(ChangeType::Add, "added")];
        let collapsed = collapse_to_replacements(&lines);
        assert!(matches!(&collapsed[0], RenderLine::Single(l) if l.change_type == ChangeType::Add));
    }

    #[test]
    fn replace_line_carries_old_and_new_content() {
        let lines = vec![
            diff_line(ChangeType::Remove, "before"),
            diff_line(ChangeType::Add, "after"),
        ];
        let collapsed = collapse_to_replacements(&lines);
        let RenderLine::Replace { old, new } = &collapsed[0] else {
            panic!("expected RenderLine::Replace");
        };
        assert_eq!(old.content, "before");
        assert_eq!(new.content, "after");
    }

    // ── Unified rendering ──

    #[test]
    fn unified_render_has_file_header() {
        let theme = default_theme();
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
        let theme = default_theme();
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
        let theme = default_theme();
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

    // ── Side-by-side rendering ──

    #[test]
    fn side_by_side_render_has_header() {
        let theme = default_theme();
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
        let theme = default_theme();
        let diff = compute_diff("test.rs", "old content\n", "new content\n");
        for width in [40, 60, 80, 120, 200] {
            let lines = render_side_by_side(&diff, width, &theme);
            assert!(!lines.is_empty(), "Failed at width {width}");
        }
    }

    // ── Word diff rendering ──

    #[test]
    fn word_diff_render_has_header() {
        let theme = default_theme();
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
        let theme = default_theme();
        let diff = compute_diff("test.rs", "let x = 42;\n", "let x = 99;\n");
        let lines = render_word_diff(&diff, &theme);
        let all_spans: Vec<&Span> = lines.iter().flat_map(|l| l.spans.iter()).collect();
        assert!(
            all_spans.len() > 2,
            "Expected multiple spans for word-level diff"
        );
    }

    // ── DiffViewState ──

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

    // ── parse_git_diff ──

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
        let lines = &files[0].hunks[0].lines;
        assert!(lines.iter().any(|l| l.change_type == ChangeType::Remove));
        assert!(lines.iter().any(|l| l.change_type == ChangeType::Add));
        assert!(lines.iter().any(|l| l.change_type == ChangeType::Context));
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

    // ── render_diff_view ──

    #[test]
    fn render_diff_view_empty_shows_no_changes() {
        let theme = default_theme();
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
        let theme = default_theme();
        let diff = compute_diff("test.rs", "old line\n", "new line\n");
        let mut state = DiffViewState::new(vec![diff]);
        let area = Rect::new(0, 0, 80, 24);
        render_diff_view(&mut state, area, &theme);
        assert!(state.total_lines > 0);
    }

    // ── Large diff ──

    #[test]
    fn large_diff_renders_without_panic() {
        let theme = default_theme();
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

    // ── Content truncation ──

    #[test]
    fn side_by_side_keeps_multibyte_line_within_column_budget() {
        // WHY(#6542): at width 80 the content budget is 32 columns. This line is 16
        // chars, 48 bytes and exactly 32 columns, so it fills the budget precisely
        // and must render whole. A byte-length guard truncated it with an ellipsis
        // at well under the budget it was asked to respect.
        let theme = default_theme();
        let content = "日本語のテキストはここにあります";
        assert_eq!(UnicodeWidthStr::width(content), 32);
        assert!(content.len() > 32, "must exceed the budget in bytes");

        let diff = compute_diff("test.rs", &format!("{content}\n"), "changed\n");
        let rendered: String = render_side_by_side(&diff, 80, &theme)
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|s| s.content.to_string())
            .collect();

        assert!(
            rendered.contains(content),
            "content within the column budget must render whole, got: {rendered}"
        );
        assert!(
            !rendered.contains('…'),
            "no ellipsis is expected below the budget, got: {rendered}"
        );
    }

    #[test]
    fn side_by_side_separator_sits_at_the_half_width_column() {
        // WHY(#6542): the panes are laid out in terminal columns, but truncation and
        // padding both measured chars. A CJK char is one char and two columns, so a
        // line of them built a left pane twice as wide as its half and pushed the
        // `│` out of the column it marks. ASCII measures the same either way, which
        // is why only non-ASCII content exposes it.
        let theme = default_theme();
        let content = "日本語のテキストはここにあります";
        let diff = compute_diff("test.rs", &format!("{content}\n"), "changed\n");

        for line in render_side_by_side(&diff, 80, &theme) {
            let Some(sep) = line.spans.iter().position(|s| s.content.as_ref() == "│") else {
                continue;
            };
            let left: String = line.spans[..sep]
                .iter()
                .map(|s| s.content.as_ref())
                .collect();
            assert_eq!(
                UnicodeWidthStr::width(left.as_str()),
                40,
                "left pane must occupy exactly half of width 80, got: {left}"
            );
        }
    }

    // ── pad_to ──

    #[test]
    fn pad_to_leaves_string_that_already_fills_the_columns() {
        // WHY(#6542): 3 chars, 9 bytes, 6 columns. It already fills a 6-column
        // budget exactly, so no padding is due. Counting chars instead appended
        // three spaces and overran the pane by three columns.
        assert_eq!(pad_to("日本語".to_owned(), 6), "日本語");
    }

    #[test]
    fn pad_to_truncates_by_columns_not_chars() {
        // WHY(#6542): 6 chars but 12 columns. A 3-column budget fits one CJK char
        // plus the ellipsis; taking 3 chars would have rendered 6 columns.
        assert_eq!(pad_to("日本語テスト".to_owned(), 3), "日…");
    }

    #[test]
    fn pad_to_leaves_exact_width_unchanged() {
        assert_eq!(pad_to("abc".to_owned(), 3), "abc");
    }

    #[test]
    fn pad_to_pads_ascii_to_width() {
        assert_eq!(pad_to("ab".to_owned(), 4), "ab  ");
    }

    // ── File path display ──

    #[test]
    fn file_path_displayed_in_all_modes() {
        let theme = default_theme();
        let diff = compute_diff("src/important.rs", "old\n", "new\n");

        for render_fn in [
            |f: &DiffFile, t: &Theme| render_unified(f, t),
            |f: &DiffFile, t: &Theme| render_word_diff(f, t),
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

    // ── FileDiff / DiffHunk fields ──

    #[test]
    fn file_diff_path_preserved() {
        let diff = compute_diff("crates/foo/src/lib.rs", "a\n", "b\n");
        assert_eq!(
            diff.path, "crates/foo/src/lib.rs",
            "FileDiff must record file path"
        );
    }

    #[test]
    fn diff_hunk_line_numbers_are_one_indexed() {
        // unified diff spec: hunk line numbers are 1-indexed
        let diff = compute_diff("test.rs", "line1\nline2\n", "line1\nchanged\n");
        assert!(!diff.hunks.is_empty(), "expected at least one hunk");
        let hunk = &diff.hunks[0];
        assert!(hunk.old_start >= 1, "old_start must be >= 1");
        assert!(hunk.new_start >= 1, "new_start must be >= 1");
    }

    // ── DiffViewState additional ──

    #[test]
    fn diff_view_state_initial_mode_is_unified() {
        let state = DiffViewState::new(vec![]);
        assert_eq!(
            state.mode,
            DiffMode::Unified,
            "initial mode must be Unified"
        );
    }

    #[test]
    fn diff_view_state_is_empty_when_all_hunks_empty() {
        let diff = DiffFile {
            path: "empty.rs".to_string(),
            hunks: vec![],
            additions: 0,
            deletions: 0,
            mode: gramma::diff::DiffViewMode::default(),
        };
        let state = DiffViewState::new(vec![diff]);
        assert!(
            state.is_empty(),
            "state with file but no hunks must be empty"
        );
    }

    #[test]
    fn diff_view_state_cycle_mode_resets_scroll() {
        let mut state = DiffViewState::new(vec![]);
        state.total_lines = 100;
        state.scroll_down(20);
        assert_eq!(state.scroll_offset, 20);
        state.cycle_mode();
        assert_eq!(
            state.scroll_offset, 0,
            "cycle_mode must reset scroll to zero"
        );
    }
}
