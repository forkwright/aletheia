//! Server connections management panel.
//!
//! Displays saved server entries with health indicators. Supports add, edit,
//! remove, test-connection, and switch-to-server actions. When the live
//! connection URL drifts from the active entry's saved URL, the entry offers
//! a one-click "Update to current".

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;

use crate::api::system_status::{SystemStatusFetchError, fetch_system_status};
use crate::services::settings_config;
use crate::state::connection::{ConnectionConfig, ConnectionState};
use crate::state::settings::{ServerConfigStore, ServerHealth};

// ── Plain data snapshot (avoids borrow-through-Signal in RSX) ──

#[derive(Clone)]
struct ServerSnap {
    id: String,
    name: String,
    url: String,
    auth_token: Option<String>,
    is_active: bool,
}

// ── Async helpers ──

/// Probe a saved server's backend subsystem health (#5315).
///
/// Uses [`fetch_system_status`] (`GET /api/v1/system/status`) rather than
/// the plain liveness check, so the result distinguishes a fully healthy
/// server from one that is reachable but degraded/unhealthy, and from one
/// whose token cannot see health at all. Returns the reduced
/// [`ServerHealth`] alongside the names of any non-healthy subsystems.
async fn probe_health(url: &str, token: Option<&str>) -> (ServerHealth, Vec<String>) {
    let config = ConnectionConfig {
        server_url: url.to_string(),
        auth_token: token.map(str::to_string),
        auto_reconnect: false,
        ..ConnectionConfig::default()
    };
    match fetch_system_status(&config).await {
        Ok(response) => {
            let failing = response.failing_names();
            let health = match response.status.as_str() {
                "healthy" => ServerHealth::Healthy,
                "degraded" => ServerHealth::Degraded,
                // WHY: "failed" and any unrecognized aggregate value both
                // report as Unhealthy rather than defaulting to Healthy.
                _ => ServerHealth::Unhealthy,
            };
            (health, failing)
        }
        Err(SystemStatusFetchError::Unauthorized) => (ServerHealth::Unauthorized, Vec::new()),
        Err(err) if err.is_invalid_token() => (ServerHealth::InvalidToken, Vec::new()),
        Err(_) => (ServerHealth::Unreachable, Vec::new()),
    }
}

// ── Main panel ──

