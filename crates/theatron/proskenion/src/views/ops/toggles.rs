//! Toggle controls panel: agent enable/disable, tool toggles, feature flags.

use dioxus::prelude::*;
use skene::api::routes::{
    config::{feature_flags_url, reload_url},
    nous::{agent_tools_url, agent_url},
};
use skeue::EmptyState;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::ops::{ReloadOutcome, ToggleActionResult, ToggleApplyState, ToggleStore};

const PANEL_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4); \
    flex: 1; \
    overflow-y: auto; \
    min-width: 280px;\
";

const SECTION_TITLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-bold); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-3);\
";

const SUBSECTION_TITLE: &str = "\
    font-size: var(--text-xs); \
    font-weight: var(--weight-bold); \
    color: var(--text-secondary); \
    margin: var(--space-3) 0 var(--space-2) 0; \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    padding: var(--space-2) 0; \
    border-bottom: 1px solid var(--border-separator);\
";

const TOGGLE_LABEL: &str = "\
    color: var(--text-primary); \
    font-size: var(--text-sm);\
";

const TOOL_ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    padding: var(--space-1) 0 var(--space-1) var(--space-4); \
    border-bottom: 1px solid var(--bg-surface); \
    font-size: var(--text-xs);\
";

const TOOL_LABEL: &str = "\
    color: var(--text-secondary);\
";

const EXPAND_BTN: &str = "\
    background: none; \
    border: none; \
    color: var(--text-secondary); \
    cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); \
    font-size: var(--text-xs); \
    padding: var(--space-1) var(--space-2);\
";

const FLAG_DESC: &str = "\
    color: var(--text-muted); \
    font-size: var(--text-xs); \
    padding: 0 0 var(--space-2) 0;\
";

const STATUS_BADGE_WARNING: &str = "\
    color: var(--status-warning); \
    font-size: var(--text-xs); \
    font-weight: var(--weight-bold);\
";

const STATUS_BADGE_ERROR: &str = "\
    color: var(--status-error); \
    font-size: var(--text-xs); \
    font-weight: var(--weight-bold);\
";

const CONFIRM_OVERLAY: &str = "\
    position: fixed; \
    top: 0; left: 0; right: 0; bottom: 0; \
    background: var(--bg-overlay); \
    display: flex; \
    align-items: center; \
    justify-content: center; \
    z-index: 100;\
";

const CONFIRM_BOX: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-6); \
    max-width: 400px; \
    text-align: center;\
";

const RELOAD_BTN: &str = "\
    background: var(--accent); \
    color: var(--text-inverse); \
    border: 1px solid var(--accent); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);\
";

const RELOAD_BTN_DISABLED: &str = "\
    background: var(--bg-surface); \
    color: var(--text-muted); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    cursor: not-allowed;\
";

const CONFIRM_BTN: &str = "\
    padding: var(--space-2) var(--space-4); \
    border-radius: var(--radius-md); \
    border: 1px solid var(--border); \
    cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); \
    font-size: var(--text-sm); \
    margin: 0 var(--space-1);\
";

