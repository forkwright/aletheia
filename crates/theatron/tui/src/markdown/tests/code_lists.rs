//! Tests for code blocks, lists, and blockquotes.
#![expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
use ratatui::style::{Color, Modifier};

use super::super::*;
use crate::highlight::Highlighter;
use crate::theme::Theme;
fn test_render(md: &str) -> Vec<Line<'static>> {
    let theme = Theme::detect();
    let hl = Highlighter::new(theme.mode);
    let (lines, _) = render(md, 80, &theme, &hl);
    lines
}

#[expect(dead_code, reason = "test helper available for future code/list tests")]
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

#[expect(dead_code, reason = "test helper available for future code/list tests")]
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

// ── Code blocks ───────────────────────────────────────────────────────

#[test]
fn fenced_code_block_rust_renders_syntax_highlighted() {
    let lines = test_render("```rust\nfn main() {}\n```");
    let all = all_lines_text(&lines);
    assert!(all.contains("fn main()"), "Rust code content must appear");
    assert!(all.contains('╭'), "code block must have top border ╭");
    assert!(all.contains('╰'), "code block must have bottom border ╰");
}

#[test]
fn fenced_code_block_python_renders_syntax_highlighted() {
    let lines = test_render("```python\ndef hello():\n    pass\n```");
    let all = all_lines_text(&lines);
    assert!(all.contains("def hello"), "Python code content must appear");
    assert!(all.contains("pass"), "Python code content must appear");
}

#[test]
fn fenced_code_block_without_language_renders_plain() {
    let lines = test_render("```\nplain code\n```");
    let all = all_lines_text(&lines);
    assert!(
        all.contains("plain code"),
        "unlanguaged code block content must appear"
    );
    assert!(
        all.contains('╭'),
        "unlanguaged block must still have top border ╭"
    );
    assert!(
        all.contains('╰'),
        "unlanguaged block must still have bottom border ╰"
    );
}

#[test]
fn fenced_code_block_unknown_language_renders_plain() {
    // Falls back to plain text highlighting; must not panic
    let lines = test_render("```xyzzy_unknown\nsome code\n```");
    let all = all_lines_text(&lines);
    assert!(
        all.contains("some code"),
        "unknown-language code block must render content"
    );
}

#[test]
fn code_block_shows_language_in_header() {
    // The language name must appear in the header line
    let lines = test_render("```rust\nlet x = 1;\n```");
    let header_line = lines.iter().find(|l| line_text(l).contains("rust"));
    assert!(
        header_line.is_some(),
        "code block header must show language name 'rust'"
    );
}

#[test]
fn code_block_has_box_drawing_border() {
    let lines = test_render("```rust\nx\n```");
    let all = all_lines_text(&lines);
    // Top-left corner, vertical bar inside, bottom-left corner
    assert!(all.contains('╭'), "must have top-left ╭");
    assert!(all.contains('│'), "must have vertical bar │");
    assert!(all.contains('╰'), "must have bottom-left ╰");
}

#[test]
fn code_block_preserves_internal_whitespace() {
    let lines = test_render("```\n    indented\n        double\n```");
    let all = all_lines_text(&lines);
    assert!(
        all.contains("    indented"),
        "leading spaces must be preserved"
    );
    assert!(
        all.contains("        double"),
        "deeper indent must be preserved"
    );
}

#[test]
fn empty_code_block_renders_border_only() {
    // Must not panic; produces border lines even with no content
    let lines = test_render("```rust\n```");
    let all = all_lines_text(&lines);
    assert!(
        all.contains('╭'),
        "empty code block must still have top border"
    );
    assert!(
        all.contains('╰'),
        "empty code block must still have bottom border"
    );
}

// ── Lists ─────────────────────────────────────────────────────────────

#[test]
fn unordered_list_item_renders_with_bullet() {
    let lines = test_render("- alpha\n- beta");
    let all = all_lines_text(&lines);
    assert!(
        all.contains('•'),
        "unordered list must use bullet character •"
    );
    assert!(all.contains("alpha"), "first list item must appear");
    assert!(all.contains("beta"), "second list item must appear");
}