/// Server connections management panel.
#[component]
pub(crate) fn ServersPanel() -> Element {
    let mut server_store: Signal<ServerConfigStore> = use_context();
    let mut connection_config: Signal<ConnectionConfig> = use_context();
    let mut connection_state: Signal<ConnectionState> = use_context();
    let appearance = use_context::<Signal<crate::state::settings::AppearanceSettings>>();
    let keybindings = use_context::<Signal<crate::state::settings::KeybindingStore>>();

    // WHY: Health results carry the URL they probed so a stale result is
    // attributable ("Unreachable — <url>") instead of an anonymous failure,
    // plus the names of any non-healthy subsystems (#5315).
    let mut health_map: Signal<HashMap<String, (ServerHealth, String, Vec<String>)>> =
        use_signal(HashMap::new);
    let mut testing_ids: Signal<HashSet<String>> = use_signal(HashSet::new);
    let mut show_add = use_signal(|| false);

    // WHY: Pre-collect snapshots before RSX to avoid borrow-through-Signal.
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

    // Live connection URL, present only while actually connected. Drives the
    // "Update to current" offer when the active entry's saved URL has drifted.
    let connected_url: Option<String> = if matches!(
        connection_state(),
        ConnectionState::Connected | ConnectionState::ConnectedDegraded { .. }
    ) {
        Some(connection_config.read().server_url.clone())
    } else {
        None
    };

    rsx! {
        div {
            style: "display: flex; flex-direction: column; gap: var(--space-4); max-width: 680px;",

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

            if show_add() {
                AddServerForm {
                    server_store,
                    on_saved: move |_| { show_add.set(false); }
                }
            }

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
                    let sid_update = sid.clone();
                    let sid_saved = sid.clone();
                    let surl = snap.url.clone();
                    let stoken = snap.auth_token.clone();
                    let (health, tested_url, failing) = health_map
                        .read()
                        .get(&snap.id)
                        .map(|(h, u, f)| (*h, Some(u.clone()), f.clone()))
                        .unwrap_or((ServerHealth::Unchecked, None, Vec::new()));
                    let is_testing = testing_ids.read().contains(&snap.id);
                    // Offer the live URL only on the active entry; other saved
                    // entries are intentionally different servers.
                    let update_url = connected_url
                        .clone()
                        .filter(|u| snap.is_active && *u != snap.url);
                    let update_url_apply = update_url.clone();
                    let name_update = snap.name.clone();

                    rsx! {
                        ServerCard {
                            key: "{sid}",
                            id: sid.clone(),
                            name: snap.name.clone(),
                            url: surl.clone(),
                            auth_token: stoken.clone(),
                            is_active: snap.is_active,
                            health,
                            tested_url,
                            failing,
                            update_url,
                            is_testing,
                            on_test: move |_| {
                                let url = surl.clone();
                                let token = stoken.clone();
                                let id = sid_test.clone();
                                testing_ids.write().insert(id.clone());
                                health_map.write().remove(&id);
                                spawn(async move {
                                    let (health, failing) = probe_health(&url, token.as_deref()).await;
                                    testing_ids.write().remove(&id);
                                    health_map.write().insert(id, (health, url, failing));
                                });
                            },
                            on_update_url: move |_| {
                                if let Some(u) = update_url_apply.clone() {
                                    server_store.write().update_identity(
                                        &sid_update,
                                        name_update.clone(),
                                        u,
                                    );
                                    health_map.write().remove(&sid_update);
                                    let store = server_store.read();
                                    let app = appearance.read();
                                    let keys = keybindings.read();
                                    settings_config::save_state(&store, &app, &keys);
                                }
                            },
                            on_saved: move |_| {
                                // WHY: A health result probed against the pre-edit URL
                                // must not linger next to the new one.
                                health_map.write().remove(&sid_saved);
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

// ── Server card ──

#[component]
fn ServerCard(
    id: String,
    name: String,
    url: String,
    auth_token: Option<String>,
    is_active: bool,
    health: ServerHealth,
    tested_url: Option<String>,
    /// Names of non-healthy subsystems from the last probe (#5315). Empty
    /// when unchecked, healthy, or the failure was reachability/auth rather
    /// than a subsystem report.
    failing: Vec<String>,
    update_url: Option<String>,
    is_testing: bool,
    on_test: EventHandler<()>,
    on_update_url: EventHandler<()>,
    on_saved: EventHandler<()>,
    on_switch: EventHandler<()>,
    on_remove: EventHandler<()>,
) -> Element {
    let mut editing = use_signal(|| false);
    let mut edit_name = use_signal(|| name.clone());
    let mut edit_url = use_signal(|| url.clone());
    let mut edit_token = use_signal(|| auth_token.clone().unwrap_or_else(String::new));

    let mut server_store: Signal<ServerConfigStore> = use_context();
    let appearance = use_context::<Signal<crate::state::settings::AppearanceSettings>>();
    let keybindings = use_context::<Signal<crate::state::settings::KeybindingStore>>();

    let health_color = health.color();
    let status_text = if is_testing {
        "Testing…".to_string()
    } else if health == ServerHealth::Unreachable {
        // WHY: Name the URL the probe actually hit so a stale saved URL is
        // self-diagnosing instead of an anonymous "Unreachable".
        match tested_url.as_deref() {
            Some(tried) => format!("Unreachable — {tried}"),
            None => health.label().to_string(),
        }
    } else {
        health.label().to_string()
    };
    let card_border = if is_active {
        "1px solid var(--accent)"
    } else {
        "1px solid var(--border)"
    };

    let id_for_save = id.clone();

    rsx! {
        div {
            style: "background: var(--bg-surface); border: {card_border}; border-radius: var(--radius-md); padding: var(--space-4) var(--space-4);",

            if editing() {
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
                                    border-radius: var(--radius-sm); color: var(--text-inverse); font-size: var(--text-xs); cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
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
                                on_saved.call(());
                            },
                            "Save"
                        }
                    }
                }
            } else {
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
                                style: "font-size: var(--text-xs); color: {health_color}; word-break: break-all;",
                                "{status_text}"
                            }
                        }
                        if !failing.is_empty() {
                            div {
                                style: "font-size: var(--text-xs); color: var(--text-muted); margin-top: 2px;",
                                "Failing: {failing.join(\", \")}"
                            }
                        }
                        if let Some(live_url) = update_url.clone() {
                            div {
                                style: "display: flex; align-items: center; gap: var(--space-2); margin-top: var(--space-1); flex-wrap: wrap;",
                                span {
                                    style: "font-size: var(--text-xs); color: var(--status-warning); word-break: break-all;",
                                    "Connected to {live_url}"
                                }
                                button {
                                    style: "padding: var(--space-1) var(--space-3); background: var(--border); border: 1px solid var(--accent); \
                                            border-radius: var(--radius-sm); color: var(--accent-hover); font-size: var(--text-xs); cursor: pointer; \
                                            transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
                                    onclick: move |_| on_update_url.call(()),
                                    "Update to current"
                                }
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

// ── Add server form ──

#[component]
fn AddServerForm(server_store: Signal<ServerConfigStore>, on_saved: EventHandler<()>) -> Element {
    let mut name = use_signal(|| "My Server".to_string());
    // WHY: skene's discovery config owns the gateway port default; deriving it
    // here keeps the form default from drifting when that port changes.
    let mut url = use_signal(|| {
        let port = skene::discovery::DiscoveryConfig::default().port;
        format!("http://localhost:{port}") // kanon:ignore SECURITY/hardcoded-loopback-url -- UI form default; user replaces with actual server URL on save
    });
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
                                border-radius: var(--radius-md); color: var(--text-inverse); font-size: var(--text-sm); cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);",
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

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions may panic on failure")]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    fn install_crypto() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn status_body(status: &str, subsystem_status: &str) -> String {
        serde_json::json!({
            "status": status,
            "generated_at": "2026-01-01T00:00:00Z",
            "subsystems": [
                {"id": "embeddings", "name": "Embedding Provider", "status": subsystem_status},
            ],
        })
        .to_string()
    }

    async fn spawn_status_server(http_status: u16, body: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut buf = [0_u8; 4096];
            let _ = stream.read(&mut buf).await;
            let reason = match http_status {
                200 => "OK",
                401 => "Unauthorized",
                _ => "Error",
            };
            let response = format!(
                "HTTP/1.1 {http_status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn probe_health_healthy_server_reports_no_failing_subsystems() {
        install_crypto();
        let url = spawn_status_server(200, status_body("healthy", "healthy")).await;
        let (health, failing) = probe_health(&url, None).await;
        assert_eq!(health, ServerHealth::Healthy);
        assert!(failing.is_empty());
    }

    #[tokio::test]
    async fn probe_health_degraded_server_names_the_failing_subsystem() {
        install_crypto();
        let url = spawn_status_server(200, status_body("degraded", "degraded")).await;
        let (health, failing) = probe_health(&url, None).await;
        assert_eq!(health, ServerHealth::Degraded);
        assert_eq!(failing, vec!["Embedding Provider".to_string()]);
    }

    #[tokio::test]
    async fn probe_health_unauthorized_is_distinct_from_unreachable() {
        install_crypto();
        let url = spawn_status_server(401, "{}".to_string()).await;
        let (health, failing) = probe_health(&url, None).await;
        assert_eq!(health, ServerHealth::Unauthorized);
        assert!(failing.is_empty());
        assert_ne!(health, ServerHealth::Unreachable);
    }

    #[tokio::test]
    async fn probe_health_unreachable_on_closed_port() {
        install_crypto();
        let (health, failing) = probe_health("http://127.0.0.1:1", None).await;
        assert_eq!(health, ServerHealth::Unreachable);
        assert!(failing.is_empty());
    }

    #[tokio::test]
    async fn probe_health_invalid_token_reported_before_any_request() {
        install_crypto();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (health, _) = probe_health(&url, Some("bad\x00token")).await;
        assert_eq!(health, ServerHealth::InvalidToken);
        let accepted =
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept()).await;
        assert!(accepted.is_err(), "invalid token must not reach the server");
    }
}
