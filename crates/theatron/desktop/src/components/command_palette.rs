//! Slash command palette — triggered by `/` in the chat input.
//!
//! Reads `Signal<CommandStore>` from context. Floats above the input bar,
//! showing a filtered list of commands as the user types after `/`.
//! Keyboard: up/down to navigate, Enter to select, Escape to dismiss.

use dioxus::prelude::*;

use crate::state::commands::CommandStore;

const PALETTE_OVERLAY_STYLE: &str = "\
    position: absolute; \
    bottom: 64px; \
    left: 16px; \
    right: 16px; \
    background: #1a1a2e; \
    border: 1px solid #4a4aff; \
    border-radius: 8px; \
    max-height: 260px; \
    overflow-y: auto; \
    z-index: 100; \
    box-shadow: 0 -4px 20px rgba(0,0,0,0.6);\
";

const PALETTE_HEADER_STYLE: &str = "\
    padding: 8px 12px 4px; \
    font-size: 11px; \
    color: #555; \
    border-bottom: 1px solid #2a2a3a;\
";

const ROW_STYLE: &str = "\
    display: flex; \
    align-items: baseline; \
    gap: 12px; \
    padding: 8px 12px; \
    cursor: pointer; \
    font-size: 13px; \
    color: #e0e0e0;\
";

const ROW_ACTIVE_STYLE: &str = "\
    display: flex; \
    align-items: baseline; \
    gap: 12px; \
    padding: 8px 12px; \
    cursor: pointer; \
    font-size: 13px; \
    color: #ffffff; \
    background: #2a2a4a;\
";

const CMD_NAME_STYLE: &str = "\
    font-family: monospace; \
    color: #4a4aff; \
    min-width: 120px; \
    flex-shrink: 0;\
";

const CMD_DESC_STYLE: &str = "\
    color: #888; \
    font-size: 12px;\
";

const PREVIEW_STYLE: &str = "\
    padding: 6px 12px; \
    font-size: 11px; \
    color: #555; \
    border-top: 1px solid #2a2a3a; \
    font-family: monospace;\
";

const EMPTY_STYLE: &str = "\
    padding: 12px; \
    color: #555; \
    font-size: 13px; \
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
