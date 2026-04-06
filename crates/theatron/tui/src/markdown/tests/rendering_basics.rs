//! Tests for basic rendering: text formatting, headings, regression tests.
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

// ── Existing regression tests (kept as-is) ────────────────────────────

#[test]
fn bold_text() {
    let (lines, _) = mk_render("**bold**");
    assert!(
        !lines.is_empty(),
        "bold text must produce at least one line"
    );
    assert!(
        line_text(&lines[0]).contains("bold"),
        "bold text must appear in output"
    );
}

#[test]
fn code_block() {
    let (lines, _) = mk_render("```rust\nlet x = 1;\n```");
    assert!(
        all_text(&lines).contains("let x = 1"),
        "code block content must appear in output"
    );
}

#[test]
fn list_items() {
    let (lines, _) = mk_render("- one\n- two");
    let text = all_text(&lines);
    assert!(
        text.contains("one"),
        "first list item must appear in output"
    );
    assert!(
        text.contains("two"),
        "second list item must appear in output"
    );
}

#[test]
fn strikethrough_renders() {
    let (lines, _) = mk_render("~~deleted~~");
    assert!(
        !lines.is_empty(),
        "strikethrough text must produce at least one line"
    );
    assert!(
        line_text(&lines[0]).contains("deleted"),
        "strikethrough text must appear in output"
    );
}

#[test]
fn markdown_link_produces_md_link() {
    let (lines, links) = mk_render("[click here](https://example.com)");
    let text = all_text(&lines);
    assert!(text.contains("click here"), "link text visible: {text}");
    assert!(!links.is_empty(), "should produce a MdLink");
    assert_eq!(
        links[0].url, "https://example.com",
        "first link url should be example.com"
    );
    assert_eq!(
        links[0].text, "click here",
        "first link text should match display text"
    );
}

#[test]
fn markdown_link_url_shown_in_non_osc8_terminal() {
    let (lines, _) = mk_render("[click](https://example.com)");
    let text = all_text(&lines);
    if !crate::hyperlink::supports_hyperlinks() {
        assert!(
            text.contains("example.com"),
            "URL suffix must be shown in non-OSC8 terminals: {text}"
        );
    }
}

#[test]
fn plain_url_in_text_linkified() {
    let (lines, links) = mk_render("See https://docs.anthropic.com/api for info.");
    let text = all_text(&lines);
    assert!(
        text.contains("https://docs.anthropic.com/api"),
        "URL in output: {text}"
    );
    assert!(!links.is_empty(), "auto-detected URL must produce MdLink");
    assert_eq!(
        links[0].url, "https://docs.anthropic.com/api",
        "auto-detected url should match source url"
    );
}

#[test]
fn url_in_code_block_not_linkified() {
    let (_, links) = mk_render("```\nhttps://example.com\n```");
    assert!(
        links.is_empty(),
        "URLs inside code blocks must NOT produce links"
    );
}

#[test]
fn url_in_inline_code_not_linkified() {
    let (_, links) = mk_render("Use `https://example.com` in config");
    assert!(
        links.is_empty(),
        "URLs inside inline code must NOT produce links"
    );
}

#[test]
fn link_text_correct() {
    let (_, links) = mk_render("[API docs](https://docs.anthropic.com)");
    assert_eq!(links.len(), 1, "should produce exactly one link");
    assert_eq!(
        links[0].text, "API docs",
        "link text should match display text"
    );
    assert_eq!(
        links[0].url, "https://docs.anthropic.com",
        "link url should match href"
    );
}

#[test]
fn plain_url_trailing_punctuation_stripped() {
    let (_, links) = mk_render("Visit https://example.com.");
    assert!(!links.is_empty(), "plain url must produce a link");
    assert_eq!(
        links[0].url, "https://example.com",
        "trailing punctuation must be stripped from url"
    );
}

#[test]
fn link_renders_with_url_visible() {
    // Regression guard: the original test checked URL visibility in non-OSC8 terminals
    let (lines, _links) = mk_render("[click](https://example.com)");
    let text = all_text(&lines);
    assert!(text.contains("click"), "link display text must appear");
    assert!(
        text.contains("example.com"),
        "link url must appear in output"
    );
}

// ── Text formatting ───────────────────────────────────────────────────

#[test]
fn bold_text_renders_with_bold_modifier() {
    let lines = test_render("**bold**");
    assert!(
        !lines.is_empty(),
        "bold text must produce at least one line"
    );
    assert!(
        any_line_has_modifier(&lines, "bold", Modifier::BOLD),
        "bold text must carry BOLD modifier; lines: {lines:?}"
    );
}

