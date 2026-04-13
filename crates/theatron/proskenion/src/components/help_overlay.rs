//! Help overlay displaying all keyboard shortcuts.
//!
//! Toggled by F1 or via the command palette. Renders as a fixed-position
//! modal with a semi-transparent backdrop that closes on Escape or click.

use dioxus::prelude::*;

const OVERLAY_BACKDROP: &str = "\
    position: fixed; \
    top: 0; \
    left: 0; \
    right: 0; \
    bottom: 0; \
    background: rgba(0, 0, 0, 0.6); \
    display: flex; \
    align-items: center; \
    justify-content: center; \
    z-index: 100;\
";

const DIALOG_STYLE: &str = "\
    background: var(--bg-surface); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-lg); \
    padding: var(--space-6); \
    max-width: 600px; \
    width: 90vw; \
    max-height: 80vh; \
    overflow-y: auto; \
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);\
";

const TITLE_STYLE: &str = "\
    font-size: var(--text-xl); \
    font-weight: var(--weight-bold); \
    color: var(--text-heading, var(--text-primary)); \
    margin: 0 0 var(--space-4) 0;\
";

const SECTION_HEADER_STYLE: &str = "\
    font-size: var(--text-sm); \
    font-weight: var(--weight-semibold); \
    color: var(--text-secondary); \
    text-transform: uppercase; \
    letter-spacing: 0.05em; \
    margin: var(--space-4) 0 var(--space-2) 0;\
";

const ROW_STYLE: &str = "\
    display: flex; \
    justify-content: space-between; \
    align-items: center; \
    padding: var(--space-1) 0;\
";

const KEY_STYLE: &str = "\
    font-family: var(--font-mono, monospace); \
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
    background: var(--bg-hover, var(--bg)); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-sm); \
    padding: var(--space-1) var(--space-2); \
    white-space: nowrap;\
";

const LABEL_STYLE: &str = "\
    font-size: var(--text-sm); \
    color: var(--text-primary);\
";

const FOOTER_STYLE: &str = "\
    margin-top: var(--space-4); \
    padding-top: var(--space-3); \
    border-top: 1px solid var(--border); \
    font-size: var(--text-xs); \
    color: var(--text-muted, var(--text-secondary)); \
    text-align: center;\
";

#[derive(PartialEq)]
struct ShortcutEntry {
    keys: &'static str,
    description: &'static str,
}

const NAV_SHORTCUTS: &[ShortcutEntry] = &[
    ShortcutEntry { keys: "Ctrl+1 / Ctrl+Shift+C", description: "Chat" },
    ShortcutEntry { keys: "Ctrl+2 / Ctrl+Shift+F", description: "Files" },
    ShortcutEntry { keys: "Ctrl+3", description: "Planning" },
    ShortcutEntry { keys: "Ctrl+4", description: "Memory" },
    ShortcutEntry { keys: "Ctrl+5", description: "Metrics" },
    ShortcutEntry { keys: "Ctrl+6", description: "Ops" },
    ShortcutEntry { keys: "Ctrl+7", description: "Sessions" },
];

const ACTION_SHORTCUTS: &[ShortcutEntry] = &[
    ShortcutEntry { keys: "Ctrl+K", description: "Command palette" },
    ShortcutEntry { keys: "Ctrl+B", description: "Toggle sidebar" },
    ShortcutEntry { keys: "Ctrl+F  /  /", description: "Focus search" },
    ShortcutEntry { keys: "F1", description: "Help (this overlay)" },
    ShortcutEntry { keys: "Escape", description: "Close / dismiss" },
];

const CHAT_SHORTCUTS: &[ShortcutEntry] = &[
    ShortcutEntry { keys: "Ctrl+Enter", description: "Send message" },
    ShortcutEntry { keys: "Shift+Enter", description: "New line" },
    ShortcutEntry { keys: "Up / Down", description: "Input history" },
];

#[component]
pub(crate) fn HelpOverlay(visible: Signal<bool>) -> Element {
    if !*visible.read() {
        return rsx! {};
    }

    rsx! {
        div {
            style: "{OVERLAY_BACKDROP}",
            onclick: move |_| visible.set(false),
            "aria-label": "Help overlay backdrop",

            div {
                style: "{DIALOG_STYLE}",
                onclick: move |e| e.stop_propagation(),
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Keyboard shortcuts",

                h2 { style: "{TITLE_STYLE}", "Keyboard Shortcuts" }

                ShortcutSection { title: "Navigation", entries: NAV_SHORTCUTS }
                ShortcutSection { title: "Actions", entries: ACTION_SHORTCUTS }
                ShortcutSection { title: "Chat", entries: CHAT_SHORTCUTS }

                div {
                    style: "{FOOTER_STYLE}",
                    "Press F1 or Escape to close"
                }
            }
        }
    }
}

#[component]
fn ShortcutSection(title: &'static str, entries: &'static [ShortcutEntry]) -> Element {
    rsx! {
        div {
            h3 { style: "{SECTION_HEADER_STYLE}", "{title}" }
            for entry in entries.iter() {
                div {
                    style: "{ROW_STYLE}",
                    span { style: "{LABEL_STYLE}", "{entry.description}" }
                    span { style: "{KEY_STYLE}", "{entry.keys}" }
                }
            }
        }
    }
}
