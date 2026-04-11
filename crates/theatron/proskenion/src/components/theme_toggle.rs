//! Theme toggle component.

use dioxus::prelude::*;

use crate::state::settings::{AppearanceSettings, KeybindingStore, ServerConfigStore};
use crate::theme::ThemeMode;

/// Cycles between Dark, Light, and System theme modes.
///
/// Reads `Signal<ThemeMode>` from context (provided by `ThemeProvider`)
/// and advances to the next mode on click. Persists the choice to the
/// settings config so it survives restarts.
#[component]
pub(crate) fn ThemeToggle() -> Element {
    let mut mode = use_context::<Signal<ThemeMode>>();
    let mut appearance = use_context::<Signal<AppearanceSettings>>();
    let server_store = use_context::<Signal<ServerConfigStore>>();
    let keybindings = use_context::<Signal<KeybindingStore>>();
    let current = mode();
    let icon = current.icon();
    let label = current.label();

    rsx! {
        button {
            r#type: "button",
            onclick: move |_| {
                let next = mode().next();
                mode.set(next);
                // Persist to settings so it survives restart
                appearance.write().theme = next.label().to_lowercase();
                crate::services::settings_config::save_state(
                    &server_store.read(),
                    &appearance.read(),
                    &keybindings.read(),
                );
            },
            title: "Theme: {label}",
            "aria-label": "Switch theme, current: {label}",
            style: "
                display: inline-flex;
                align-items: center;
                gap: var(--space-2);
                padding: var(--space-1) var(--space-3);
                border: 1px solid var(--border);
                border-radius: var(--radius-md);
                background: var(--bg-surface);
                color: var(--text-secondary);
                font-family: var(--font-body);
                font-size: var(--text-sm);
                cursor: pointer;
                transition: border-color var(--transition-quick),
                            color var(--transition-quick),
                            background-color var(--transition-quick);
            ",
            span { "{icon}" }
            span { "{label}" }
        }
    }
}
