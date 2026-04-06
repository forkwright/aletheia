//! Toggle controls panel: agent enable/disable, tool toggles, feature flags.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::ops::ToggleStore;

const PANEL_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px; \
    flex: 1; \
    overflow-y: auto; \
    min-width: 280px;\
";

const SECTION_TITLE: &str = "\
    font-size: 14px; \
    font-weight: bold; \
    color: #aaa; \
    margin-bottom: 10px;\
";

const SUBSECTION_TITLE: &str = "\
    font-size: 12px; \
    font-weight: bold; \
    color: #888; \
    margin: 12px 0 6px 0; \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    padding: 6px 0; \
    border-bottom: 1px solid #222;\
";

const TOGGLE_LABEL: &str = "\
    color: #e0e0e0; \
    font-size: 13px;\
";

const TOOL_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    padding: 4px 0 4px 16px; \
    border-bottom: 1px solid #1a1a2e; \
    font-size: 12px;\
";

const TOOL_LABEL: &str = "\
    color: #aaa;\
";

const EXPAND_BTN: &str = "\
    background: none; \
    border: none; \
    color: #888; \
    cursor: pointer; \
    font-size: 11px; \
    padding: 2px 6px;\
";

const FLAG_DESC: &str = "\
    color: #666; \
    font-size: 11px; \
    padding: 0 0 6px 0;\
";

const EMPTY_STATE: &str = "\
    color: #555; \
    font-size: 12px; \
    padding: 4px 0;\
";

const CONFIRM_OVERLAY: &str = "\
    position: fixed; \
    top: 0; left: 0; right: 0; bottom: 0; \
    background: rgba(0, 0, 0, 0.5); \
    display: flex; \
    align-items: center; \
    justify-content: center; \
    z-index: 100;\
";

const CONFIRM_BOX: &str = "\
    background: #1a1a2e; \
    border: 1px solid #444; \
    border-radius: 8px; \
    padding: 24px; \
    max-width: 400px; \
    text-align: center;\
";

const CONFIRM_BTN: &str = "\
    padding: 6px 16px; \
    border-radius: 6px; \
    border: 1px solid #444; \
    cursor: pointer; \
    font-size: 13px; \
    margin: 0 4px;\
";

#[component]
pub(crate) fn ToggleControlsPanel(
    store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
) -> Element {
    let confirm_disable: Signal<Option<theatron_core::id::NousId>> = use_signal(|| None);

    // WHY: Collect into owned data to avoid holding signal read across rsx boundaries.
    let agent_ids: Vec<_> = {
        let data = store.read();
        data.agent_toggles
            .iter()
            .map(|t| (t.id.clone(), t.name.clone(), t.enabled, t.pending))
            .collect()
    };

    let flag_data: Vec<_> = {
        let data = store.read();
        data.feature_flags
            .iter()
            .map(|f| (f.key.clone(), f.description.clone(), f.enabled, f.pending))
            .collect()
    };

    rsx! {
        div {
            style: "{PANEL_STYLE}",

            div { style: "{SECTION_TITLE}", "Controls" }

            div { style: "{SUBSECTION_TITLE}", "Agents" }

            if agent_ids.is_empty() {
                div { style: "{EMPTY_STATE}", "No agents available" }
            }

            for (id , name , enabled , pending) in agent_ids {
                AgentToggleRow {
                    key: "{id}",
                    id: id.clone(),
                    name,
                    enabled,
                    pending,
                    store,
                    config,
                    confirm_disable,
                }
            }

            div { style: "{SUBSECTION_TITLE}", "Feature Flags" }

            if flag_data.is_empty() {
                div { style: "{EMPTY_STATE}", "No feature flags configured" }
            }

            for (key , description , enabled , pending) in flag_data {
                FeatureFlagRow {
                    key: "{key}",
                    flag_key: key,
                    description,
                    enabled,
                    pending,
                    store,
                    config,
                }
            }
        }

        if let Some(ref agent_id) = *confirm_disable.read() {
            ConfirmDisableDialog {
                agent_id: agent_id.clone(),
                store,
                config,
                confirm_disable,
            }
        }
    }
}

// WHY: Each toggle row is a #[component] so onclick handlers have direct
// mutable access to Signal (Fn closures inside RSX for-loops prevent
// Signal::set which requires &mut self).