#[component]
pub(crate) fn ToggleControlsPanel(
    store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
) -> Element {
    let confirm_disable: Signal<Option<skene::id::NousId>> = use_signal(|| None);

    // WHY: Collect into owned data to avoid holding signal read across rsx boundaries.
    let agent_ids: Vec<_> = {
        let data = store.read();
        data.agent_toggles
            .iter()
            .map(|t| {
                (
                    t.id.clone(),
                    t.name.clone(),
                    t.enabled,
                    t.pending,
                    t.apply_state,
                    t.live_status.clone(),
                )
            })
            .collect()
    };

    let flag_data: Vec<_> = {
        let data = store.read();
        data.feature_flags
            .iter()
            .map(|f| {
                (
                    f.key.clone(),
                    f.description.clone(),
                    f.enabled,
                    f.pending,
                    f.error.clone(),
                )
            })
            .collect()
    };

    let restart_required: Vec<_> = {
        let data = store.read();
        data.restart_required.clone()
    };

    rsx! {
        div {
            style: "{PANEL_STYLE}",

            div { style: "{SECTION_TITLE}", "Controls" }

            div { style: "{SUBSECTION_TITLE}", "Config" }

            ConfigReloadRow { store, config }

            div { style: "{SUBSECTION_TITLE}", "Agents" }

            if agent_ids.is_empty() {
                EmptyState { title: "No agents available".to_string() }
            }

            for (id , name , enabled , pending , apply_state , live_status) in agent_ids {
                AgentToggleRow {
                    key: "{id}",
                    id: id.clone(),
                    name,
                    enabled,
                    pending,
                    apply_state,
                    live_status,
                    store,
                    config,
                    confirm_disable,
                }
            }

            div { style: "{SUBSECTION_TITLE}", "Feature Flags" }

            if flag_data.is_empty() {
                EmptyState { title: "No feature flags configured".to_string() }
            }

            if !restart_required.is_empty() {
                div {
                    style: "color: var(--status-warning); font-size: var(--text-xs); margin-bottom: var(--space-2);",
                    "Restart required for changes to take effect:"
                }
                for path in restart_required {
                div {
                    style: "color: var(--status-warning); font-size: var(--text-xs); margin-left: var(--space-2);",
                    "- {path}"
                }
            }
            }

            for (key , description , enabled , pending , error) in flag_data {
                FeatureFlagRow {
                    key: "{key}",
                    flag_key: key,
                    description,
                    enabled,
                    pending,
                    error,
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
    id: skene::id::NousId,
    name: String,
    enabled: bool,
    pending: bool,
    apply_state: ToggleApplyState,
    live_status: Option<String>,
    store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    mut confirm_disable: Signal<Option<skene::id::NousId>>,
) -> Element {
    let is_expanded = store
        .read()
        .expanded_agent
        .as_ref()
        .is_some_and(|e| *e == id);

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
                    t.apply_state,
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    let status_label = toggle_status_label(pending, apply_state, live_status.as_deref());
    let status_style = toggle_status_style(pending, apply_state);

    rsx! {
        div {
            style: "{ROW_STYLE}",
            div {
                style: "display: flex; align-items: center; gap: var(--space-2);",
                span { style: "{TOGGLE_LABEL}", "{name}" }
                if let Some(label) = status_label {
                    span { style: "{status_style}", "{label}" }
                }
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
            for (aid , tname , tool_enabled , tool_pending , tool_apply_state) in tools {
                ToolToggleRow {
                    key: "{aid}-{tname}",
                    agent_id: aid,
                    tool_name: tname,
                    enabled: tool_enabled,
                    pending: tool_pending,
                    apply_state: tool_apply_state,
                    store,
                    config,
                }
            }
        }
    }
}

#[component]
fn ToolToggleRow(
    agent_id: skene::id::NousId,
    tool_name: String,
    enabled: bool,
    pending: bool,
    apply_state: ToggleApplyState,
    store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
) -> Element {
    let status_label = toggle_status_label(pending, apply_state, None);
    let status_style = toggle_status_style(pending, apply_state);

    rsx! {
        div {
            style: "{TOOL_ROW_STYLE}",
            div {
                style: "display: flex; align-items: center; gap: var(--space-2);",
                span { style: "{TOOL_LABEL}", "{tool_name}" }
                if let Some(label) = status_label {
                    span { style: "{status_style}", "{label}" }
                }
            }
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

fn toggle_status_label(
    pending: bool,
    apply_state: ToggleApplyState,
    live_status: Option<&str>,
) -> Option<&'static str> {
    if pending {
        return Some("pending");
    }
    match apply_state {
        ToggleApplyState::Synced => None,
        ToggleApplyState::Pending => Some("pending live state"),
        ToggleApplyState::Degraded => Some("degraded"),
        ToggleApplyState::ReloadRequired => Some("reload required"),
        ToggleApplyState::RestartRequired if live_status == Some("degraded") => Some("degraded"),
        ToggleApplyState::RestartRequired => Some("restart required"),
        ToggleApplyState::Failed => Some("update failed"),
    }
}

fn toggle_status_style(pending: bool, apply_state: ToggleApplyState) -> &'static str {
    if pending {
        return STATUS_BADGE_WARNING;
    }
    match apply_state {
        ToggleApplyState::Degraded | ToggleApplyState::Failed => STATUS_BADGE_ERROR,
        ToggleApplyState::Synced
        | ToggleApplyState::Pending
        | ToggleApplyState::ReloadRequired
        | ToggleApplyState::RestartRequired => STATUS_BADGE_WARNING,
    }
}

const ERROR_STYLE: &str = "\
    color: var(--status-error); \
    font-size: var(--text-xs); \
    padding: var(--space-1) 0; \
    margin-top: calc(-1 * var(--space-1));\
";

#[component]
fn FeatureFlagRow(
    flag_key: String, // kanon:ignore RUST/plain-string-secret -- feature flag identifier, not credential material (#3988)
    description: String,
    enabled: bool,
    pending: bool,
    error: Option<String>,
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
            if let Some(ref err) = error {
                div { style: "{ERROR_STYLE}", "{err}" }
            }
        }
    }
}

/// Build the one-line summary shown beneath the reload button.
///
/// Returns `(text, is_error)` so the caller can pick the error vs. muted
/// style without re-matching on the outcome.
fn reload_summary(outcome: Option<&ReloadOutcome>) -> Option<(String, bool)> {
    match outcome {
        None => None,
        Some(ReloadOutcome::Applied {
            hot_reloaded,
            changed,
        }) => {
            if *changed == 0 {
                Some(("Config already up to date.".to_string(), false))
            } else {
                let text = format!(
                    "Reloaded {hot_reloaded} of {changed} changed value(s) without a restart."
                );
                Some((text, false))
            }
        }
        Some(ReloadOutcome::Failed(message)) => Some((message.clone(), true)),
    }
}

// WHY(#5799): backend exposes POST /api/v1/config/reload (re-read
// aletheia.toml + env overrides, apply hot-reloadable values) with no UI
// caller. This row fills that gap, matching the fire_feature_toggle
// spawn/request/state-update shape: optimistic-free (nothing to flip), busy
// button while pending, and the same connection/parse/status error surface.
#[component]
fn ConfigReloadRow(store: Signal<ToggleStore>, config: Signal<ConnectionConfig>) -> Element {
    let (pending, outcome) = {
        let data = store.read();
        (data.reload_pending, data.reload_outcome.clone())
    };
    let summary = reload_summary(outcome.as_ref());

    rsx! {
        div {
            div {
                style: "{ROW_STYLE}",
                span { style: "{TOGGLE_LABEL}", "Reload config from disk" }
                if pending {
                    button { style: "{RELOAD_BTN_DISABLED}", disabled: true, "Reloading\u{2026}" }
                } else {
                    button {
                        style: "{RELOAD_BTN}",
                        onclick: move |_| fire_config_reload(store, config),
                        "Reload Config"
                    }
                }
            }
            if let Some((text, is_error)) = summary {
                div {
                    style: if is_error { "{ERROR_STYLE}" } else { "{FLAG_DESC}" },
                    "{text}"
                }
            }
        }
    }
}

#[component]
fn ConfirmDisableDialog(
    agent_id: skene::id::NousId,
    store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    mut confirm_disable: Signal<Option<skene::id::NousId>>,
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
                    style: "color: var(--text-primary); margin: 0 0 var(--space-4) 0;",
                    "Disable agent \"{name}\"?"
                }
                p {
                    style: "color: var(--text-secondary); font-size: var(--text-xs); margin: 0 0 var(--space-5) 0;",
                    "Active sessions will be interrupted."
                }
                div {
                    button {
                        style: "{CONFIRM_BTN} background: var(--status-error-bg); color: var(--status-error);",
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
                        style: "{CONFIRM_BTN} background: var(--border); color: var(--text-primary);",
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
        "width: 36px; height: 20px; border-radius: var(--radius-lg); background: var(--text-secondary); position: relative; cursor: wait; opacity: 0.6; flex-shrink: 0;"
    } else if enabled {
        "width: 36px; height: 20px; border-radius: var(--radius-lg); background: var(--status-success); position: relative; cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); flex-shrink: 0;"
    } else {
        "width: 36px; height: 20px; border-radius: var(--radius-lg); background: var(--text-muted); position: relative; cursor: pointer; transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick); flex-shrink: 0;"
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
fn request_confirm(mut sig: Signal<Option<skene::id::NousId>>, id: skene::id::NousId) {
    sig.set(Some(id));
}

fn default_true() -> bool {
    true
}

/// Server response shape for `PATCH /api/v1/nous/{id}`.
#[derive(Debug, Clone, serde::Deserialize)]
struct AgentToggleUpdateResponse {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default = "default_true")]
    config_applied: bool,
    #[serde(default = "default_true")]
    live_applied: bool,
    #[serde(default)]
    reload_required: bool,
    #[serde(default)]
    restart_required: bool,
}

impl AgentToggleUpdateResponse {
    fn action_result(&self) -> ToggleActionResult {
        ToggleActionResult {
            config_applied: self.config_applied,
            live_applied: self.live_applied,
            reload_required: self.reload_required,
            restart_required: self.restart_required,
        }
    }
}

/// Server response shape for `GET /api/v1/nous/{id}` after a toggle.
#[derive(Debug, Clone, Default, serde::Deserialize)]
struct AgentStatusRefreshResponse {
    #[serde(default)]
    status: Option<String>,
}

/// Tool entry returned by `PATCH /api/v1/nous/{id}/tools`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
struct ToolToggleUpdateEntry {
    #[serde(default)]
    name: String,
    #[serde(default)]
    enabled: bool,
}

/// Server response shape for `PATCH /api/v1/nous/{id}/tools`.
#[derive(Debug, Clone, serde::Deserialize)]
struct ToolToggleUpdateResponse {
    #[serde(default)]
    tools: Vec<ToolToggleUpdateEntry>,
    #[serde(default = "default_true")]
    config_applied: bool,
    #[serde(default = "default_true")]
    live_applied: bool,
    #[serde(default)]
    reload_required: bool,
    #[serde(default)]
    restart_required: bool,
}

impl ToolToggleUpdateResponse {
    fn action_result(&self) -> ToggleActionResult {
        ToggleActionResult {
            config_applied: self.config_applied,
            live_applied: self.live_applied,
            reload_required: self.reload_required,
            restart_required: self.restart_required,
        }
    }

    fn enabled_for(&self, tool_name: &str) -> Option<bool> {
        self.tools
            .iter()
            .find(|tool| tool.name == tool_name)
            .map(|tool| tool.enabled)
    }
}

fn fire_agent_toggle(
    mut store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    id: skene::id::NousId,
) {
    let prev = store.write().flip_agent(&id);
    let Some(prev_val) = prev else { return };

    let cfg = config.read().clone();
    let agent_id = id.clone();

    spawn(async move {
        let client = match authenticated_client(&cfg) {
            Ok(client) => client,
            Err(err) => {
                tracing::warn!("agent toggle client error: {err}");
                store.write().resolve_agent(&id, false, prev_val);
                return;
            }
        };
        let new_enabled = !prev_val;
        let url = agent_url(&cfg.server_url, agent_id.as_ref());

        let result = client
            .patch(&url)
            .json(&serde_json::json!({ "enabled": new_enabled }))
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<AgentToggleUpdateResponse>().await {
                    Ok(mut body) => {
                        let status_url = agent_url(&cfg.server_url, agent_id.as_ref());
                        match client.get(&status_url).send().await {
                            Ok(status_resp) if status_resp.status().is_success() => {
                                if let Ok(status_body) =
                                    status_resp.json::<AgentStatusRefreshResponse>().await
                                {
                                    body.status = status_body.status.or(body.status);
                                }
                            }
                            Ok(_) | Err(_) => {}
                        }
                        let action_result = body.action_result();
                        store.write().resolve_agent_result(
                            &id,
                            prev_val,
                            body.enabled,
                            body.status,
                            action_result,
                        );
                    }
                    Err(_) => {
                        store.write().resolve_agent(&id, false, prev_val);
                    }
                }
            }
            Ok(_) | Err(_) => {
                store.write().resolve_agent(&id, false, prev_val);
            }
        }
    });
}

fn fire_tool_toggle(
    mut store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    agent_id: skene::id::NousId,
    tool_name: String,
) {
    let prev = store.write().flip_tool(&agent_id, &tool_name);
    let Some(prev_val) = prev else { return };

    let cfg = config.read().clone();
    let aid = agent_id.clone();
    let tname = tool_name.clone();

    spawn(async move {
        let client = match authenticated_client(&cfg) {
            Ok(client) => client,
            Err(err) => {
                tracing::warn!("tool toggle client error: {err}");
                store
                    .write()
                    .resolve_tool(&agent_id, &tool_name, false, prev_val);
                return;
            }
        };
        let new_enabled = !prev_val;
        let url = agent_tools_url(&cfg.server_url, aid.as_ref());

        let result = client
            .patch(&url)
            .json(&serde_json::json!({ "tool": tname, "enabled": new_enabled }))
            .send()
            .await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<ToolToggleUpdateResponse>().await {
                    Ok(body) => {
                        store.write().resolve_tool_result(
                            &agent_id,
                            &tool_name,
                            prev_val,
                            body.enabled_for(&tool_name),
                            body.action_result(),
                        );
                    }
                    Err(_) => {
                        store
                            .write()
                            .resolve_tool(&agent_id, &tool_name, false, prev_val);
                    }
                }
            }
            Ok(_) | Err(_) => {
                store
                    .write()
                    .resolve_tool(&agent_id, &tool_name, false, prev_val);
            }
        }
    });
}