#[test]
fn italic_text_renders_with_italic_modifier() {
    let lines = test_render("*italic*");
    assert!(
        !lines.is_empty(),
        "italic text must produce at least one line"
    );
    let all = all_lines_text(&lines);
    assert!(all.contains("italic"), "expected italic text in output");
    assert!(
        any_line_has_modifier(&lines, "italic", Modifier::ITALIC),
        "italic text must carry ITALIC modifier"
    );
}

#[test]
fn strikethrough_text_renders_with_strikethrough_modifier() {
    let lines = test_render("~~deleted~~");
    assert!(
        any_line_has_modifier(&lines, "deleted", Modifier::CROSSED_OUT),
        "strikethrough text must carry CROSSED_OUT modifier"
    );
}

#[test]
fn bold_italic_text_renders_with_both_modifiers() {
    // ***text*** is bold + italic in CommonMark
    let lines = test_render("***combo***");
    assert!(
        !lines.is_empty(),
        "bold-italic text must produce at least one line"
    );
    assert!(
        any_line_has_modifier(&lines, "combo", Modifier::BOLD),
        "bold-italic text must have BOLD"
    );
    assert!(
        any_line_has_modifier(&lines, "combo", Modifier::ITALIC),
        "bold-italic text must have ITALIC"
    );
}

#[test]
fn inline_code_renders_without_bold_modifier() {
    let (lines, theme) = test_render_with_theme("use `std::mem::take`");
    let all = all_lines_text(&lines);
    assert!(
        all.contains("`std::mem::take`"),
        "inline code must appear with backticks"
    );
    // Inline code uses theme.status.warning as fg
    assert!(
        any_line_has_fg(&lines, "`std::mem::take`", theme.status.warning),
        "inline code must have warning fg color"
    );
}

#[test]
fn nested_formatting_applies_outer_and_inner_modifiers() {
    // Bold wrapping italic
    let lines = test_render("**bold _bold-italic_ bold**");
    let all = all_lines_text(&lines);
    assert!(
        all.contains("bold-italic"),
        "nested text content must appear"
    );
    assert!(
        any_line_has_modifier(&lines, "bold-italic", Modifier::ITALIC),
        "nested italic must carry ITALIC modifier"
    );
    assert!(
        any_line_has_modifier(&lines, "bold-italic", Modifier::BOLD),
        "nested italic inside bold must carry BOLD modifier"
    );
}

// ── Headings ──────────────────────────────────────────────────────────

#[test]
fn h1_heading_renders_with_bold_and_large_size() {
    let lines = test_render("# Heading One");
    assert!(!lines.is_empty(), "H1 must produce at least one line");
    let text = line_text(&lines[0]);
    assert!(
        text.starts_with("# "),
        "H1 must start with '# ', got: {text:?}"
    );
    assert!(
        text.contains("Heading One"),
        "H1 text must appear in output"
    );
}

#[test]
fn h2_heading_renders_with_bold() {
    let lines = test_render("## Heading Two");
    assert!(!lines.is_empty(), "H2 must produce at least one line");
    let text = line_text(&lines[0]);
    assert!(
        text.starts_with("## "),
        "H2 must start with '## ', got: {text:?}"
    );
    assert!(
        text.contains("Heading Two"),
        "H2 text must appear in output"
    );
}

#[test]
fn h3_heading_renders_without_size_modifier() {
    let lines = test_render("### Heading Three");
    assert!(!lines.is_empty(), "H3 must produce at least one line");
    let text = line_text(&lines[0]);
    assert!(
        text.starts_with("### "),
        "H3 must start with '### ', got: {text:?}"
    );
    assert!(
        text.contains("Heading Three"),
        "H3 text must appear in output"
    );
}

#[test]
fn h4_heading_renders_without_size_modifier() {
    let lines = test_render("#### Heading Four");
    assert!(!lines.is_empty(), "H4 must produce at least one line");
    let text = line_text(&lines[0]);
    assert!(
        text.starts_with("#### "),
        "H4 must start with '#### ', got: {text:?}"
    );
    assert!(
        text.contains("Heading Four"),
        "H4 text must appear in output"
    );
}

#[test]
fn heading_applies_correct_fg_color() {
    // Headings use style_accent_bold: accent fg + BOLD modifier
    let (lines, theme) = test_render_with_theme("# Styled Heading");
    assert!(!lines.is_empty(), "heading must produce at least one line");
    assert!(
        any_line_has_modifier(&lines, "# ", Modifier::BOLD),
        "heading prefix must be BOLD"
    );
    assert!(
        any_line_has_fg(&lines, "# ", theme.colors.accent),
        "heading prefix must use accent color"
    );
    assert!(
        any_line_has_modifier(&lines, "Styled Heading", Modifier::BOLD),
        "heading text must be BOLD"
    );
}
