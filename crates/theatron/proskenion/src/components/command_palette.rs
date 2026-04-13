//! Slash command palette -- triggered by `/` in the chat input.
//!
//! Reads `Signal<CommandStore>` from context. Floats above the input bar,
//! showing a filtered list of commands as the user types after `/`.
//! Keyboard: up/down to browse, Enter to select, Escape to dismiss.

use dioxus::prelude::*;

use crate::app::Route;
use crate::components::chat::ChatState;
use crate::services::export::messages_to_markdown;
use crate::state::chat::{ChatMessage, Role};
use crate::state::commands::{CommandCategory, CommandStore};
use crate::state::toasts::{Severity, ToastStore};

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

/// Dispatch a command by name. Handles navigation routes and the export
/// action internally; forwards everything else to the caller via `on_execute`.
fn dispatch_command(
    name: &str,
    category: CommandCategory,
    nav: &dioxus_router::Navigator,
    on_execute: &EventHandler<String>,
) {
    match category {
        CommandCategory::Navigation => {
            let route = match name {
                "sessions" => Route::Sessions {},
                "memory" => Route::Memory {},
                "metrics" => Route::Metrics {},
                "ops" => Route::Ops {},
                "files" => Route::Files {},
                "planning" => Route::Planning {},
                "settings" => Route::Settings {},
                _ => {
                    on_execute.call(format!("/{name}"));
                    return;
                }
            };
            nav.push(route);
        }
        CommandCategory::Action if name == "export" => {
            // WHY: ChatState is a local signal in chat.rs, not a global context.
            // Use try_consume_context so export gracefully handles being called
            // from a non-chat view.
            let Some(legacy_state) = try_consume_context::<Signal<ChatState>>() else {
                if let Some(mut toast_store) = try_consume_context::<Signal<ToastStore>>() {
                    toast_store
                        .write()
                        .push(Severity::Warning, "Navigate to Chat first to export a conversation");
                }
                return;
            };

            let messages: Vec<ChatMessage> = {
                use crate::components::chat::MessageRole;
                let state = legacy_state.read();
                state
                    .messages
                    .iter()
                    .enumerate()
                    .map(|(i, m)| ChatMessage {
                        #[expect(clippy::as_conversions, reason = "message index to u64 id")]
                        id: i as u64 + 1,
                        role: match m.role {
                            MessageRole::User => Role::User,
                            MessageRole::Assistant => Role::Assistant,
                        },
                        content: m.content.clone(),
                        timestamp: 0,
                        agent_id: None,
                        tool_calls: 0,
                        thinking_content: None,
                        is_streaming: false,
                        model: None,
                        input_tokens: 0,
                        output_tokens: 0,
                    })
                    .collect()
            };

            if messages.is_empty() {
                if let Some(mut toast_store) = try_consume_context::<Signal<ToastStore>>() {
                    toast_store
                        .write()
                        .push(Severity::Warning, "Nothing to export — start a conversation first");
                }
                return;
            }

            let md = messages_to_markdown(&messages);
            if let Ok(escaped) = serde_json::to_string(&md) {
                let js = format!("navigator.clipboard.writeText({escaped})");
                spawn(async move {
                    let _ = document::eval(&js).await;
                    if let Some(mut toast_store) = try_consume_context::<Signal<ToastStore>>() {
                        toast_store
                            .write()
                            .push(Severity::Success, "Conversation copied to clipboard");
                    }
                });
            }
        }
        CommandCategory::Action => {
            on_execute.call(format!("/{name}"));
        }
    }
}

/// Slash command palette component.
///
/// - `is_open`: whether the palette is currently visible
/// - `on_execute`: called with the full command string when the user picks one
///
/// Reads `Signal<CommandStore>` from context.
#[component]
pub(crate) fn CommandPaletteView(is_open: bool, on_execute: EventHandler<String>) -> Element {
    let mut store = use_context::<Signal<CommandStore>>();
    let nav = use_navigator();

    if !is_open {
        return rsx! { div {} };
    }

    let rows: Vec<(usize, String, String, CommandCategory, bool)> = {
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
                    i == s.cursor,
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
                for (idx , name , desc , category , is_active) in rows {
                    {
                        let cmd_name = format!("/{name}");
                        let row_style = if is_active { ROW_ACTIVE_STYLE } else { ROW_STYLE };
                        let on_execute = on_execute;
                        let nav = nav.clone();
                        rsx! {
                            div {
                                key: "{idx}",
                                style: "{row_style}",
                                role: "option",
                                aria_selected: if is_active { "true" } else { "false" },
                                aria_label: "{cmd_name}: {desc}",
                                onclick: move |_| {
                                    store.write().cursor = idx;
                                    dispatch_command(&name, category, &nav, &on_execute);
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