#[component]
fn AgentToggleRow(
    id: theatron_core::id::NousId,
    name: String,
    enabled: bool,
    pending: bool,
    store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    mut confirm_disable: Signal<Option<theatron_core::id::NousId>>,
) -> Element {
    let is_expanded = store
        .read()
        .expanded_agent
        .as_ref()
        .map_or(false, |e| *e == id);

    let expand_label = if is_expanded {
        "tools \u{25bc}"
    } else {
        "tools \u{25b6}"
    };

    // WHY: Collect tool data while we have the read lock.
    let tools: Vec<_> = if is_expanded {
        let data = store.read();
        data.tools_for_agent(&id)
            .iter()
            .map(|t| {
                (
                    t.agent_id.clone(),
                    t.tool_name.clone(),
                    t.enabled,
                    t.pending,
                )
            })
            .collect()
    } else {
        Vec::new()
    };

    rsx! {
        div {
            style: "{ROW_STYLE}",
            div {
                style: "display: flex; align-items: center; gap: 8px;",
                span { style: "{TOGGLE_LABEL}", "{name}" }
                button {
                    style: "{EXPAND_BTN}",
                    onclick: {
                        let id = id.clone();
                        move |_| {
                            let mut ts = store.write();
                            if ts.expanded_agent.as_ref() == Some(&id) {
                                ts.expanded_agent = None;
                            } else {
                                ts.expanded_agent = Some(id.clone());
                            }
                        }
                    },
                    "{expand_label}"
                }
            }
            {toggle_switch(
                enabled,
                pending,
                {
                    let id = id.clone();
                    move |_: Event<MouseData>| {
                        if enabled {
                            request_confirm(confirm_disable, id.clone());
                        } else {
                            fire_agent_toggle(store, config, id.clone());
                        }
                    }
                },
            )}
        }

        if is_expanded {
            for (aid , tname , tool_enabled , tool_pending) in tools {
                ToolToggleRow {
                    key: "{aid}-{tname}",
                    agent_id: aid,
                    tool_name: tname,
                    enabled: tool_enabled,
                    pending: tool_pending,
                    store,
                    config,
                }
            }
        }
    }
}

#[component]
fn ToolToggleRow(
    agent_id: theatron_core::id::NousId,
    tool_name: String,
    enabled: bool,
    pending: bool,
    store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
) -> Element {
    rsx! {
        div {
            style: "{TOOL_ROW_STYLE}",
            span { style: "{TOOL_LABEL}", "{tool_name}" }
            {toggle_switch(
                enabled,
                pending,
                {
                    let aid = agent_id.clone();
                    let tname = tool_name.clone();
                    move |_: Event<MouseData>| {
                        fire_tool_toggle(store, config, aid.clone(), tname.clone());
                    }
                },
            )}
        }
    }
}

#[component]
fn FeatureFlagRow(
    flag_key: String,
    description: String,
    enabled: bool,
    pending: bool,
    store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
) -> Element {
    rsx! {
        div {
            div {
                style: "{ROW_STYLE}",
                span { style: "{TOGGLE_LABEL}", "{flag_key}" }
                {toggle_switch(
                    enabled,
                    pending,
                    {
                        let key = flag_key.clone();
                        move |_: Event<MouseData>| {
                            fire_feature_toggle(store, config, key.clone());
                        }
                    },
                )}
            }
            if !description.is_empty() {
                div { style: "{FLAG_DESC}", "{description}" }
            }
        }
    }
}

#[component]
fn ConfirmDisableDialog(
    agent_id: theatron_core::id::NousId,
    store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    mut confirm_disable: Signal<Option<theatron_core::id::NousId>>,
) -> Element {
    let name = store
        .read()
        .agent_toggles
        .iter()
        .find(|t| t.id == agent_id)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| agent_id.to_string());

    rsx! {
        div {
            style: "{CONFIRM_OVERLAY}",
            onclick: move |_| confirm_disable.set(None),
            div {
                style: "{CONFIRM_BOX}",
                onclick: move |e| e.stop_propagation(),
                p {
                    style: "color: #e0e0e0; margin: 0 0 16px 0;",
                    "Disable agent \"{name}\"?"
                }
                p {
                    style: "color: #888; font-size: 12px; margin: 0 0 20px 0;",
                    "Active sessions will be interrupted."
                }
                div {
                    button {
                        style: "{CONFIRM_BTN} background: #3a1a1a; color: #ef4444;",
                        onclick: {
                            let id = agent_id.clone();
                            move |_| {
                                fire_agent_toggle(store, config, id.clone());
                                confirm_disable.set(None);
                            }
                        },
                        "Disable"
                    }
                    button {
                        style: "{CONFIRM_BTN} background: #2a2a4a; color: #e0e0e0;",
                        onclick: move |_| confirm_disable.set(None),
                        "Cancel"
                    }
                }
            }
        }
    }
}

