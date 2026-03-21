//! Settings panel: server info, theme toggle, and about section.

use dioxus::prelude::*;

use crate::state::connection::ConnectionConfig;
use crate::theme::ThemeMode;

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 24px; \
    max-width: 600px;\
";

const SECTION_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px 20px;\
";

const SECTION_TITLE: &str = "\
    font-size: 14px; \
    font-weight: bold; \
    color: #aaa; \
    text-transform: uppercase; \
    letter-spacing: 0.5px; \
    margin-bottom: 12px;\
";

const ROW_STYLE: &str = "\
    display: flex; \
    justify-content: space-between; \
    align-items: center; \
    padding: 8px 0; \
    border-bottom: 1px solid #222;\
";

const LABEL_STYLE: &str = "\
    color: #888; \
    font-size: 13px;\
";

const VALUE_STYLE: &str = "\
    color: #e0e0e0; \
    font-size: 13px;\
";

const TOGGLE_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 6px 14px; \
    font-size: 13px; \
    cursor: pointer;\
";

const ABOUT_STYLE: &str = "\
    color: #666; \
    font-size: 12px; \
    line-height: 1.6;\
";

#[component]
pub(crate) fn Settings() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut theme_mode: Signal<ThemeMode> = use_context();

    let server_url = config.read().server_url.clone();
    let has_token = config.read().auth_token.is_some();
    let auto_reconnect = config.read().auto_reconnect;
    let current_theme = *theme_mode.read();

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            h2 { style: "font-size: 20px; margin: 0;", "Settings" }

            div {
                style: "{SECTION_STYLE}",
                div { style: "{SECTION_TITLE}", "Connection" }
                div {
                    style: "{ROW_STYLE}",
                    span { style: "{LABEL_STYLE}", "Server URL" }
                    span { style: "{VALUE_STYLE}", "{server_url}" }
                }
                div {
                    style: "{ROW_STYLE}",
                    span { style: "{LABEL_STYLE}", "Auth token" }
                    span { style: "{VALUE_STYLE}",
                        if has_token { "configured" } else { "none" }
                    }
                }
                div {
                    style: "{ROW_STYLE} border-bottom: none;",
                    span { style: "{LABEL_STYLE}", "Auto-reconnect" }
                    span { style: "{VALUE_STYLE}",
                        if auto_reconnect { "enabled" } else { "disabled" }
                    }
                }
            }

            div {
                style: "{SECTION_STYLE}",
                div { style: "{SECTION_TITLE}", "Appearance" }
                div {
                    style: "{ROW_STYLE} border-bottom: none;",
                    span { style: "{LABEL_STYLE}", "Theme" }
                    button {
                        style: "{TOGGLE_BTN}",
                        onclick: move |_| {
                            theme_mode.set(current_theme.next());
                        },
                        "{current_theme.icon()} {current_theme.label()}"
                    }
                }
            }

            div {
                style: "{SECTION_STYLE}",
                div { style: "{SECTION_TITLE}", "About" }
                div {
                    style: "{ABOUT_STYLE}",
                    p { "Aletheia Desktop" }
                    p { "Distributed cognition system" }
                    p { "Built with Dioxus + theatron-core" }
                }
            }
        }
    }
}