#[test]
fn nested_unordered_list_indents_child_items() {
    let md = "- top\n  - nested\n    - deep";
    let lines = test_render(md);
    let all = all_lines_text(&lines);
    assert!(all.contains("top"), "top-level list item must appear");
    assert!(all.contains("nested"), "nested list item must appear");
    assert!(all.contains("deep"), "deeply nested list item must appear");

    // Nested items must be indented (2 spaces per level beyond first)
    let nested_line = lines.iter().find(|l| line_text(l).contains("nested"));
    assert!(nested_line.is_some(), "nested item line must exist");
    let nested_text = line_text(nested_line.unwrap());
    assert!(
        nested_text.starts_with("  "),
        "depth-2 item must have 2-space indent, got: {nested_text:?}"
    );

    let deep_line = lines.iter().find(|l| line_text(l).contains("deep"));
    assert!(deep_line.is_some(), "deep item line must exist");
    let deep_text = line_text(deep_line.unwrap());
    assert!(
        deep_text.starts_with("    "),
        "depth-3 item must have 4-space indent, got: {deep_text:?}"
    );
}

#[test]
fn list_item_with_bold_text_applies_bold_modifier() {
    let lines = test_render("- **bold item**\n- *italic item*");
    assert!(
        any_line_has_modifier(&lines, "bold item", Modifier::BOLD),
        "bold text inside list must carry BOLD modifier"
    );
    assert!(
        any_line_has_modifier(&lines, "italic item", Modifier::ITALIC),
        "italic text inside list must carry ITALIC modifier"
    );
}

// ── Blockquotes ───────────────────────────────────────────────────────

#[test]
fn blockquote_renders_with_vertical_bar_prefix() {
    // Regression for blockquote border bug: │ and content must be on the SAME line.
    let lines = test_render("> hello");
    let all = all_lines_text(&lines);
    assert!(all.contains("hello"), "blockquote content must appear");

    let border_line = lines.iter().find(|l| line_text(l).contains('│'));
    assert!(
        border_line.is_some(),
        "blockquote border │ must appear; lines: {lines:?}"
    );

    let border_text = line_text(border_line.unwrap());
    assert!(
        border_text.contains("hello"),
        "│ border and content must be on the SAME line, got: {border_text:?}"
    );
}

#[test]
fn blockquote_border_uses_accent_foreground_color() {
    // The │ border span must use theme.borders.normal color.
    let (lines, theme) = test_render_with_theme("> check color");
    assert!(
        any_line_has_fg(&lines, "│ ", theme.borders.normal),
        "blockquote border │ must use theme.borders.normal color"
    );
}

#[test]
fn blockquote_with_bold_content_applies_bold_modifier() {
    let lines = test_render("> **bold inside**");
    assert!(
        any_line_has_modifier(&lines, "bold inside", Modifier::BOLD),
        "bold inside blockquote must carry BOLD modifier"
    );
    // Border must still be present on the same line
    let border_line = lines.iter().find(|l| line_text(l).contains('│'));
    assert!(
        border_line.is_some(),
        "border line must exist in bold blockquote"
    );
    assert!(
        line_text(border_line.unwrap()).contains("bold inside"),
        "border and bold content must be on the same line"
    );
}

#[test]
fn nested_blockquote_indents_inner_content() {
    // Two levels of blockquote should produce two │ characters on the content line.
    let lines = test_render("> > deeply nested");
    let all = all_lines_text(&lines);
    assert!(
        all.contains("deeply nested"),
        "nested blockquote content must appear"
    );

    let content_line = lines
        .iter()
        .find(|l| line_text(l).contains("deeply nested"));
    assert!(content_line.is_some(), "content line must exist");
    let text = line_text(content_line.unwrap());
    let border_count = text.matches('│').count();
    assert!(
        border_count >= 2,
        "nested blockquote must have at least 2 │ characters on content line, got: {text:?}"
    );
}

// ── Links ─────────────────────────────────────────────────────────────

#[test]
fn link_text_renders_with_underline_modifier() {
    let lines = test_render("[click here](https://example.com)");
    assert!(
        any_line_has_modifier(&lines, "click here", Modifier::UNDERLINED),
        "link text must carry UNDERLINED modifier"
    );
}

#[test]
fn link_appends_url_after_display_text() {
    let lines = test_render("[text](https://example.com)");
    let all = all_lines_text(&lines);
    assert!(all.contains("text"), "link display text must appear");
    assert!(
        all.contains("https://example.com"),
        "link URL must be appended in output"
    );
    // URL is wrapped in parens
    assert!(
        all.contains("(https://example.com)"),
        "link URL must appear in parentheses"
    );
}

#[test]
fn link_url_uses_dim_foreground_color() {
    let (lines, theme) = test_render_with_theme("[click](https://example.com)");
    assert!(
        any_line_has_fg(&lines, "https://example.com", theme.text.fg_dim),
        "link URL must use dim (fg_dim) color"
    );
}
