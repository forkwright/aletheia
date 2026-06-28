//! Slash command palette -- triggered by `/` in the chat input.
//!
//! Reads `Signal<CommandStore>` from context. Floats above the input bar,
//! showing a filtered list of commands as the user types after `/`.
//! Keyboard: up/down to browse, Enter to select, Escape to dismiss.

use dioxus::prelude::*;

use crate::state::commands::{CommandCategory, CommandSource, CommandStore};

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
    box-shadow: var(--shadow-float);\
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

const ROW_DISABLED_STYLE: &str = "\
    display: flex; \
    align-items: baseline; \
    gap: var(--space-3); \
    padding: var(--space-2) var(--space-3); \
    cursor: pointer; \
    font-size: var(--text-sm); \
    color: var(--text-muted); \
    opacity: 0.72;\
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

const CMD_META_STYLE: &str = "\
    color: var(--text-muted); \
    font-size: var(--text-xs); \
    margin-left: auto; \
    flex-shrink: 0;\
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

    let rows: Vec<(
        usize,
        String,
        String,
        CommandCategory,
        CommandSource,
        bool,
        bool,
    )> = {
        let s = store.read();
        s.filtered
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    i,
                    c.name.clone(),
                    c.description.clone(),
                    c.category,
                    c.source.clone(),
                    i == s.cursor,
                    c.disabled_reason.is_some(),
                )
            })
            .collect()
    };

    let preview = store.read().selected().map(|c| c.usage.clone());

    rsx! {
        div {
            style: "{PALETTE_OVERLAY_STYLE}",
            role: "listbox",
            aria_label: "Command palette",

            div { style: "{PALETTE_HEADER_STYLE}", "Commands — ↑↓ navigate · Enter select · Esc dismiss" }

            if rows.is_empty() {
                div { style: "{EMPTY_STYLE}", "No matching commands" }
            } else {
                for (idx , name , desc , category , source , is_active , is_disabled) in rows {
                    {
                        let cmd_name = format!("/{name}");
                        let row_style = if is_disabled {
                            ROW_DISABLED_STYLE
                        } else if is_active {
                            ROW_ACTIVE_STYLE
                        } else {
                            ROW_STYLE
                        };
                        let on_execute = on_execute;
                        let execute_name = name.clone();
                        let meta = command_meta(category, source, is_disabled);
                        rsx! {
                            div {
                                key: "{idx}",
                                style: "{row_style}",
                                role: "option",
                                aria_selected: if is_active { "true" } else { "false" },
                                aria_label: "{cmd_name}: {desc}",
                                onclick: move |_| {
                                    store.write().cursor = idx;
                                    on_execute.call(format!("/{execute_name}"));
                                },
                                span { style: "{CMD_NAME_STYLE}", "{cmd_name}" }
                                span { style: "{CMD_DESC_STYLE}", "{desc}" }
                                span { style: "{CMD_META_STYLE}", "{meta}" }
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

fn command_meta(
    category: CommandCategory,
    source: CommandSource,
    is_disabled: bool,
) -> &'static str {
    if is_disabled {
        return "disabled";
    }

    match (source, category) {
        (CommandSource::Server, _) => "server",
        (_, CommandCategory::Navigation) => "nav",
        (_, CommandCategory::Action) => "action",
        (_, CommandCategory::Server) => "server",
    }
}
