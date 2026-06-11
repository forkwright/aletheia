//! Settings views: server connections, appearance, keybindings, notifications, and setup wizard.

pub(crate) mod appearance;
pub(crate) mod keybindings;
pub(crate) mod notifications;
pub(crate) mod servers;
pub(crate) mod wizard;

use dioxus::prelude::*;

use crate::views::settings::notifications::NotificationSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SettingsTab {
    #[default]
    Servers,
    Appearance,
    Keybindings,
    Notifications,
}

impl SettingsTab {
    fn label(self) -> &'static str {
        match self {
            Self::Servers => "Connections",
            Self::Appearance => "Appearance",
            Self::Keybindings => "Keybindings",
            Self::Notifications => "Notifications",
        }
    }
}

/// Settings container with tab navigation.
#[component]
pub(crate) fn Settings() -> Element {
    let mut active_tab = use_signal(SettingsTab::default);

    rsx! {
        div {
            style: "display: flex; flex-direction: column; height: 100%; overflow: hidden;",

            div {
                style: "display: flex; gap: var(--space-2); padding: 0 var(--space-5); border-bottom: 1px solid var(--border); background: var(--bg-surface);",
                for tab in [SettingsTab::Servers, SettingsTab::Appearance, SettingsTab::Keybindings, SettingsTab::Notifications] {
                    SettingsTabButton {
                        key: "{tab:?}",
                        label: tab.label(),
                        is_active: active_tab() == tab,
                        on_select: move |_| active_tab.set(tab),
                    }
                }
            }

            div {
                style: "flex: 1; overflow-y: auto; padding: var(--space-6);",
                { match active_tab() {
                    SettingsTab::Servers => rsx! { servers::ServersPanel {} },
                    SettingsTab::Appearance => rsx! { appearance::AppearancePanel {} },
                    SettingsTab::Keybindings => rsx! { keybindings::KeybindingsPanel {} },
                    SettingsTab::Notifications => rsx! { NotificationSettings {} },
                } }
            }
        }
    }
}

/// Single settings tab: mono-uppercase label with active underline and hover state.
#[component]
fn SettingsTabButton(label: &'static str, is_active: bool, on_select: EventHandler<()>) -> Element {
    let mut hovered = use_signal(|| false);

    let underline = if is_active {
        "var(--accent)"
    } else {
        "transparent"
    };
    let color = if is_active || hovered() {
        "var(--text-primary)"
    } else {
        "var(--text-secondary)"
    };
    let bg = if hovered() && !is_active {
        "var(--bg-surface-bright)"
    } else {
        "transparent"
    };
    let style = format!(
        "padding: var(--space-3) var(--space-4); background: {bg}; border: none; \
         border-bottom: 2px solid {underline}; border-radius: var(--radius-sm) var(--radius-sm) 0 0; \
         color: {color}; font-family: var(--font-mono); font-size: var(--text-xs); \
         font-weight: var(--weight-medium); text-transform: uppercase; letter-spacing: var(--tracking-wide); \
         cursor: pointer; transition: background-color var(--transition-quick), \
         color var(--transition-quick), border-color var(--transition-quick);"
    );

    rsx! {
        button {
            style: "{style}",
            onmouseenter: move |_| hovered.set(true),
            onmouseleave: move |_| hovered.set(false),
            onclick: move |_| on_select.call(()),
            "{label}"
        }
    }
}
