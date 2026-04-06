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
            style: "display: flex; flex-direction: column; gap: 16px; max-width: 680px;",

            // Header
            div {
                style: "display: flex; justify-content: space-between; align-items: center;",
                h3 { style: "margin: 0; font-size: 16px; color: #e0e0e0;", "Server Connections" }
                button {
                    style: "padding: 6px 14px; background: #2a2a4a; border: 1px solid #444; \
                            border-radius: 6px; color: #e0e0e0; font-size: 13px; cursor: pointer;",
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
                    style: "padding: 32px; text-align: center; color: #555; font-size: 14px; \
                            background: #1a1a2e; border: 1px solid #333; border-radius: 8px;",
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
    let card_border = if is_active { "1px solid #5b6af0" } else { "1px solid #333" };

    let id_for_save = id.clone();

    rsx! {
        div {
            style: "background: #1a1a2e; border: {card_border}; border-radius: 8px; padding: 14px 16px;",

            if editing() {
                // Edit mode
                div {
                    style: "display: flex; flex-direction: column; gap: 10px;",
                    div {
                        style: "display: flex; flex-direction: column; gap: 4px;",
                        label { style: "font-size: 11px; color: #888; text-transform: uppercase; letter-spacing: 0.5px;", "Name" }
                        input {
                            style: "background: #0d0d1a; border: 1px solid #444; border-radius: 4px; \
                                    padding: 6px 10px; color: #e0e0e0; font-size: 13px; width: 100%; box-sizing: border-box;",
                            value: "{edit_name}",
                            oninput: move |e| edit_name.set(e.value()),
                        }
                    }
                    div {
                        style: "display: flex; flex-direction: column; gap: 4px;",
                        label { style: "font-size: 11px; color: #888; text-transform: uppercase; letter-spacing: 0.5px;", "URL" }
                        input {
                            style: "background: #0d0d1a; border: 1px solid #444; border-radius: 4px; \
                                    padding: 6px 10px; color: #e0e0e0; font-size: 13px; width: 100%; box-sizing: border-box;",
                            value: "{edit_url}",
                            oninput: move |e| edit_url.set(e.value()),
                        }
                    }
                    div {
                        style: "display: flex; flex-direction: column; gap: 4px;",
                        label { style: "font-size: 11px; color: #888; text-transform: uppercase; letter-spacing: 0.5px;", "Auth token (leave blank to clear)" }
                        input {
                            r#type: "password",
                            style: "background: #0d0d1a; border: 1px solid #444; border-radius: 4px; \
                                    padding: 6px 10px; color: #e0e0e0; font-size: 13px; width: 100%; box-sizing: border-box;",
                            value: "{edit_token}",
                            oninput: move |e| edit_token.set(e.value()),
                        }
                    }
                    div {
                        style: "display: flex; gap: 8px; justify-content: flex-end;",
                        button {
                            style: "padding: 6px 14px; background: none; border: 1px solid #444; \
                                    border-radius: 4px; color: #888; font-size: 12px; cursor: pointer;",
                            onclick: move |_| editing.set(false),
                            "Cancel"
                        }
                        button {
                            style: "padding: 6px 14px; background: #5b6af0; border: none; \
                                    border-radius: 4px; color: #fff; font-size: 12px; cursor: pointer;",
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
                        style: "display: flex; flex-direction: column; gap: 4px; min-width: 0;",
                        div {
                            style: "display: flex; align-items: center; gap: 8px;",
                            span {
                                style: "font-size: 14px; font-weight: 600; color: #e0e0e0;",
                                "{name}"
                            }
                            if is_active {
                                span {
                                    style: "font-size: 10px; padding: 1px 6px; background: #1a1a4a; \
                                            border: 1px solid #5b6af0; border-radius: 10px; color: #8899ff;",
                                    "active"
                                }
                            }
                        }
                        span {
                            style: "font-size: 12px; color: #666; word-break: break-all;",
                            "{url}"
                        }
                        div {
                            style: "display: flex; align-items: center; gap: 6px; margin-top: 4px;",
                            div {
                                style: "width: 7px; height: 7px; border-radius: 50%; background: {health_color};",
                            }
                            span {
                                style: "font-size: 11px; color: {health_color};",
                                if is_testing { "Testing…" } else { "{health_label}" }
                            }
                        }
                    }

                    div {
                        style: "display: flex; gap: 6px; flex-shrink: 0; margin-left: 12px;",
                        button {
                            style: "padding: 4px 10px; background: none; border: 1px solid #444; \
                                    border-radius: 4px; color: #888; font-size: 11px; cursor: pointer;",
                            disabled: is_testing,
                            onclick: move |_| on_test.call(()),
                            "Test"
                        }
                        button {
                            style: "padding: 4px 10px; background: none; border: 1px solid #444; \
                                    border-radius: 4px; color: #888; font-size: 11px; cursor: pointer;",
                            onclick: move |_| editing.set(true),
                            "Edit"
                        }
                        if !is_active {
                            button {
                                style: "padding: 4px 10px; background: #2a2a4a; border: 1px solid #5b6af0; \
                                        border-radius: 4px; color: #8899ff; font-size: 11px; cursor: pointer;",
                                onclick: move |_| on_switch.call(()),
                                "Switch"
                            }
                            button {
                                style: "padding: 4px 10px; background: none; border: 1px solid #4a2222; \
                                        border-radius: 4px; color: #f87171; font-size: 11px; cursor: pointer;",
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
            style: "background: #161628; border: 1px solid #5b6af0; border-radius: 8px; padding: 16px;",
            h4 { style: "margin: 0 0 14px; font-size: 14px; color: #e0e0e0;", "Add Server" }

            div {
                style: "display: flex; flex-direction: column; gap: 10px;",
                div {
                    style: "display: flex; flex-direction: column; gap: 4px;",
                    label { style: "font-size: 11px; color: #888; text-transform: uppercase; letter-spacing: 0.5px;", "Name" }
                    input {
                        style: "background: #0d0d1a; border: 1px solid #444; border-radius: 4px; \
                                padding: 6px 10px; color: #e0e0e0; font-size: 13px; width: 100%; box-sizing: border-box;",
                        value: "{name}",
                        oninput: move |e| name.set(e.value()),
                    }
                }
                div {
                    style: "display: flex; flex-direction: column; gap: 4px;",
                    label { style: "font-size: 11px; color: #888; text-transform: uppercase; letter-spacing: 0.5px;", "Server URL" }
                    input {
                        style: "background: #0d0d1a; border: 1px solid #444; border-radius: 4px; \
                                padding: 6px 10px; color: #e0e0e0; font-size: 13px; width: 100%; box-sizing: border-box;",
                        value: "{url}",
                        oninput: move |e| url.set(e.value()),
                    }
                }
                div {
                    style: "display: flex; flex-direction: column; gap: 4px;",
                    label { style: "font-size: 11px; color: #888; text-transform: uppercase; letter-spacing: 0.5px;", "Auth token (optional)" }
                    input {
                        r#type: "password",
                        style: "background: #0d0d1a; border: 1px solid #444; border-radius: 4px; \
                                padding: 6px 10px; color: #e0e0e0; font-size: 13px; width: 100%; box-sizing: border-box;",
                        value: "{token}",
                        oninput: move |e| token.set(e.value()),
                    }
                }
                div {
                    style: "display: flex; justify-content: flex-end;",
                    button {
                        style: "padding: 7px 18px; background: #5b6af0; border: none; \
                                border-radius: 6px; color: #fff; font-size: 13px; cursor: pointer;",
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
