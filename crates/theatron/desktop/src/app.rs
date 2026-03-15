//! Root component that gates on connection state.
//!
//! When disconnected, shows the connect view. When connected, shows the
//! main layout with the router. This is the entry point for the Dioxus
//! component tree.

use dioxus::prelude::*;

use crate::services::config;
use crate::state::connection::ConnectionState;
use crate::views::connect::ConnectView;

/// Root component.
///
/// Provides connection state and config as context signals, then renders
/// either the connect view or the main content based on connection state.
#[component]
pub fn App() -> Element {
    let loaded_config = use_hook(config::load_or_default);
    let connection_state = use_signal(ConnectionState::default);
    let connection_config = use_signal(|| loaded_config);

    let needs_connect = connection_state.read().needs_connect_view();

    if needs_connect {
        rsx! {
            ConnectView {
                connection_state,
                connection_config,
            }
        }
    } else {
        rsx! {
            MainLayout {
                connection_state,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main layout (placeholder until P602 scaffold merges)
// ---------------------------------------------------------------------------

const LAYOUT_STYLE: &str = "\
    display: flex; \
    height: 100vh; \
    font-family: system-ui, -apple-system, sans-serif;\
";

const SIDEBAR_STYLE: &str = "\
    width: 220px; \
    background: #1a1a2e; \
    color: #e0e0e0; \
    padding: 16px; \
    display: flex; \
    flex-direction: column; \
    gap: 4px; \
    flex-shrink: 0;\
";

const CONTENT_STYLE: &str = "\
    flex: 1; \
    padding: 24px; \
    overflow-y: auto; \
    background: #0f0f1a; \
    color: #e0e0e0;\
";

const BRAND_STYLE: &str = "\
    font-size: 18px; \
    font-weight: bold; \
    padding: 8px 12px; \
    margin-bottom: 16px; \
    color: #ffffff;\
";

const STATUS_DOT_STYLE: &str = "\
    display: inline-block; \
    width: 8px; \
    height: 8px; \
    border-radius: 50%; \
    background: #4caf50; \
    margin-right: 8px;\
";

const STATUS_BAR_STYLE: &str = "\
    margin-top: auto; \
    padding: 12px; \
    font-size: 12px; \
    color: #888; \
    border-top: 1px solid #333;\
";

/// Main app layout shown when connected.
///
/// This is a minimal layout that will be replaced by P602's full
/// sidebar/router layout. For now it shows a connected status and
/// placeholder content.
#[component]
fn MainLayout(connection_state: Signal<ConnectionState>) -> Element {
    let state_label = connection_state.read().label().to_string();

    rsx! {
        div {
            style: "{LAYOUT_STYLE}",
            nav {
                style: "{SIDEBAR_STYLE}",
                div { style: "{BRAND_STYLE}", "Aletheia" }

                div { style: "padding: 8px 12px; color: #888;", "Chat" }
                div { style: "padding: 8px 12px; color: #888;", "Files" }
                div { style: "padding: 8px 12px; color: #888;", "Planning" }
                div { style: "padding: 8px 12px; color: #888;", "Memory" }

                div {
                    style: "{STATUS_BAR_STYLE}",
                    span { style: "{STATUS_DOT_STYLE}" }
                    "{state_label}"
                }
            }
            main {
                style: "{CONTENT_STYLE}",
                h1 { style: "font-size: 24px; margin-bottom: 16px;", "Connected" }
                p { style: "color: #888;",
                    "Connected to the pylon server. Views will be available once the full scaffold (P602) merges."
                }
            }
        }
    }
}
