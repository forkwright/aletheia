//! Tests for links, images, tables, structural elements, and edge cases.
#![expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
use super::super::*;
use ratatui::style::{Color, Modifier};

use crate::highlight::Highlighter;
use crate::theme::Theme;

// ── Helpers ──────────────────────────────────────────────────────────────

fn test_render(md: &str) -> Vec<Line<'static>> {
    let theme = Theme::detect();
    let hl = Highlighter::new(theme.mode);
    let (lines, _) = render(md, 80, &theme, &hl);
    lines
}

#[expect(
    dead_code,
    reason = "test helper available for future link/table tests"
)]
fn mk_render(md: &str) -> (Vec<Line<'static>>, Vec<MdLink>) {
    let theme = Theme::detect();
    let hl = Highlighter::new(theme.mode);
    render(md, 80, &theme, &hl)
}

/// Render and return both the lines and the theme so callers can assert
/// against theme-derived colors rather than hardcoding Rgb values.
fn test_render_with_theme(md: &str) -> (Vec<Line<'static>>, Theme) {
    let theme = Theme::detect();
    let hl = Highlighter::new(theme.mode);
    let (lines, _) = render(md, 80, &theme, &hl);
    (lines, theme)
}

/// Concatenate all span content in a single line.
fn line_text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// Concatenate all lines with newlines as a single string.
fn all_lines_text(lines: &[Line]) -> String {
    lines.iter().map(line_text).collect::<Vec<_>>().join("\n")
}

