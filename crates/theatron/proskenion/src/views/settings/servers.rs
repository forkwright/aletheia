//! Server connections management panel.
//!
//! Displays saved server entries with health indicators. Supports add, edit,
//! remove, test-connection, and switch-to-server actions.

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;

use crate::services::{connection::PylonClient, settings_config};
use crate::state::connection::{ConnectionConfig, ConnectionState};
use crate::state::settings::{ServerConfigStore, ServerHealth};

// --- Plain data snapshot (avoids borrow-through-Signal in RSX) ---

#[derive(Clone)]
struct ServerSnap {
    id: String,
    name: String,
    url: String,
    auth_token: Option<String>,
    is_active: bool,
}

// --- Async helpers ---

async fn probe_health(url: &str, token: Option<&str>) -> ServerHealth {
    let config = ConnectionConfig {
        server_url: url.to_string(),
        auth_token: token.map(str::to_string),
        auto_reconnect: false,
    };
    match PylonClient::new(&config) {
        Ok(client) => match client.health().await {
            Ok(()) => ServerHealth::Healthy,
            Err(_) => ServerHealth::Unreachable,
        },
        Err(_) => ServerHealth::Unreachable,
    }
}

// --- Main panel ---

/// Server connections management panel.
#[component]
pub(crate) fn ServersPanel() -> Element {
    let mut server_store: Signal<ServerConfigStore> = use_context();
    let mut connection_config: Signal<ConnectionConfig> = use_context();
    let mut connection_state: Signal<ConnectionState> = use_context();
    let appearance = use_context::<Signal<crate::state::settings::AppearanceSettings>>();
    let keybindings = use_context::<Signal<crate::state::settings::KeybindingStore>>();

    let mut health_map: Signal<HashMap<String, ServerHealth>> = use_signal(HashMap::new);
    let mut testing_ids: Signal<HashSet<String>> = use_signal(HashSet::new);
    let mut show_add = use_signal(|| false);

    // Pre-collect snapshots before RSX to avoid borrow-through-Signal.
    let snapshots: Vec<ServerSnap> = {
        let store = server_store.read();
        store
            .servers
            .iter()
            .map(|e| ServerSnap {
                id: e.id.clone(),
                name: e.name.clone(),
                url: e.url.clone(),
                auth_token: e.auth_token.clone(),
                is_active: store.active_id.as_deref() == Some(e.id.as_str()),
            })
            .collect()
    };

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: var(--space-4); max-width: 680px;",

            // Header
            div {
                style: "display: flex; justify-content: space-between; align-items: center;",
                h3 { style: "margin: 0; font-size: var(--text-md); color: var(--text-primary);", "Server Connections" }
                button {
                    style: "padding: var(--space-2) var(--space-4); background: var(--border); border: 1px solid var(--border); \
                            border-radius: var(--radius-md); color: var(--text-primary); font-size: var(--text-sm); cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
                    onclick: move |_| show_add.toggle(),
                    if show_add() { "Cancel" } else { "+ Add server" }
                }
            }

            // Add form (inline)
            if show_add() {
                AddServerForm {
                    server_store,
                    on_saved: move |_| { show_add.set(false); }
                }
            }

            // Server list
            if snapshots.is_empty() {
                div {
                    style: "padding: var(--space-8); text-align: center; color: var(--text-muted); font-size: var(--text-base); \
                            background: var(--bg-surface); border: 1px solid var(--border); border-radius: var(--radius-md);",
                    "No servers configured. Add one above."
                }
            }

            for snap in snapshots.iter() {
                {
                    let sid = snap.id.clone();
                    let sid_test = sid.clone();
                    let sid_switch = sid.clone();
                    let sid_remove = sid.clone();
                    let surl = snap.url.clone();
                    let stoken = snap.auth_token.clone();
                    let health = *health_map.read().get(&snap.id).unwrap_or(&ServerHealth::Unchecked);
                    let is_testing = testing_ids.read().contains(&snap.id);

                    rsx! {
                        ServerCard {
                            key: "{sid}",
                            id: sid.clone(),
                            name: snap.name.clone(),
                            url: surl.clone(),
                            auth_token: stoken.clone(),
                            is_active: snap.is_active,
                            health,
                            is_testing,
                            on_test: move |_| {
                                let url = surl.clone();
                                let token = stoken.clone();
                                let id = sid_test.clone();
                                testing_ids.write().insert(id.clone());
                                health_map.write().remove(&id);
                                spawn(async move {
                                    let result = probe_health(&url, token.as_deref()).await;
                                    testing_ids.write().remove(&id);
                                    health_map.write().insert(id, result);
                                });
                            },
                            on_switch: move |_| {
                                {
                                    let mut store = server_store.write();
                                    store.set_active(&sid_switch);
                                    if let Some(entry) = store.active() {
                                        let url = entry.url.clone();
                                        let token = entry.auth_token.clone();
                                        drop(store);
                                        connection_config.write().server_url = url;
                                        connection_config.write().auth_token = token;
                                    }
                                }
                                connection_state.set(ConnectionState::Disconnected);
                                let store = server_store.read();
                                let app = appearance.read();
                                let keys = keybindings.read();
                                settings_config::save_state(&store, &app, &keys);
                            },
                            on_remove: move |_| {
                                server_store.write().remove(&sid_remove);
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

// --- Server card ---

#[component]
fn ServerCard(
    id: String,
    name: String,
    url: String,
    auth_token: Option<String>,
    is_active: bool,
    health: ServerHealth,
    is_testing: bool,
    on_test: EventHandler<()>,
    on_switch: EventHandler<()>,
    on_remove: EventHandler<()>,
) -> Element {
    let mut editing = use_signal(|| false);
    let mut edit_name = use_signal(|| name.clone());
    let mut edit_url = use_signal(|| url.clone());
    let mut edit_token = use_signal(|| auth_token.clone().unwrap_or_default());

    let mut server_store: Signal<ServerConfigStore> = use_context();
    let appearance = use_context::<Signal<crate::state::settings::AppearanceSettings>>();
    let keybindings = use_context::<Signal<crate::state::settings::KeybindingStore>>();

    let health_color = health.color();
    let health_label = health.label();
    let card_border = if is_active { "1px solid var(--accent)" } else { "1px solid var(--border)" };

    let id_for_save = id.clone();

    rsx! {
        div {
            style: "background: var(--bg-surface); border: {card_border}; border-radius: var(--radius-md); padding: var(--space-4) var(--space-4);",

            if editing() {
                // Edit mode
                div {
                    style: "display: flex; flex-direction: column; gap: var(--space-3);",
                    div {
                        style: "display: flex; flex-direction: column; gap: var(--space-1);",
                        label { style: "font-size: var(--text-xs); color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.5px;", "Name" }
                        input {
                            style: "background: var(--input-bg); border: 1px solid var(--border); border-radius: var(--radius-sm); \
                                    padding: var(--space-2) var(--space-3); color: var(--text-primary); font-size: var(--text-sm); width: 100%; box-sizing: border-box;",
                            value: "{edit_name}",
                            oninput: move |e| edit_name.set(e.value()),
                        }
                    }
                    div {
                        style: "display: flex; flex-direction: column; gap: var(--space-1);",
                        label { style: "font-size: var(--text-xs); color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.5px;", "URL" }
                        input {
                            style: "background: var(--input-bg); border: 1px solid var(--border); border-radius: var(--radius-sm); \
                                    padding: var(--space-2) var(--space-3); color: var(--text-primary); font-size: var(--text-sm); width: 100%; box-sizing: border-box;",
                            value: "{edit_url}",
                            oninput: move |e| edit_url.set(e.value()),
                        }
                    }
                    div {
                        style: "display: flex; flex-direction: column; gap: var(--space-1);",
                        label { style: "font-size: var(--text-xs); color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.5px;", "Auth token (leave blank to clear)" }
                        input {
                            r#type: "password",
                            style: "background: var(--input-bg); border: 1px solid var(--border); border-radius: var(--radius-sm); \
                                    padding: var(--space-2) var(--space-3); color: var(--text-primary); font-size: var(--text-sm); width: 100%; box-sizing: border-box;",
                            value: "{edit_token}",
                            oninput: move |e| edit_token.set(e.value()),
                        }
                    }
                    div {
                        style: "display: flex; gap: var(--space-2); justify-content: flex-end;",
                        button {
                            style: "padding: var(--space-2) var(--space-4); background: none; border: 1px solid var(--border); \
                                    border-radius: var(--radius-sm); color: var(--text-secondary); font-size: var(--text-xs); cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
                            onclick: move |_| editing.set(false),
                            "Cancel"
                        }
                        button {
                            style: "padding: var(--space-2) var(--space-4); background: var(--accent); border: none; \
                                    border-radius: var(--radius-sm); color: var(--text-primary); font-size: var(--text-xs); cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
                            onclick: move |_| {
                                let new_name = edit_name();
                                let new_url = edit_url();
                                let new_token_str = edit_token();
                                let new_token = if new_token_str.is_empty() { None } else { Some(new_token_str) };
                                server_store.write().update(&id_for_save, new_name, new_url, new_token);
                                {
                                    let store = server_store.read();
                                    let app = appearance.read();
                                    let keys = keybindings.read();
                                    settings_config::save_state(&store, &app, &keys);
                                }
                                editing.set(false);
                            },
                            "Save"
                        }
                    }
                }
            } else {
                // Display mode
                div {
                    style: "display: flex; justify-content: space-between; align-items: flex-start;",

                    div {
                        style: "display: flex; flex-direction: column; gap: var(--space-1); min-width: 0;",
                        div {
                            style: "display: flex; align-items: center; gap: var(--space-2);",
                            span {
                                style: "font-size: var(--text-base); font-weight: var(--weight-semibold); color: var(--text-primary);",
                                "{name}"
                            }
                            if is_active {
                                span {
                                    style: "font-size: var(--text-xs); padding: 1px var(--space-2); background: var(--bg-surface-dim); \
                                            border: 1px solid var(--accent); border-radius: var(--radius-lg); color: var(--accent-hover);",
                                    "active"
                                }
                            }
                        }
                        span {
                            style: "font-size: var(--text-xs); color: var(--text-muted); word-break: break-all;",
                            "{url}"
                        }
                        div {
                            style: "display: flex; align-items: center; gap: var(--space-2); margin-top: var(--space-1);",
                            div {
                                style: "width: 7px; height: 7px; border-radius: 50%; background: {health_color};",
                            }
                            span {
                                style: "font-size: var(--text-xs); color: {health_color};",
                                if is_testing { "Testing…" } else { "{health_label}" }
                            }
                        }
                    }

                    div {
                        style: "display: flex; gap: var(--space-2); flex-shrink: 0; margin-left: var(--space-3);",
                        button {
                            style: "padding: var(--space-1) var(--space-3); background: none; border: 1px solid var(--border); \
                                    border-radius: var(--radius-sm); color: var(--text-secondary); font-size: var(--text-xs); cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
                            disabled: is_testing,
                            onclick: move |_| on_test.call(()),
                            "Test"
                        }
                        button {
                            style: "padding: var(--space-1) var(--space-3); background: none; border: 1px solid var(--border); \
                                    border-radius: var(--radius-sm); color: var(--text-secondary); font-size: var(--text-xs); cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
                            onclick: move |_| editing.set(true),
                            "Edit"
                        }
                        if !is_active {
                            button {
                                style: "padding: var(--space-1) var(--space-3); background: var(--border); border: 1px solid var(--accent); \
                                        border-radius: var(--radius-sm); color: var(--accent-hover); font-size: var(--text-xs); cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
                                onclick: move |_| on_switch.call(()),
                                "Switch"
                            }
                            button {
                                style: "padding: var(--space-1) var(--space-3); background: none; border: 1px solid var(--status-error-bg); \
                                        border-radius: var(--radius-sm); color: var(--status-error); font-size: var(--text-xs); cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
                                onclick: move |_| on_remove.call(()),
                                "Remove"
                            }
                        }
                    }
                }
            }
        }
    }
}

// --- Add server form ---

#[component]
fn AddServerForm(server_store: Signal<ServerConfigStore>, on_saved: EventHandler<()>) -> Element {
    let mut name = use_signal(|| "My Server".to_string());
    let mut url = use_signal(|| "http://localhost:3000".to_string());
    let mut token = use_signal(String::new);
    let appearance = use_context::<Signal<crate::state::settings::AppearanceSettings>>();
    let keybindings = use_context::<Signal<crate::state::settings::KeybindingStore>>();

    rsx! {
        div {
            style: "background: var(--bg-surface); border: 1px solid var(--accent); border-radius: var(--radius-md); padding: var(--space-4);",
            h4 { style: "margin: 0 0 var(--space-4); font-size: var(--text-base); color: var(--text-primary);", "Add Server" }

            div {
                style: "display: flex; flex-direction: column; gap: var(--space-3);",
                div {
                    style: "display: flex; flex-direction: column; gap: var(--space-1);",
                    label { style: "font-size: var(--text-xs); color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.5px;", "Name" }
                    input {
                        style: "background: var(--input-bg); border: 1px solid var(--border); border-radius: var(--radius-sm); \
                                padding: var(--space-2) var(--space-3); color: var(--text-primary); font-size: var(--text-sm); width: 100%; box-sizing: border-box;",
                        value: "{name}",
                        oninput: move |e| name.set(e.value()),
                    }
                }
                div {
                    style: "display: flex; flex-direction: column; gap: var(--space-1);",
                    label { style: "font-size: var(--text-xs); color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.5px;", "Server URL" }
                    input {
                        style: "background: var(--input-bg); border: 1px solid var(--border); border-radius: var(--radius-sm); \
                                padding: var(--space-2) var(--space-3); color: var(--text-primary); font-size: var(--text-sm); width: 100%; box-sizing: border-box;",
                        value: "{url}",
                        oninput: move |e| url.set(e.value()),
                    }
                }
                div {
                    style: "display: flex; flex-direction: column; gap: var(--space-1);",
                    label { style: "font-size: var(--text-xs); color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.5px;", "Auth token (optional)" }
                    input {
                        r#type: "password",
                        style: "background: var(--input-bg); border: 1px solid var(--border); border-radius: var(--radius-sm); \
                                padding: var(--space-2) var(--space-3); color: var(--text-primary); font-size: var(--text-sm); width: 100%; box-sizing: border-box;",
                        value: "{token}",
                        oninput: move |e| token.set(e.value()),
                    }
                }
                div {
                    style: "display: flex; justify-content: flex-end;",
                    button {
                        style: "padding: var(--space-2) var(--space-4); background: var(--accent); border: none; \
                                border-radius: var(--radius-md); color: var(--text-primary); font-size: var(--text-sm); cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
                        onclick: move |_| {
                            let n = name();
                            let u = url();
                            let t = token();
                            let auth_token = if t.is_empty() { None } else { Some(t) };
                            server_store.write().add(n, u, auth_token);
                            {
                                let store = server_store.read();
                                let app = appearance.read();
                                let keys = keybindings.read();
                                settings_config::save_state(&store, &app, &keys);
                            }
                            on_saved.call(());
                        },
                        "Add"
                    }
                }
            }
        }
    }
}
