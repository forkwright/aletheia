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
                style: "display: flex; gap: 0; padding: 0 var(--space-5); border-bottom: 1px solid var(--border); background: var(--bg-surface);",
                for tab in [SettingsTab::Servers, SettingsTab::Appearance, SettingsTab::Keybindings, SettingsTab::Notifications] {
                    {
                        let is_active = active_tab() == tab;
                        let border = if is_active { "2px solid var(--accent)" } else { "2px solid transparent" };
                        let color = if is_active { "var(--text-primary)" } else { "var(--text-secondary)" };
                        let style = format!(
                            "padding: var(--space-3) 18px; background: none; border: none; border-bottom: {border}; \
                             color: {color}; font-size: var(--text-sm); cursor: pointer; \
                             transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);"
                        );
                        rsx! {
                            button {
                                key: "{tab:?}",
                                style: "{style}",
                                onclick: move |_| active_tab.set(tab),
                                "{tab.label()}"
                            }
                        }
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
