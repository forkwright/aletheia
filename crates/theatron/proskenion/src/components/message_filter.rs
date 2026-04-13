//! Search/filter overlay bar for the chat message list.
//!
//! Embeds at the top of the chat area and filters messages by substring match
//! with live match counts. Designed to mirror the TUI filter bar in the desktop
//! app.

use dioxus::prelude::*;

const BAR_STYLE: &str = "\
    position: sticky; \
    top: 0; \
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-2) var(--space-3); \
    background: var(--bg-surface); \
    border-bottom: 1px solid var(--border); \
    z-index: 10;\
";

const INPUT_STYLE: &str = "\
    flex: 1; \
    background: var(--input-bg); \
    border: 1px solid var(--input-border); \
    border-radius: var(--radius-md); \
    color: var(--text-primary); \
    padding: var(--space-1) var(--space-2); \
    font-size: var(--text-sm); \
    font-family: var(--font-body); \
    outline: none;\
";

const MATCH_COUNT_STYLE: &str = "\
    color: var(--text-muted); \
    font-size: var(--text-xs); \
    white-space: nowrap;\
";

const CLOSE_BTN_STYLE: &str = "\
    background: none; \
    border: none; \
    color: var(--text-muted); \
    cursor: pointer; \
    font-size: var(--text-sm); \
    padding: var(--space-1); \
    line-height: 1; \
    transition: color var(--transition-quick);\
";

/// Message filter bar component.
///
/// Renders a search input with live match counts and a close button.
/// The parent component is responsible for filtering logic — this component
/// only drives the query signal and displays counts.
///
/// # Props
///
/// - `query` — Signal holding the current filter text (two-way bound).
/// - `match_count` — Number of messages matching the current query.
/// - `total_count` — Total number of messages in the session.
/// - `on_close` — Fired when the user dismisses the filter bar.
#[component]
pub(crate) fn MessageFilterBar(
    mut query: Signal<String>,
    match_count: usize,
    total_count: usize,
    on_close: EventHandler,
) -> Element {
    rsx! {
        div {
            style: "{BAR_STYLE}",
            role: "search",
            aria_label: "Filter messages",

            // Search icon
            span {
                style: "color: var(--text-muted); font-size: var(--text-sm); flex-shrink: 0;",
                aria_hidden: "true",
                "\u{1f50d}"
            }

            // Filter input
            input {
                style: "{INPUT_STYLE}",
                r#type: "text",
                placeholder: "Filter messages...",
                aria_label: "Filter messages",
                value: "{query}",
                oninput: move |evt: Event<FormData>| query.set(evt.value()),
                autofocus: true,
            }

            // Match count
            span {
                style: "{MATCH_COUNT_STYLE}",
                aria_live: "polite",
                "{match_count} of {total_count}"
            }

            // Close button
            button {
                style: "{CLOSE_BTN_STYLE}",
                aria_label: "Close search",
                onclick: move |_| on_close.call(()),
                "\u{2715}"
            }
        }
    }
}
