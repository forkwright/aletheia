//! Slash command palette -- triggered by `/` in the chat input.
//!
//! Reads `Signal<CommandStore>` from context. Floats above the input bar,
//! showing a filtered list of commands as the user types after `/`.
//! Keyboard: up/down to browse, Enter to select, Escape to dismiss.

use dioxus::prelude::*;

use crate::state::commands::CommandStore;

const PALETTE_OVERLAY_STYLE: &str = "\
    position: absolute; \
    bottom: 64px; \
    left: var(--space-4); \
    right: var(--space-4); \
    background: var(--bg-surface); \
    border: 1px solid var(--accent); \
    border-radius: var(--radius-md); \
    max-height: 260px; \
    overflow-y: auto; \
    z-index: 100; \
    box-shadow: var(--shadow-lg);\
";

const PALETTE_HEADER_STYLE: &str = "\
    padding: var(--space-2) var(--space-3) var(--space-1); \
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    border-bottom: 1px solid var(--border);\
";

const ROW_STYLE: &str = "\
    display: flex; \
    align-items: baseline; \
    gap: var(--space-3); \
    padding: var(--space-2) var(--space-3); \
    cursor: pointer; \
    font-size: var(--text-sm); \
    color: var(--text-primary); \
    transition: background-color var(--transition-quick);\
";

const ROW_ACTIVE_STYLE: &str = "\
    display: flex; \
    align-items: baseline; \
    gap: var(--space-3); \
    padding: var(--space-2) var(--space-3); \
    cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); \
    font-size: var(--text-sm); \
    color: var(--text-primary); \
    background: var(--bg-surface-bright);\
";

const CMD_NAME_STYLE: &str = "\
    font-family: var(--font-mono); \
    color: var(--accent); \
    min-width: 120px; \
    flex-shrink: 0;\
";

const CMD_DESC_STYLE: &str = "\
    color: var(--text-secondary); \
    font-size: var(--text-xs);\
";

const PREVIEW_STYLE: &str = "\
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    border-top: 1px solid var(--border); \
    font-family: var(--font-mono);\
";

const EMPTY_STYLE: &str = "\
    padding: var(--space-3); \
    color: var(--text-muted); \
    font-size: var(--text-sm); \
    text-align: center;\
";

/// Slash command palette component.
///
/// - `is_open`: whether the palette is currently visible
/// - `on_execute`: called with the full command string when the user picks one
///
/// Reads `Signal<CommandStore>` from context.
#[component]
pub(crate) fn CommandPaletteView(is_open: bool, on_execute: EventHandler<String>) -> Element {
    let mut store = use_context::<Signal<CommandStore>>();

    if !is_open {
        return rsx! { div {} };
    }

    let rows: Vec<(usize, String, String, bool)> = {
        let s = store.read();
        s.filtered
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.name.clone(), c.description.clone(), i == s.cursor))
            .collect()
    };

    let preview = store.read().selected().map(|c| c.usage.clone());

    rsx! {
        div {
            style: "{PALETTE_OVERLAY_STYLE}",

            div { style: "{PALETTE_HEADER_STYLE}", "Commands — ↑↓ navigate · Enter select · Esc dismiss" }

            if rows.is_empty() {
                div { style: "{EMPTY_STYLE}", "No matching commands" }
            } else {
                for (idx , name , desc , is_active) in rows {
                    {
                        let cmd_name = format!("/{name}");
                        let row_style = if is_active { ROW_ACTIVE_STYLE } else { ROW_STYLE };
                        let on_execute = on_execute;
                        rsx! {
                            div {
                                key: "{idx}",
                                style: "{row_style}",
                                onclick: move |_| {
                                    store.write().cursor = idx;
                                    on_execute.call(format!("/{name}"));
                                },
                                span { style: "{CMD_NAME_STYLE}", "{cmd_name}" }
                                span { style: "{CMD_DESC_STYLE}", "{desc}" }
                            }
                        }
                    }
                }
            }

            if let Some(usage) = preview {
                div { style: "{PREVIEW_STYLE}", "{usage}" }
            }
        }
    }
}
