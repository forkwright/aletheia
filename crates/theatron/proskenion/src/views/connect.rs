//! Connection view: first-run experience and reconnection UI.
//!
//! Shown when the app is not connected to a pylon instance. Provides inputs
//! for server URL and auth token, a connect button, and a status indicator
//! showing the current connection state.

use dioxus::prelude::*;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::services::config;
use crate::services::connection::ConnectionService;
use crate::state::connection::{ConnectionConfig, ConnectionState};

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    height: 100vh; \
    background: var(--bg); \
    color: var(--text-primary); \
    font-family: var(--font-body, system-ui, -apple-system, sans-serif);\
";

const CARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border-radius: var(--radius-lg); \
    padding: 40px; \
    width: 400px; \
    display: flex; \
    flex-direction: column; \
    gap: var(--space-4);\
";

const TITLE_STYLE: &str = "\
    font-size: var(--text-2xl); \
    font-weight: var(--weight-bold); \
    text-align: center; \
    margin-bottom: var(--space-2);\
";

const LABEL_STYLE: &str = "\
    font-size: var(--text-base); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-1);\
";

const INPUT_STYLE: &str = "\
    background: var(--input-bg); \
    border: 1px solid var(--input-border); \
    border-radius: var(--radius-md); \
    padding: var(--space-3) var(--space-3); \
    color: var(--text-primary); \
    font-size: var(--text-base); \
    width: 100%; \
    box-sizing: border-box;\
";

const BUTTON_STYLE: &str = "\
    background: var(--accent); \
    color: var(--text-inverse); \
    border: none; \
    border-radius: var(--radius-md); \
    padding: var(--space-3); \
    font-size: var(--text-md); \
    cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); \
    margin-top: var(--space-2);\
";

const BUTTON_DISABLED_STYLE: &str = "\
    background: var(--bg-surface-bright); \
    color: var(--text-muted); \
    border: none; \
    border-radius: var(--radius-md); \
    padding: var(--space-3); \
    font-size: var(--text-md); \
    cursor: not-allowed; \
    margin-top: var(--space-2);\
";

const STATUS_STYLE: &str = "\
    text-align: center; \
    font-size: var(--text-base); \
    min-height: 20px;\
";

