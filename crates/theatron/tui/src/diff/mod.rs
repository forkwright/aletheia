//! Tool execution diff viewer.

mod parse;
mod render;
mod types;

pub(crate) use parse::parse_git_diff;
#[cfg(test)]
pub(crate) use render::render_diff_view;
pub(crate) use render::render_diff_view_immutable;
pub(crate) use types::{DiffViewState, compute_diff};

#[cfg(test)]
use crate::theme::Theme;
#[cfg(test)]
pub(crate) use parse::{parse_hunk_header, truncate_str};
#[cfg(test)]
pub(crate) use render::{render_side_by_side, render_unified, render_word_diff};
#[cfg(test)]
pub(crate) use types::{DiffChange, DiffHunk, DiffMode, FileDiff, collapse_to_replacements};

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::text::Span;

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
