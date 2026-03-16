//! Theme toggle component.

use dioxus::prelude::*;

use crate::theme::ThemeMode;

/// Cycles between Dark, Light, and System theme modes.
///
/// Reads `Signal<ThemeMode>` from context (provided by `ThemeProvider`)
/// and advances to the next mode on click.
#[component]
pub fn ThemeToggle() -> Element {
    let mut mode = use_context::<Signal<ThemeMode>>();
    let current = mode();
    let icon = current.icon();
    let label = current.label();

    rsx! {
        button {
            r#type: "button",
            onclick: move |_| mode.set(mode().next()),
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