/// Server response shape for `PUT /api/v1/config/feature_flags`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
struct ConfigFeatureFlagsUpdateResponse {
    #[serde(default)]
    restart_required: Vec<String>,
}

fn fire_feature_toggle(
    mut store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    key: String, // kanon:ignore RUST/plain-string-secret -- feature flag identifier, not credential material (#3988)
) {
    let prev = store.write().flip_feature(&key);
    let Some(prev_val) = prev else { return };

    let cfg = config.read().clone();
    let flag_key = key.clone();

    spawn(async move {
        let client = match authenticated_client(&cfg) {
            Ok(client) => client,
            Err(err) => {
                store.write().resolve_feature(
                    &flag_key,
                    false,
                    prev_val,
                    Some(err.to_string()),
                    Vec::new(),
                );
                return;
            }
        };
        let url = feature_flags_url(&cfg.server_url);

        // WHY: Send the complete feature_flags section so the server replaces
        // the array wholesale; a partial PATCH would silently drop sibling flags.
        let payload = {
            let data = store.read();
            data.feature_flags_payload()
        };

        let result = client.put(&url).json(&payload).send().await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<ConfigFeatureFlagsUpdateResponse>().await {
                    Ok(body) => {
                        store.write().resolve_feature(
                            &flag_key,
                            true,
                            prev_val,
                            None,
                            body.restart_required,
                        );
                    }
                    Err(err) => {
                        store.write().resolve_feature(
                            &flag_key,
                            false,
                            prev_val,
                            Some(format!("failed to parse config response: {err}")),
                            Vec::new(),
                        );
                    }
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let detail = resp.text().await.unwrap_or_default();
                let message = if detail.is_empty() {
                    format!("server returned {status}")
                } else {
                    format!("server returned {status}: {}", detail.trim())
                };
                store.write().resolve_feature(
                    &flag_key,
                    false,
                    prev_val,
                    Some(message),
                    Vec::new(),
                );
            }
            Err(e) => {
                store.write().resolve_feature(
                    &flag_key,
                    false,
                    prev_val,
                    Some(format!("connection error: {e}")),
                    Vec::new(),
                );
            }
        }
    });
}

