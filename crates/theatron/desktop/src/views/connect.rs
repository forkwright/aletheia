//! Connection view — first-run experience and reconnection UI.
//!
//! Shown when the app is not connected to a pylon instance. Provides inputs
//! for server URL and auth token, a connect button, and a status indicator
//! showing the current connection state.

use dioxus::prelude::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::services::config;
use crate::services::connection::ConnectionService;
use crate::state::connection::{ConnectionConfig, ConnectionState};

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    height: 100vh; \
    background: #0f0f1a; \
    color: #e0e0e0; \
    font-family: system-ui, -apple-system, sans-serif;\
";

const CARD_STYLE: &str = "\
    background: #1a1a2e; \
    border-radius: 12px; \
    padding: 40px; \
    width: 400px; \
    display: flex; \
    flex-direction: column; \
    gap: 16px;\
";

const TITLE_STYLE: &str = "\
    font-size: 24px; \
    font-weight: bold; \
    text-align: center; \
    margin-bottom: 8px;\
";

const LABEL_STYLE: &str = "\
    font-size: 14px; \
    color: #aaa; \
    margin-bottom: 4px;\
";

const INPUT_STYLE: &str = "\
    background: #0f0f1a; \
    border: 1px solid #333; \
    border-radius: 6px; \
    padding: 10px 12px; \
    color: #e0e0e0; \
    font-size: 14px; \
    width: 100%; \
    box-sizing: border-box;\
";

const BUTTON_STYLE: &str = "\
    background: #4a4aff; \
    color: white; \
    border: none; \
    border-radius: 6px; \
    padding: 12px; \
    font-size: 16px; \
    cursor: pointer; \
    margin-top: 8px;\
";

const BUTTON_DISABLED_STYLE: &str = "\
    background: #333; \
    color: #666; \
    border: none; \
    border-radius: 6px; \
    padding: 12px; \
    font-size: 16px; \
    cursor: not-allowed; \
    margin-top: 8px;\
";

const STATUS_STYLE: &str = "\
    text-align: center; \
    font-size: 14px; \
    min-height: 20px;\
";

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

/// Connect view component.
///
/// Reads connection state from context and provides a form for the user to
/// configure and initiate a server connection.
#[component]
pub fn ConnectView(
    connection_state: Signal<ConnectionState>,
    connection_config: Signal<ConnectionConfig>,
) -> Element {
    let mut url_input = use_signal(|| connection_config.read().server_url.clone());
    let mut token_input = use_signal(|| {
        connection_config
            .read()
            .auth_token
            .clone()
            .unwrap_or_default()
    });

    let is_connecting = matches!(
        *connection_state.read(),
        ConnectionState::Connecting | ConnectionState::Reconnecting { .. }
    );

    let status_text = match &*connection_state.read() {
        ConnectionState::Disconnected => String::new(),
        ConnectionState::Connecting => "Connecting...".to_string(),
        ConnectionState::Connected => "Connected".to_string(),
        ConnectionState::Reconnecting { attempt } => {
            format!("Reconnecting (attempt {attempt})...")
        }
        ConnectionState::Failed { reason } => format!("Failed: {reason}"),
    };

    let status_color = match &*connection_state.read() {
        ConnectionState::Connected => "#4caf50",
        ConnectionState::Failed { .. } => "#f44336",
        ConnectionState::Reconnecting { .. } | ConnectionState::Connecting => "#ff9800",
        ConnectionState::Disconnected => "#888",
    };

    let on_connect = move |_| {
        let url = url_input.read().clone();
        let token = token_input.read().clone();
        let token_opt = if token.is_empty() { None } else { Some(token) };

        let new_config = ConnectionConfig {
            server_url: url,
            auth_token: token_opt,
            auto_reconnect: true,
        };

        // Persist config to disk.
        if let Err(e) = config::save(&new_config) {
            tracing::warn!("failed to save config: {e}");
        }

        // Update the shared config signal.
        connection_config.write().clone_from(&new_config);

        // Set up channel for background → UI communication.
        let (tx, mut rx) = mpsc::unbounded_channel::<ConnectionState>();
        let cancel = CancellationToken::new();
        let svc = ConnectionService::new(new_config, cancel, tx);

        // Spawn the connection service on the tokio runtime.
        tokio::spawn(svc.run());

        // Spawn a Dioxus-side task to read state updates from the channel
        // and write them to the signal on the UI thread.
        let mut state_signal = connection_state;
        spawn(async move {
            while let Some(new_state) = rx.recv().await {
                state_signal.set(new_state);
            }
        });
    };

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            div {
                style: "{CARD_STYLE}",
                div { style: "{TITLE_STYLE}", "Aletheia" }

                div {
                    label { style: "{LABEL_STYLE}", "Server URL" }
                    input {
                        style: "{INPUT_STYLE}",
                        r#type: "text",
                        placeholder: "http://localhost:3000",
                        value: "{url_input}",
                        disabled: is_connecting,
                        oninput: move |evt: Event<FormData>| {
                            url_input.set(evt.value().clone());
                        },
                    }
                }

                div {
                    label { style: "{LABEL_STYLE}", "Auth Token (optional)" }
                    input {
                        style: "{INPUT_STYLE}",
                        r#type: "password",
                        placeholder: "Bearer token",
                        value: "{token_input}",
                        disabled: is_connecting,
                        oninput: move |evt: Event<FormData>| {
                            token_input.set(evt.value().clone());
                        },
                    }
                }

                button {
                    style: if is_connecting { "{BUTTON_DISABLED_STYLE}" } else { "{BUTTON_STYLE}" },
                    disabled: is_connecting,
                    onclick: on_connect,
                    if is_connecting { "Connecting..." } else { "Connect" }
                }

                div {
                    style: "{STATUS_STYLE} color: {status_color};",
                    "{status_text}"
                }
            }
        }
    }
}