fn toggle_switch(
    enabled: bool,
    pending: bool,
    on_click: impl Fn(Event<MouseData>) + 'static,
) -> Element {
    let track_style = if pending {
        "width: 36px; height: 20px; border-radius: 10px; background: #888; position: relative; cursor: wait; opacity: 0.6; flex-shrink: 0;"
    } else if enabled {
        "width: 36px; height: 20px; border-radius: 10px; background: #22c55e; position: relative; cursor: pointer; flex-shrink: 0;"
    } else {
        "width: 36px; height: 20px; border-radius: 10px; background: #555; position: relative; cursor: pointer; flex-shrink: 0;"
    };

    let knob_style = if enabled {
        "width: 16px; height: 16px; border-radius: 50%; background: white; position: absolute; top: 2px; left: 18px;"
    } else {
        "width: 16px; height: 16px; border-radius: 50%; background: white; position: absolute; top: 2px; left: 2px;"
    };

    rsx! {
        div {
            style: "{track_style}",
            onclick: move |e| {
                if !pending {
                    on_click(e);
                }
            },
            div { style: "{knob_style}" }
        }
    }
}

// WHY: Signal::set requires &mut self, which is unavailable inside Fn closures.
// Passing Signal by value to a function with `mut` parameter sidesteps this.
fn request_confirm(
    mut sig: Signal<Option<theatron_core::id::NousId>>,
    id: theatron_core::id::NousId,
) {
    sig.set(Some(id));
}

fn fire_agent_toggle(
    mut store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    id: theatron_core::id::NousId,
) {
    let prev = store.write().flip_agent(&id);
    let Some(prev_val) = prev else { return };

    let cfg = config.read().clone();
    let agent_id = id.clone();

    spawn(async move {
        let client = authenticated_client(&cfg);
        let base = cfg.server_url.trim_end_matches('/');
        let new_enabled = !prev_val;
        let url = format!("{base}/api/v1/nous/{agent_id}");

        let result = client
            .patch(&url)
            .json(&serde_json::json!({ "enabled": new_enabled }))
            .send()
            .await;

        let success = matches!(result, Ok(ref r) if r.status().is_success());
        store.write().resolve_agent(&id, success, prev_val);
    });
}

fn fire_tool_toggle(
    mut store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    agent_id: theatron_core::id::NousId,
    tool_name: String,
) {
    let prev = store.write().flip_tool(&agent_id, &tool_name);
    let Some(prev_val) = prev else { return };

    let cfg = config.read().clone();
    let aid = agent_id.clone();
    let tname = tool_name.clone();

    spawn(async move {
        let client = authenticated_client(&cfg);
        let base = cfg.server_url.trim_end_matches('/');
        let new_enabled = !prev_val;
        let url = format!("{base}/api/v1/nous/{aid}/tools");

        let result = client
            .patch(&url)
            .json(&serde_json::json!({ "tool": tname, "enabled": new_enabled }))
            .send()
            .await;

        let success = matches!(result, Ok(ref r) if r.status().is_success());
        store
            .write()
            .resolve_tool(&agent_id, &tool_name, success, prev_val);
    });
}

fn fire_feature_toggle(
    mut store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    key: String,
) {
    let prev = store.write().flip_feature(&key);
    let Some(prev_val) = prev else { return };

    let cfg = config.read().clone();
    let flag_key = key.clone();

    spawn(async move {
        let client = authenticated_client(&cfg);
        let base = cfg.server_url.trim_end_matches('/');
        let new_enabled = !prev_val;
        let url = format!("{base}/api/v1/config");

        let result = client
            .patch(&url)
            .json(&serde_json::json!({
                "feature_flags": { flag_key: new_enabled }
            }))
            .send()
            .await;

        let success = matches!(result, Ok(ref r) if r.status().is_success());
        store.write().resolve_feature(&key, success, prev_val);
    });
}