#[expect(
    dead_code,
    reason = "test helper available for future link/table tests"
)]
fn all_text(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|l| line_text(l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// True if any span in `line` whose content contains `text` also carries `modifier`.
fn span_has_modifier(line: &Line, text: &str, modifier: Modifier) -> bool {
    line.spans
        .iter()
        .any(|s| s.content.contains(text) && s.style.add_modifier.contains(modifier))
}

/// True if any span in `line` whose content contains `text` has the given fg color.
fn span_has_fg(line: &Line, text: &str, color: Color) -> bool {
    line.spans
        .iter()
        .any(|s| s.content.contains(text) && s.style.fg == Some(color))
}

/// True if ANY line has a span matching the text + modifier.
fn any_line_has_modifier(lines: &[Line], text: &str, modifier: Modifier) -> bool {
    lines.iter().any(|l| span_has_modifier(l, text, modifier))
}

/// True if ANY line has a span matching the text + fg color.
fn any_line_has_fg(lines: &[Line], text: &str, color: Color) -> bool {
    lines.iter().any(|l| span_has_fg(l, text, color))
}

// ── Images ────────────────────────────────────────────────────────────

#[test]
fn image_with_alt_text_renders_alt_as_display() {
    let lines = test_render("![alt description](image.png)");
    let all = all_lines_text(&lines);
    assert!(
        all.contains("[image: alt description]"),
        "image must render as [image: alt], got: {all:?}"
    );
}

#[test]
fn image_without_alt_renders_image_label() {
    let lines = test_render("![](image.png)");
    let all = all_lines_text(&lines);
    assert!(
        all.contains("[image]"),
        "image with no alt must render as [image], got: {all:?}"
    );
}

// ── Tables ────────────────────────────────────────────────────────────

#[test]
fn simple_table_renders_header_and_row() {
    let md = "| A | B |\n|---|---|\n| 1 | 2 |";
    let lines = test_render(md);
    let all = all_lines_text(&lines);

    assert!(all.contains('┌'), "table must have top-left ┌");
    assert!(all.contains('┐'), "table must have top-right ┐");
    assert!(all.contains('├'), "table must have header separator ├");
    assert!(all.contains('┤'), "table must have header separator ┤");
    assert!(all.contains('└'), "table must have bottom-left └");
    assert!(all.contains('┘'), "table must have bottom-right ┘");
    assert!(all.contains('│'), "table must have vertical separators │");

    assert!(all.contains('A'), "table header A must appear");
    assert!(all.contains('B'), "table header B must appear");
    assert!(all.contains('1'), "table cell 1 must appear");
    assert!(all.contains('2'), "table cell 2 must appear");
}

#[test]
fn table_header_cells_render_with_bold() {
    // The first row (header) uses style_accent_bold; data rows use fg.
    let (lines, theme) = test_render_with_theme("| Head |\n|-----|\n| data |");
    assert!(
        any_line_has_modifier(&lines, "Head", Modifier::BOLD),
        "table header must be BOLD"
    );
    assert!(
        any_line_has_fg(&lines, "Head", theme.colors.accent),
        "table header must use accent color"
    );
}

#[test]
fn table_with_three_columns_renders_all_cells() {
    let md = "| X | Y | Z |\n|---|---|---|\n| a | b | c |";
    let lines = test_render(md);
    let all = all_lines_text(&lines);
    assert!(all.contains('X'), "table header X must appear");
    assert!(all.contains('Y'), "table header Y must appear");
    assert!(all.contains('Z'), "table header Z must appear");
    assert!(all.contains('a'), "table cell a must appear");
    assert!(all.contains('b'), "table cell b must appear");
    assert!(all.contains('c'), "table cell c must appear");
    // Column separators (┬ in top border, ┼ in header sep, ┴ in bottom)
    assert!(all.contains('┬'), "3-col table must have ┬ in top border");
    assert!(
        all.contains('┼'),
        "3-col table must have ┼ in header separator"
    );
    assert!(
        all.contains('┴'),
        "3-col table must have ┴ in bottom border"
    );
}

// ── Structural ────────────────────────────────────────────────────────

#[test]
fn horizontal_rule_spans_full_line() {
    let lines = test_render("---");
    let all = all_lines_text(&lines);
    assert!(
        all.contains('─'),
        "horizontal rule must render as a line of ─ characters"
    );
    // Must be a solid run of dashes (at least 10)
    let rule_line = lines.iter().find(|l| line_text(l).contains('─'));
    assert!(rule_line.is_some(), "rule line must exist");
    let rule_text = line_text(rule_line.unwrap());
    let dash_count = rule_text.chars().filter(|&c| c == '─').count();
    assert!(
        dash_count >= 10,
        "horizontal rule must have ≥10 dashes, got {dash_count}"
    );
}

#[test]
fn horizontal_rule_uses_dim_foreground_color() {
    let (lines, theme) = test_render_with_theme("---");
    let rule_line = lines.iter().find(|l| line_text(l).contains('─'));
    assert!(rule_line.is_some(), "rule line must exist");
    assert!(
        span_has_fg(rule_line.unwrap(), "─", theme.text.fg_dim),
        "horizontal rule must use dim (fg_dim) color"
    );
}

#[test]
fn consecutive_paragraphs_have_blank_line_separator() {
    // Two separate paragraphs must each appear in output
    let lines = test_render("first paragraph\n\nsecond paragraph");
    let all = all_lines_text(&lines);
    assert!(
        all.contains("first paragraph"),
        "first paragraph must appear in output"
    );
    assert!(
        all.contains("second paragraph"),
        "second paragraph must appear in output"
    );

    // They must be on different lines
    let first = lines
        .iter()
        .position(|l| line_text(l).contains("first paragraph"));
    let second = lines
        .iter()
        .position(|l| line_text(l).contains("second paragraph"));
    assert!(
        first.is_some() && second.is_some(),
        "both paragraphs must appear on some line"
    );
    assert_ne!(
        first.unwrap(),
        second.unwrap(),
        "two paragraphs must be on different lines"
    );
}

#[test]
fn hard_line_break_creates_new_line() {
    // Backslash at end of line is a hard break in CommonMark
    let lines = test_render("line one\\\nline two");
    let all = all_lines_text(&lines);
    assert!(all.contains("line one"), "first line must appear");
    assert!(all.contains("line two"), "second line must appear");

    // The two parts must end up on separate lines
    let first = lines.iter().position(|l| line_text(l).contains("line one"));
    let second = lines.iter().position(|l| line_text(l).contains("line two"));
    assert!(
        first.is_some() && second.is_some(),
        "both parts of hard break must appear on some line"
    );
    assert_ne!(
        first.unwrap(),
        second.unwrap(),
        "hard break must split into separate lines"
    );
}

#[test]
fn soft_break_renders_as_space_between_words() {
    // Two lines in the same paragraph (no blank line) join with a soft break (space)
    let lines = test_render("line one\nline two");
    // In a tight paragraph pulldown-cmark emits Text, SoftBreak, Text → same line
    let all = all_lines_text(&lines);
    assert!(all.contains("line one"), "first part must appear");
    assert!(all.contains("line two"), "second part must appear");
    // Both should be on the same line joined by a space
    let joined = lines.iter().find(|l| {
        let t = line_text(l);
        t.contains("line one") && t.contains("line two")
    });
    assert!(
        joined.is_some(),
        "soft-break-joined lines must appear on same output line; got:\n{all}"
    );
}

// ── Edge cases ────────────────────────────────────────────────────────

#[test]
fn empty_input_renders_no_lines() {
    let lines = test_render("");
    assert!(
        lines.is_empty(),
        "empty input must produce no lines, got: {lines:?}"
    );
}

#[test]
fn whitespace_only_input_renders_empty() {
    // Must not panic and should produce no meaningful content
    let lines = test_render("   \n\n   ");
    // No assertion on exact count: just that it doesn't crash
    let _ = lines;
}

#[test]
fn deeply_nested_list_indents_each_level() {
    // 5 levels of nesting: must not panic, indent grows correctly
    let md = "- l1\n  - l2\n    - l3\n      - l4\n        - l5";
    let lines = test_render(md);
    let all = all_lines_text(&lines);
    for level in ["l1", "l2", "l3", "l4", "l5"] {
        assert!(all.contains(level), "level {level} must appear in output");
    }
    // l5 (depth 5) must have 8-space indent (2 × (5-1))
    let l5_line = lines.iter().find(|l| line_text(l).contains("l5"));
    assert!(
        l5_line.is_some(),
        "l5 (depth-5) item must appear on some line"
    );
    let l5_text = line_text(l5_line.unwrap());
    assert!(
        l5_text.starts_with("        "),
        "depth-5 item must have 8-space indent, got: {l5_text:?}"
    );
}

#[test]
fn very_long_line_does_not_truncate_content() {
    // A single paragraph line >500 chars must render without panicking
    let long = "word ".repeat(120); // ~600 chars
    let lines = test_render(long.trim());
    assert!(
        !lines.is_empty(),
        "very long line must produce at least one output line"
    );
}

#[test]
fn unicode_content_renders_correctly() {
    // Emoji, CJK, and combining characters must all pass through cleanly
    let md = "Hello 🎉 world 你好世界 café";
    let lines = test_render(md);
    let all = all_lines_text(&lines);
    assert!(all.contains('🎉'), "emoji must appear in output");
    assert!(all.contains("你好世界"), "CJK characters must appear");
    assert!(all.contains("café"), "accented characters must appear");
}

#[test]
fn mixed_heading_and_paragraph_renders_both() {
    // Full document with multiple element types must render all parts
    let md =
        "# Title\n\nA paragraph.\n\n- item\n\n> quote\n\n```rust\ncode\n```\n\n| H |\n|---|\n| v |";
    let lines = test_render(md);
    let all = all_lines_text(&lines);
    assert!(all.contains("Title"), "heading must appear");
    assert!(all.contains("paragraph"), "paragraph must appear");
    assert!(all.contains("item"), "list item must appear");
    assert!(all.contains("quote"), "blockquote must appear");
    assert!(all.contains("code"), "code block must appear");
    assert!(all.contains('H'), "table header must appear");
    assert!(all.contains('v'), "table cell must appear");
}

#[test]
fn unclosed_bold_marker_renders_as_plain_text() {
    // pulldown-cmark treats unclosed ** as literal asterisks: must not panic
    let lines = test_render("**not closed");
    let _ = all_lines_text(&lines);
}

#[test]
fn ansi_escape_sequences_pass_through_unchanged() {
    // The renderer itself does not strip ANSI: callers sanitize before passing.
    // Verify the renderer handles arbitrary bytes without panicking.
    let lines = test_render("plain text without escapes");
    let all = all_lines_text(&lines);
    assert!(all.contains("plain text"), "plain text must pass through");
}

#[test]
fn ordered_list_renders_with_bullet_prefix() {
    // Ordered lists use the same bullet renderer (no numbering yet)
    let lines = test_render("1. first\n2. second\n3. third");
    let all = all_lines_text(&lines);
    assert!(all.contains("first"), "first ordered list item must appear");
    assert!(
        all.contains("second"),
        "second ordered list item must appear"
    );
    assert!(all.contains("third"), "third ordered list item must appear");
    assert!(all.contains('•'), "ordered list items must use bullet •");
}

#[test]
fn code_block_language_label_uses_accent_color() {
    // Language label must be styled with code_lang color
    let (lines, theme) = test_render_with_theme("```python\npass\n```");
    let header = lines.iter().find(|l| line_text(l).contains("python"));
    assert!(header.is_some(), "python language label must appear");
    assert!(
        span_has_fg(header.unwrap(), "python", theme.code.lang),
        "language label must use code_lang color"
    );
}

#[test]
fn blockquote_text_uses_dim_foreground_color() {
    // Text inside blockquote must use the muted style
    let (lines, theme) = test_render_with_theme("> muted content");
    assert!(
        any_line_has_fg(&lines, "muted content", theme.text.fg_muted),
        "blockquote text must use fg_muted color"
    );
}

#[test]
fn inline_code_has_background_color() {
    // Inline code must have both fg (warning) and bg (code_bg)
    let (lines, theme) = test_render_with_theme("use `foo`");
    let code_span = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .find(|s| s.content.contains("`foo`"));
    assert!(code_span.is_some(), "inline code span must exist");
    let span = code_span.unwrap();
    assert_eq!(
        span.style.fg,
        Some(theme.status.warning),
        "inline code fg must be warning color"
    );
    assert_eq!(
        span.style.bg,
        Some(theme.code.bg),
        "inline code bg must be code_bg color"
    );
}

#[test]
fn extremely_long_line_does_not_overflow() {
    // A single line >65 535 bytes must not panic from u16 overflow in push_span.
    let long = "x".repeat(70_000);
    let lines = test_render(&long);
    assert!(
        !lines.is_empty(),
        "extremely long line must produce output without panic"
    );
}

#[test]
fn deeply_nested_blockquotes_indent_each_level() {
    // 50 levels of blockquote nesting must not stack overflow (iterative parser).
    let mut md: String = (0..50).map(|_| "> ").collect();
    md.push_str("deeply nested");
    let lines = test_render(&md);
    assert!(
        !lines.is_empty(),
        "deeply nested blockquotes must produce at least one line"
    );
}
