//! Single diff line component with gutter, change indicator, and word-level highlighting.

use dioxus::prelude::*;

use crate::components::code_block::highlight_code;
use crate::state::diff::{ChangeType, DiffLine, WordSpan};

const LINE_STYLE: &str = "\
    display: flex; \
    min-height: 1.5em; \
    font-family: monospace; \
    font-size: 13px; \
    line-height: 1.5;\
";

const GUTTER_STYLE: &str = "\
    display: flex; \
    gap: 0; \
    flex-shrink: 0; \
    user-select: none;\
";

const GUTTER_NUM_STYLE: &str = "\
    display: inline-block; \
    width: 4ch; \
    text-align: right; \
    padding: 0 4px; \
    color: #555;\
";

const INDICATOR_STYLE: &str = "\
    display: inline-block; \
    width: 2ch; \
    text-align: center; \
    flex-shrink: 0; \
    user-select: none;\
";

const CONTENT_STYLE: &str = "\
    white-space: pre; \
    flex: 1; \
    padding: 0 8px;\
";

/// Background color for the entire line based on change type.
fn line_bg(change_type: ChangeType) -> &'static str {
    match change_type {
        ChangeType::Context => "transparent",
        ChangeType::Add => "rgba(34, 197, 94, 0.1)",
        ChangeType::Remove => "rgba(239, 68, 68, 0.1)",
    }
}

/// Stronger background for word-level changed spans.
fn word_changed_bg(change_type: ChangeType) -> &'static str {
    match change_type {
        ChangeType::Context => "transparent",
        ChangeType::Add => "rgba(34, 197, 94, 0.3)",
        ChangeType::Remove => "rgba(239, 68, 68, 0.3)",
    }
}

/// Change indicator character.
fn indicator_char(change_type: ChangeType) -> &'static str {
    match change_type {
        ChangeType::Context => " ",
        ChangeType::Add => "+",
        ChangeType::Remove => "-",
    }
}

/// Indicator text color.
fn indicator_color(change_type: ChangeType) -> &'static str {
    match change_type {
        ChangeType::Context => "#555",
        ChangeType::Add => "#22c55e",
        ChangeType::Remove => "#ef4444",
    }
}

/// Render a single diff line with gutter, indicator, and highlighted content.
#[component]
pub(crate) fn DiffLineView(line: DiffLine, language: String) -> Element {
    let bg = line_bg(line.change_type);
    let old_no = line.old_line_no.map_or_else(String::new, |n| n.to_string());
    let new_no = line.new_line_no.map_or_else(String::new, |n| n.to_string());
    let ind = indicator_char(line.change_type);
    let ind_color = indicator_color(line.change_type);

    rsx! {
        div {
            style: "{LINE_STYLE} background: {bg};",
            div {
                style: "{GUTTER_STYLE}",
                span { style: "{GUTTER_NUM_STYLE}", "{old_no}" }
                span { style: "{GUTTER_NUM_STYLE}", "{new_no}" }
            }
            span {
                style: "{INDICATOR_STYLE} color: {ind_color};",
                "{ind}"
            }
            div {
                style: "{CONTENT_STYLE}",
                if line.word_spans.is_empty() {
                    // NOTE: No word-level diff — render with syntax highlighting.
                    {render_highlighted_content(&line.content, &language)}
                } else {
                    // NOTE: Word-level diff present — render spans with change markers.
                    {render_word_spans(&line.word_spans, line.change_type)}
                }
            }
        }
    }
}

/// Render content with syntax highlighting via syntect.
fn render_highlighted_content(content: &str, language: &str) -> Element {
    let line_with_newline = format!("{content}\n");
    let highlighted = highlight_code(&line_with_newline, language);

    if let Some(spans) = highlighted.first() {
        rsx! {
            for (i , span) in spans.iter().enumerate() {
                span {
                    key: "{i}",
                    style: "color: {span.color};{bold_css(span.bold)}{italic_css(span.italic)}",
                    "{span.text}"
                }
            }
        }
    } else {
        rsx! { "{content}" }
    }
}

/// Render word-diff spans with changed segments highlighted.
fn render_word_spans(spans: &[WordSpan], change_type: ChangeType) -> Element {
    let changed_bg = word_changed_bg(change_type);

    rsx! {
        for (i , span) in spans.iter().enumerate() {
            if span.changed {
                span {
                    key: "{i}",
                    style: "background: {changed_bg}; border-radius: 2px;",
                    "{span.text}"
                }
            } else {
                span {
                    key: "{i}",
                    "{span.text}"
                }
            }
        }
    }
}

fn bold_css(bold: bool) -> &'static str {
    if bold { " font-weight: bold;" } else { "" }
}

fn italic_css(italic: bool) -> &'static str {
    if italic { " font-style: italic;" } else { "" }
}