/// Connect view component.
///
/// Reads connection state from context and provides a form for the user to
/// configure and initiate a server connection. Auto-discovery always runs in
/// the background when the view mounts and pre-fills the URL with the first
/// live server found; a value the user has typed takes precedence.
#[component]
pub(crate) fn ConnectView(
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
    // WHY: Hold the cancel token so the user can abort a connection attempt.
    let mut active_cancel = use_signal(|| None::<CancellationToken>);
    // WHY: Discovery must never clobber a URL the user typed; this flips on the
    // first edit and wins at both discovery checkpoints below.
    let mut user_edited_url = use_signal(|| false);

    let default_url = ConnectionConfig::default().server_url;
    // WHY: A persisted custom URL joins the probe list so a live custom server
    // survives discovery, while a dead one no longer strands the user.
    let persisted_url = {
        let url = url_input.peek().clone();
        (url != default_url).then_some(url)
    };

    // WHY: Always run discovery when this view mounts -- it only shows while
    // disconnected, so pre-filling the first live server found beats trusting
    // a possibly-stale persisted URL. `peek` keeps typing from re-triggering.
    let _discovery = use_resource(move || {
        let persisted_url = persisted_url.clone();
        async move {
            if *user_edited_url.peek() {
                return;
            }
            let mut discovery_config =
                skene::discovery::DiscoveryConfig::from_env_and_known_hosts();
            if let Some(url) = persisted_url {
                discovery_config.base_urls.push(url);
            }
            let discovered = skene::discovery::discover_server_with_config(&discovery_config).await;
            // NOTE: Re-checked after the await -- the user may have typed while
            // the probe was in flight, and their value wins.
            if *user_edited_url.peek() {
                return;
            }
            if let Some(url) = discovered {
                tracing::info!(url = %url, "auto-discovered server, updating connect form");
                url_input.set(url);
            }
        }
    });

    let is_connecting = matches!(
        *connection_state.read(),
        ConnectionState::Connecting | ConnectionState::Reconnecting { .. }
    );

    let status_text = match &*connection_state.read() {
        ConnectionState::Disconnected => String::new(),
        ConnectionState::Connecting => "Connecting...".to_string(),
        ConnectionState::Connected => "Connected".to_string(),
        ConnectionState::ConnectedDegraded { status } => {
            format!("Connected ({status})")
        }
        ConnectionState::Reconnecting { attempt } => {
            format!("Reconnecting (attempt {attempt})...")
        }
        ConnectionState::TimedOut => "Connection timed out".to_string(),
        ConnectionState::Failed { reason } => format!("Failed: {reason}"),
    };

    let status_color = match &*connection_state.read() {
        ConnectionState::Connected => "var(--status-success)",
        ConnectionState::ConnectedDegraded { .. } => "var(--status-warning)",
        ConnectionState::Failed { .. } | ConnectionState::TimedOut => "var(--status-error)",
        ConnectionState::Reconnecting { .. } | ConnectionState::Connecting => {
            "var(--status-warning)"
        }
        ConnectionState::Disconnected => "var(--text-muted)",
    };

    let on_connect = move |_| {
        // WHY: Cancel any previously running connection attempt so we do not
        // leak background tasks when the user clicks Connect multiple times.
        if let Some(prev) = active_cancel.read().as_ref() {
            prev.cancel();
        }

        let url = url_input.read().clone();
        let token = token_input.read().clone();
        let token_opt = if token.is_empty() { None } else { Some(token) };

        let mut new_config = ConnectionConfig {
            server_url: url,
            auth_token: token_opt,
            auto_reconnect: true,
            ..ConnectionConfig::default()
        };

        // NOTE: Set up channel for background to UI communication.
        let (tx, mut rx) = mpsc::unbounded_channel::<ConnectionState>();
        let cancel = CancellationToken::new();
        active_cancel.set(Some(cancel.clone()));
        connection_state.set(ConnectionState::Connecting);

        let mut state_signal = connection_state;
        let mut config_signal = connection_config;
        spawn(async move {
            crate::services::connection::refresh_client_contract(&mut new_config).await;

            // WHY: Persist the active server to the canonical settings store so
            // the connection survives app restarts with discovered client contract
            // metadata intact.
            if let Err(e) = config::save(&new_config) {
                tracing::warn!("failed to save active server: {e}");
            }

            // NOTE: Update the shared config signal before the connected UI mounts.
            config_signal.write().clone_from(&new_config);

            if cancel.is_cancelled() {
                state_signal.set(ConnectionState::Disconnected);
                return;
            }

            let svc = ConnectionService::new(new_config, cancel, tx);

            // WHY: Spawn the connection service on the tokio runtime so it runs
            // concurrently without blocking the Dioxus UI thread.
            tokio::spawn(
                svc.run()
                    .instrument(tracing::info_span!("connection_service")),
            );

            // WHY: Read state updates from the channel and write them to the
            // signal on the UI thread.
            while let Some(new_state) = rx.recv().await {
                state_signal.set(new_state);
            }
        });
    };

    // WHY: Allow the user to abort a connection attempt that is taking too long.
    let on_cancel = move |_| {
        if let Some(cancel) = active_cancel.read().as_ref() {
            cancel.cancel();
        }
        connection_state.set(ConnectionState::Disconnected);
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
                        placeholder: "{default_url}",
                        value: "{url_input}",
                        disabled: is_connecting,
                        oninput: move |evt: Event<FormData>| {
                            user_edited_url.set(true);
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

                if is_connecting {
                    button {
                        style: "{BUTTON_DISABLED_STYLE}",
                        disabled: true,
                        "Connecting..."
                    }
                    button {
                        style: "\
                            background: transparent; \
                            color: var(--status-error); \
                            border: 1px solid var(--status-error); \
                            border-radius: var(--radius-md); \
                            padding: var(--space-2); \
                            font-size: var(--text-sm); \
                            cursor: pointer; \
                            transition: background-color var(--transition-quick);\
                        ",
                        onclick: on_cancel,
                        "Cancel"
                    }
                } else {
                    button {
                        style: "{BUTTON_STYLE}",
                        onclick: on_connect,
                        if matches!(*connection_state.read(), ConnectionState::TimedOut) {
                            "Retry"
                        } else {
                            "Connect"
                        }
                    }
                }

                div {
                    style: "{STATUS_STYLE} color: {status_color};",
                    "{status_text}"
                }
            }
        }
    }
}
