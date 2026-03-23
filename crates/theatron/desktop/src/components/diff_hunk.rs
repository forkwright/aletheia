//! Diff hunk component: header, collapsible context, and line list.

use dioxus::prelude::*;

use crate::components::diff_line::DiffLineView;
use crate::state::diff::{ChangeType, DiffHunk, DiffLine, DiffViewMode, align_side_by_side};

const HUNK_HEADER_STYLE: &str = "\
    padding: 4px 12px; \
    font-family: monospace; \
    font-size: 12px; \
    color: #888; \
    background: rgba(74, 74, 255, 0.08); \
    border-top: 1px solid #333; \
    border-bottom: 1px solid #333; \
    user-select: none;\
";

const SBS_ROW_STYLE: &str = "\
    display: flex; \
    min-height: 1.5em; \
    font-family: monospace; \
    font-size: 13px; \
    line-height: 1.5;\
";

const SBS_GUTTER_STYLE: &str = "\
    display: inline-block; \
    width: 4ch; \
    text-align: right; \
    padding: 0 4px; \
    color: #555; \
    user-select: none; \
    flex-shrink: 0;\
";

const SBS_CONTENT_STYLE: &str = "\
    white-space: pre; \
    flex: 1; \
    padding: 0 8px; \
    overflow: hidden;\
";

const SBS_DIVIDER_STYLE: &str = "\
    width: 1px; \
    background: #333; \
    flex-shrink: 0;\
";

/// Render a single diff hunk.
#[component]
pub(crate) fn DiffHunkView(hunk: DiffHunk, language: String, mode: DiffViewMode) -> Element {
    let header = format!(
        "@@ -{},{} +{},{} @@ {}",
        hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count, hunk.context_label
    );

    rsx! {
        div {
            div { style: "{HUNK_HEADER_STYLE}", "{header}" }
            match mode {
                DiffViewMode::Unified => rsx! {
                    {render_unified_lines(&hunk.lines, &language)}
                },
                DiffViewMode::SideBySide => rsx! {
                    {render_side_by_side(&hunk, &language)}
                },
            }
        }
    }
}

/// Render lines in unified mode.
fn render_unified_lines(lines: &[DiffLine], language: &str) -> Element {
    rsx! {
        for (i , line) in lines.iter().enumerate() {
            DiffLineView {
                key: "{i}",
                line: line.clone(),
                language: language.to_string(),
            }
        }
    }
}

/// Render lines in side-by-side mode.
fn render_side_by_side(hunk: &DiffHunk, language: &str) -> Element {
    let rows = align_side_by_side(&hunk.lines);

    rsx! {
        for (i , row) in rows.iter().enumerate() {
            div {
                key: "{i}",
                style: "{SBS_ROW_STYLE}",
                // Left side (old)
                {render_sbs_half(row.left.as_ref(), ChangeType::Remove, language)}
                div { style: "{SBS_DIVIDER_STYLE}" }
                // Right side (new)
                {render_sbs_half(row.right.as_ref(), ChangeType::Add, language)}
            }
        }
    }
}

/// Render one half of a side-by-side row.
fn render_sbs_half(line: Option<&DiffLine>, side: ChangeType, _language: &str) -> Element {
    let bg = match line {
        Some(l) => match l.change_type {
            ChangeType::Add => "rgba(34, 197, 94, 0.1)",
            ChangeType::Remove => "rgba(239, 68, 68, 0.1)",
            ChangeType::Context => "transparent",
        },
        None => "rgba(128, 128, 128, 0.05)",
    };

    let line_no = line.and_then(|l| match side {
        ChangeType::Remove => l.old_line_no,
        ChangeType::Add => l.new_line_no,
        ChangeType::Context => l.old_line_no.or(l.new_line_no),
    });

    let line_no_str = line_no.map_or_else(String::new, |n| n.to_string());
    let content = line.map_or("", |l| l.content.as_str());

    rsx! {
        div {
            style: "display: flex; flex: 1; background: {bg};",
            span { style: "{SBS_GUTTER_STYLE}", "{line_no_str}" }
            div {
                style: "{SBS_CONTENT_STYLE}",
                if let Some(l) = line {
                    if !l.word_spans.is_empty() {
                        {render_sbs_word_spans(&l.word_spans, l.change_type)}
                    } else {
                        "{content}"
                    }
                } else {
                    ""
                }
            }
        }
    }
}

/// Render word-level spans in side-by-side mode.
fn render_sbs_word_spans(
    spans: &[crate::state::diff::WordSpan],
    change_type: ChangeType,
) -> Element {
    let changed_bg = match change_type {
        ChangeType::Add => "rgba(34, 197, 94, 0.3)",
        ChangeType::Remove => "rgba(239, 68, 68, 0.3)",
        ChangeType::Context => "transparent",
    };

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
