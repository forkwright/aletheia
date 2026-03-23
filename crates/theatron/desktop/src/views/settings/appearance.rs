//! Appearance settings panel.
//!
//! Controls theme (Dark/Light/System), font size slider, UI density, and
//! accent color swatches.

use dioxus::prelude::*;

use crate::services::settings_config;
use crate::state::settings::{ACCENT_PRESETS, AppearanceSettings, UiDensity};
use crate::theme::ThemeMode;

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
            style: "display: flex; flex-direction: column; gap: 24px; max-width: 540px;",

            // Theme
            section {
                style: "background: #1a1a2e; border: 1px solid #333; border-radius: 8px; padding: 16px 20px;",
                div {
                    style: "font-size: 11px; font-weight: bold; color: #666; text-transform: uppercase; \
                            letter-spacing: 0.6px; margin-bottom: 14px;",
                    "Theme"
                }
                div {
                    style: "display: flex; gap: 8px;",
                    for mode in [ThemeMode::Dark, ThemeMode::Light, ThemeMode::System] {
                        {
                            let is_active = current.theme == mode_str(mode);
                            let bg = if is_active { "#5b6af0" } else { "#2a2a4a" };
                            let border = if is_active { "1px solid #5b6af0" } else { "1px solid #444" };
                            let color = if is_active { "#fff" } else { "#aaa" };
                            let style = format!(
                                "padding: 8px 18px; background: {bg}; border: {border}; \
                                 border-radius: 6px; color: {color}; font-size: 13px; cursor: pointer;"
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
                style: "background: #1a1a2e; border: 1px solid #333; border-radius: 8px; padding: 16px 20px;",
                div {
                    style: "font-size: 11px; font-weight: bold; color: #666; text-transform: uppercase; \
                            letter-spacing: 0.6px; margin-bottom: 14px;",
                    "Font Size"
                }
                div {
                    style: "display: flex; align-items: center; gap: 14px;",
                    span {
                        style: "font-size: 11px; color: #666; width: 22px;",
                        "12"
                    }
                    input {
                        r#type: "range",
                        min: "12",
                        max: "20",
                        step: "1",
                        value: "{current.font_size}",
                        style: "flex: 1; accent-color: #5b6af0;",
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
                        style: "font-size: 11px; color: #666; width: 22px;",
                        "20"
                    }
                    span {
                        style: "font-size: 13px; color: #e0e0e0; width: 36px; text-align: right;",
                        "{current.font_size}px"
                    }
                }
            }

            // Density
            section {
                style: "background: #1a1a2e; border: 1px solid #333; border-radius: 8px; padding: 16px 20px;",
                div {
                    style: "font-size: 11px; font-weight: bold; color: #666; text-transform: uppercase; \
                            letter-spacing: 0.6px; margin-bottom: 14px;",
                    "UI Density"
                }
                div {
                    style: "display: flex; gap: 8px;",
                    for density in [UiDensity::Compact, UiDensity::Comfortable, UiDensity::Spacious] {
                        {
                            let is_active = current.density == density;
                            let bg = if is_active { "#5b6af0" } else { "#2a2a4a" };
                            let border = if is_active { "1px solid #5b6af0" } else { "1px solid #444" };
                            let color = if is_active { "#fff" } else { "#aaa" };
                            let style = format!(
                                "flex: 1; padding: 8px; background: {bg}; border: {border}; \
                                 border-radius: 6px; color: {color}; font-size: 13px; cursor: pointer; text-align: center;"
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
                style: "background: #1a1a2e; border: 1px solid #333; border-radius: 8px; padding: 16px 20px;",
                div {
                    style: "font-size: 11px; font-weight: bold; color: #666; text-transform: uppercase; \
                            letter-spacing: 0.6px; margin-bottom: 14px;",
                    "Accent Color"
                }
                div {
                    style: "display: flex; gap: 10px; flex-wrap: wrap;",
                    for (label, hex) in ACCENT_PRESETS.iter() {
                        {
                            let is_active = current.accent_color == *hex;
                            let border = if is_active { "3px solid #fff" } else { "3px solid transparent" };
                            let hex_owned = hex.to_string();
                            let style = format!(
                                "width: 30px; height: 30px; border-radius: 50%; background: {hex_owned}; \
                                 border: {border}; cursor: pointer; outline: none;"
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