/// Server response shape for `POST /api/v1/config/reload`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
struct ConfigReloadResponse {
    #[serde(default)]
    hot_reloaded: usize,
    #[serde(default)]
    restart_required: Vec<String>,
    #[serde(default)]
    changed: Vec<String>,
}

fn fire_config_reload(mut store: Signal<ToggleStore>, config: Signal<ConnectionConfig>) {
    store.write().begin_reload();

    let cfg = config.read().clone();

    spawn(async move {
        let client = match authenticated_client(&cfg) {
            Ok(client) => client,
            Err(err) => {
                store.write().resolve_reload_failure(err.to_string());
                return;
            }
        };
        let url = reload_url(&cfg.server_url);

        // NOTE: no body -- POST /api/v1/config/reload re-reads aletheia.toml
        // + env overrides from disk; there is nothing for the client to send.
        let result = client.post(&url).send().await;

        match result {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<ConfigReloadResponse>().await {
                    Ok(body) => {
                        store.write().resolve_reload_success(
                            body.hot_reloaded,
                            body.changed.len(),
                            body.restart_required,
                        );
                    }
                    Err(err) => {
                        store.write().resolve_reload_failure(format!(
                            "failed to parse reload response: {err}"
                        ));
                    }
                }
            }
            Ok(resp) => {
                let status = resp.status();
                let detail = resp.text().await.unwrap_or_default();
                let message = if detail.is_empty() {
                    format!("server returned {status}")
                } else {
                    format!("server returned {status}: {}", detail.trim())
                };
                store.write().resolve_reload_failure(message);
            }
            Err(e) => {
                store
                    .write()
                    .resolve_reload_failure(format!("connection error: {e}"));
            }
        }
    });
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::reload_summary;
    use crate::state::ops::{FeatureFlag, ReloadOutcome, ToggleStore};

    use skene::api::routes::config::{feature_flags_url, reload_url};

    #[test]
    fn feature_flags_url_uses_put_section_endpoint() {
        assert_eq!(
            feature_flags_url("https://example.com"),
            "https://example.com/api/v1/config/feature_flags"
        );
    }

    #[test]
    fn feature_flags_url_trims_trailing_slash() {
        assert_eq!(
            feature_flags_url("http://localhost:8080/"),
            "http://localhost:8080/api/v1/config/feature_flags"
        );
    }

    #[test]
    fn reload_url_uses_reload_endpoint() {
        assert_eq!(
            reload_url("https://example.com"),
            "https://example.com/api/v1/config/reload"
        );
    }

    #[test]
    fn reload_url_trims_trailing_slash() {
        assert_eq!(
            reload_url("http://localhost:8080/"),
            "http://localhost:8080/api/v1/config/reload"
        );
    }

    #[test]
    fn reload_summary_none_when_no_outcome_yet() {
        assert_eq!(reload_summary(None), None);
    }

    #[test]
    fn reload_summary_reports_up_to_date_when_nothing_changed() {
        let outcome = ReloadOutcome::Applied {
            hot_reloaded: 0,
            changed: 0,
        };
        assert_eq!(
            reload_summary(Some(&outcome)),
            Some(("Config already up to date.".to_string(), false))
        );
    }

    #[test]
    fn reload_summary_reports_counts_when_changed() {
        let outcome = ReloadOutcome::Applied {
            hot_reloaded: 2,
            changed: 5,
        };
        let (text, is_error) = reload_summary(Some(&outcome)).unwrap();
        assert_eq!(
            text,
            "Reloaded 2 of 5 changed value(s) without a restart."
        );
        assert!(!is_error);
    }

    #[test]
    fn reload_summary_surfaces_failure_as_error() {
        let outcome = ReloadOutcome::Failed("connection error: timed out".to_string());
        assert_eq!(
            reload_summary(Some(&outcome)),
            Some(("connection error: timed out".to_string(), true))
        );
    }

    #[test]
    fn feature_flags_payload_matches_put_contract() {
        let mut store = ToggleStore::new();
        store.feature_flags.push(FeatureFlag {
            key: "dark_mode".to_string(),
            description: "Enable dark mode".to_string(),
            enabled: true,
            pending: false,
            error: None,
        });
        store.feature_flags.push(FeatureFlag {
            key: "beta_tools".to_string(),
            description: "Beta tool access".to_string(),
            enabled: false,
            pending: false,
            error: None,
        });

        let json = serde_json::to_value(store.feature_flags_payload()).unwrap();
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 2);

        let first = arr[0].as_object().unwrap();
        assert_eq!(first["key"], "dark_mode");
        assert_eq!(first["description"], "Enable dark mode");
        assert_eq!(first["enabled"], true);

        let second = arr[1].as_object().unwrap();
        assert_eq!(second["key"], "beta_tools");
        assert_eq!(second["enabled"], false);
    }

    #[test]
    fn feature_flags_payload_preserves_state_after_flip() {
        let mut store = ToggleStore::new();
        store.feature_flags.push(FeatureFlag {
            key: "flag_a".to_string(),
            description: String::new(),
            enabled: false,
            pending: false,
            error: None,
        });
        store.feature_flags.push(FeatureFlag {
            key: "flag_b".to_string(),
            description: String::new(),
            enabled: true,
            pending: false,
            error: None,
        });

        store.flip_feature("flag_a");
        let payload = store.feature_flags_payload();
        assert!(payload.iter().any(|f| f.key == "flag_a" && f.enabled));
        assert!(payload.iter().any(|f| f.key == "flag_b" && f.enabled));
    }
}
