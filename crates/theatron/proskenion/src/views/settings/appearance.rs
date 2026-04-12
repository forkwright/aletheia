//! Appearance settings panel.
//!
//! Controls theme (Dark/Light/System), font size slider, UI density, and
//! accent color swatches. All styling uses CSS variables from the design
//! system so changes are visible under both themes.

use dioxus::prelude::*;

use crate::services::settings_config;
use crate::state::settings::{ACCENT_PRESETS, AppearanceSettings, UiDensity};
use crate::theme::ThemeMode;

// WHY: Section card styling uses theme tokens so appearance settings are
// visually consistent with the rest of the app and respond to theme changes.
const SECTION_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-5);";

const SECTION_LABEL_STYLE: &str = "\
    font-size: var(--text-xs); \
    font-weight: var(--weight-bold); \
    color: var(--text-muted); \
    text-transform: uppercase; \
    letter-spacing: 0.6px; \
    margin-bottom: var(--space-4);";

/// Appearance settings panel.
#[component]
pub(crate) fn AppearancePanel() -> Element {
    let mut appearance: Signal<AppearanceSettings> = use_context();
    let mut theme_mode: Signal<ThemeMode> = use_context();
    let server_store = use_context::<Signal<crate::state::settings::ServerConfigStore>>();
    let keybindings = use_context::<Signal<crate::state::settings::KeybindingStore>>();

    let current = appearance.read().clone();

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: var(--space-6); max-width: 540px;",

            // Theme
            section {
                style: SECTION_STYLE,
                div { style: SECTION_LABEL_STYLE, "Theme" }
                div {
                    style: "display: flex; gap: var(--space-2);",
                    for mode in [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System] {
                        {
                            let is_active = current.theme == mode_str(mode);
                            let bg = if is_active { "var(--accent)" } else { "var(--bg-surface-bright)" };
                            let border = if is_active { "1px solid var(--accent)" } else { "1px solid var(--border)" };
                            let color = if is_active { "var(--text-inverse)" } else { "var(--text-secondary)" };
                            let style = format!(
                                "padding: var(--space-2) 18px; background: {bg}; border: {border}; \
                                 border-radius: var(--radius-md); color: {color}; font-size: var(--text-sm); cursor: pointer; \
                                 transition: background var(--transition-quick), \
                                 color var(--transition-quick), border-color var(--transition-quick);"
                            );
                            rsx! {
                                button {
                                    key: "{mode:?}",
                                    style: "{style}",
                                    onclick: move |_| {
                                        theme_mode.set(mode);
                                        appearance.write().theme = mode_str(mode).to_string();
                                        let store = server_store.read();
                                        let app = appearance.read();
                                        let keys = keybindings.read();
                                        settings_config::save_state(&store, &app, &keys);
                                    },
                                    "{mode.icon()} {mode.label()}"
                                }
                            }
                        }
                    }
                }
            }

            // Font size
            section {
                style: SECTION_STYLE,
                div { style: SECTION_LABEL_STYLE, "Font Size" }
                div {
                    style: "display: flex; align-items: center; gap: var(--space-4);",
                    span {
                        style: "font-size: var(--text-xs); color: var(--text-muted); width: 22px;",
                        "12"
                    }
                    input {
                        r#type: "range",
                        min: "12",
                        max: "20",
                        step: "1",
                        value: "{current.font_size}",
                        style: "flex: 1; accent-color: var(--accent);",
                        oninput: move |e| {
                            if let Ok(v) = e.value().parse::<u8>() {
                                appearance.write().set_font_size(v);
                                let store = server_store.read();
                                let app = appearance.read();
                                let keys = keybindings.read();
                                settings_config::save_state(&store, &app, &keys);
                            }
                        },
                    }
                    span {
                        style: "font-size: var(--text-xs); color: var(--text-muted); width: 22px;",
                        "20"
                    }
                    span {
                        style: "font-size: var(--text-sm); color: var(--text-primary); width: 36px; text-align: right;",
                        "{current.font_size}px"
                    }
                }
            }

            // Density
            section {
                style: SECTION_STYLE,
                div { style: SECTION_LABEL_STYLE, "UI Density" }
                div {
                    style: "display: flex; gap: var(--space-2);",
                    for density in [UiDensity::Compact, UiDensity::Comfortable, UiDensity::Spacious] {
                        {
                            let is_active = current.density == density;
                            let bg = if is_active { "var(--accent)" } else { "var(--bg-surface-bright)" };
                            let border = if is_active { "1px solid var(--accent)" } else { "1px solid var(--border)" };
                            let color = if is_active { "var(--text-inverse)" } else { "var(--text-secondary)" };
                            let style = format!(
                                "flex: 1; padding: var(--space-2); background: {bg}; border: {border}; \
                                 border-radius: var(--radius-md); color: {color}; font-size: var(--text-sm); cursor: pointer; \
                                 text-align: center; transition: background var(--transition-quick), \
                                 color var(--transition-quick), border-color var(--transition-quick);"
                            );
                            rsx! {
                                button {
                                    key: "{density:?}",
                                    style: "{style}",
                                    onclick: move |_| {
                                        appearance.write().density = density;
                                        let store = server_store.read();
                                        let app = appearance.read();
                                        let keys = keybindings.read();
                                        settings_config::save_state(&store, &app, &keys);
                                    },
                                    "{density.label()}"
                                }
                            }
                        }
                    }
                }
            }

            // Accent color
            section {
                style: SECTION_STYLE,
                div { style: SECTION_LABEL_STYLE, "Accent Color" }
                div {
                    style: "display: flex; gap: var(--space-3); flex-wrap: wrap;",
                    for (label, hex) in ACCENT_PRESETS.iter() {
                        {
                            let is_active = current.accent_color == *hex;
                            let border = if is_active {
                                "3px solid var(--text-primary)"
                            } else {
                                "3px solid transparent"
                            };
                            let hex_owned = hex.to_string();
                            let style = format!(
                                "width: 30px; height: 30px; border-radius: 50%; background: {hex_owned}; \
                                 border: {border}; cursor: pointer; outline: none; \
                                 transition: border-color var(--transition-quick);"
                            );
                            rsx! {
                                button {
                                    key: "{label}",
                                    title: "{label}",
                                    style: "{style}",
                                    onclick: move |_| {
                                        appearance.write().accent_color = hex_owned.clone();
                                        let store = server_store.read();
                                        let app = appearance.read();
                                        let keys = keybindings.read();
                                        settings_config::save_state(&store, &app, &keys);
                                    },
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn mode_str(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Dark => "dark",
        ThemeMode::Light => "light",
        ThemeMode::System => "system",
    }
}
