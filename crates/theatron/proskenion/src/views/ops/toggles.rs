//! Toggle controls panel: agent enable/disable, tool toggles, feature flags.

use dioxus::prelude::*;
use skeue::EmptyState;

use crate::state::connection::ConnectionConfig;
use crate::state::ops::{
    FeatureFlagConfigEntry, RecoverOutcome, ReloadOutcome, ToggleActionResult, ToggleApplyState,
    ToggleStore, ToolToggle,
};

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
    let confirm_disable: Signal<Option<skene::id::ApiNousId>> = use_signal(|| None);

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
                    t.error.clone(),
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

    // WHY(#4986): the backend feature-flag write is whole-section, so a
    // second flag flipped while one write is outstanding could race and
    // stomp the first write's persisted value -- disable every row's
    // control while any one flag is pending, not just its own.
    let any_feature_pending = flag_data.iter().any(|(_, _, _, pending, _)| *pending);

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

            for (id , name , enabled , pending , apply_state , live_status , error) in agent_ids {
                AgentToggleRow {
                    key: "{id}",
                    id: id.clone(),
                    name,
                    enabled,
                    pending,
                    apply_state,
                    live_status,
                    error,
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
                    pending: pending || any_feature_pending,
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
    id: skene::id::ApiNousId,
    name: String,
    enabled: bool,
    pending: bool,
    apply_state: ToggleApplyState,
    live_status: Option<String>,
    error: Option<String>,
    store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    mut confirm_disable: Signal<Option<skene::id::ApiNousId>>,
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
    let tools: Vec<ToolToggle> = if is_expanded {
        store
            .read()
            .tools_for_agent(&id)
            .into_iter()
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    let status_label = toggle_status_label(pending, apply_state, live_status.as_deref());
    let status_style = toggle_status_style(pending, apply_state);

    // WHY(#5800): the recover action is offered only while the server reports
    // this agent degraded -- "degraded" is the same live_status sentinel
    // toggle_status_label and ToggleStore::set_agent_live_status key on. On any
    // other lifecycle the endpoint has nothing to reset, so advertising it
    // would be a control that cannot do anything.
    let is_degraded = live_status.as_deref() == Some("degraded");
    let (recovering, recover_result) = {
        let data = store.read();
        (
            data.is_recovering(&id),
            recover_summary(data.recover_outcome_for(&id)),
        )
    };

    rsx! {
        div {
            style: "{ROW_STYLE}",
            div {
                style: "display: flex; align-items: center; gap: var(--space-2);",
                span { style: "{TOGGLE_LABEL}", "{name}" }
                if let Some(label) = status_label {
                    span { style: "{status_style}", "{label}" }
                }
                if is_degraded {
                    if recovering {
                        button { style: "{RELOAD_BTN_DISABLED}", disabled: true, "Recovering\u{2026}" }
                    } else {
                        button {
                            style: "{RELOAD_BTN}",
                            "aria-label": "Recover {name}",
                            onclick: {
                                let id = id.clone();
                                move |_| fire_agent_recover(store, config, id.clone())
                            },
                            "Recover"
                        }
                    }
                }
                button {
                    style: "{EXPAND_BTN}",
                    "aria-expanded": if is_expanded { "true" } else { "false" },
                    "aria-label": "{name} tools",
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
                &format!("{name} enabled"),
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

        if let Some((text, is_error)) = recover_result {
            div {
                style: if is_error { "{ERROR_STYLE}" } else { "{FLAG_DESC}" },
                "{text}"
            }
        }

        if let Some(ref err) = error {
            div { style: "{ERROR_STYLE}", "{err}" }
        }

        if is_expanded {
            for tool in tools {
                ToolToggleRow {
                    key: "{tool.agent_id}-{tool.tool_name}",
                    tool,
                    store,
                    config,
                }
            }
        }
    }
}

#[component]
fn ToolToggleRow(
    tool: ToolToggle,
    store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
) -> Element {
    let status_label = toggle_status_label(tool.pending, tool.apply_state, None);
    let status_style = toggle_status_style(tool.pending, tool.apply_state);
    // WHY(#4772): a denied/inactive tool has no live toggle to flip -- show
    // WHY instead of a switch nobody can meaningfully act on.
    let is_actionable = tool.policy_state != "denied" && tool.policy_state != "inactive";
    let detail = {
        let mut line = format!(
            "{} \u{b7} {} \u{b7} {}",
            tool.source_plane, tool.reversibility, tool.approval
        );
        if !tool.groups.is_empty() {
            line.push_str(" \u{b7} ");
            line.push_str(&tool.groups.join(", "));
        }
        if tool.destructive {
            line.push_str(" \u{b7} destructive");
        }
        line
    };

    rsx! {
        div {
            div {
                style: "{TOOL_ROW_STYLE}",
                div {
                    style: "display: flex; align-items: center; gap: var(--space-2);",
                    span { style: "{TOOL_LABEL}", "{tool.tool_name}" }
                    if let Some(label) = status_label {
                        span { style: "{status_style}", "{label}" }
                    }
                }
                if is_actionable {
                    {toggle_switch(
                        &format!("{} enabled", tool.tool_name),
                        tool.enabled,
                        tool.pending,
                        {
                            let aid = tool.agent_id.clone();
                            let tname = tool.tool_name.clone();
                            move |_: Event<MouseData>| {
                                fire_tool_toggle(store, config, aid.clone(), tname.clone());
                            }
                        },
                    )}
                } else {
                    span { style: "{STATUS_BADGE_ERROR}", "{tool.policy_state}" }
                }
            }
            div { style: "{FLAG_DESC}", "{detail}" }
            if let Some(ref reason) = tool.unavailable_reason {
                div { style: "{ERROR_STYLE}", "{reason}" }
            }
        }
        if let Some(ref err) = tool.error {
            div { style: "{ERROR_STYLE}", "{err}" }
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
                    &format!("{flag_key} enabled"),
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
                        "aria-label": "Reload config from disk",
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
    agent_id: skene::id::ApiNousId,
    store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    mut confirm_disable: Signal<Option<skene::id::ApiNousId>>,
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
            "aria-label": "Disable agent confirmation backdrop",
            onclick: move |_| confirm_disable.set(None),
            div {
                style: "{CONFIRM_BOX}",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Disable agent \"{name}\"?",
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
    label: &str,
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
            role: "switch",
            "aria-label": "{label}",
            "aria-checked": if enabled { "true" } else { "false" },
            "aria-busy": if pending { "true" } else { "false" },
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
fn request_confirm(mut sig: Signal<Option<skene::id::ApiNousId>>, id: skene::id::ApiNousId) {
    sig.set(Some(id));
}

// WHY(#4565): `PATCH /api/v1/nous/{id}` and `PATCH /api/v1/nous/{id}/tools`
// now go through skene's typed `ApiClient` (`update_agent_enabled`,
// `update_agent_tool`) instead of a hand-built request on the raw client.
// skene's `NousSummary`/`NousToolsResponse` carry the `*_applied`/
// `*_required` fields as `Option<bool>` (a server old enough to omit them
// deserializes to `None`); the `unwrap_or` defaults here reproduce the
// exact `#[serde(default = "default_true")]`/`#[serde(default)]` behavior
// the pre-migration local response types applied to the same fields.
fn agent_toggle_action_result(body: &skene::api::types::NousSummary) -> ToggleActionResult {
    ToggleActionResult {
        config_applied: body.config_applied.unwrap_or(true),
        live_applied: body.live_applied.unwrap_or(true),
        reload_required: body.reload_required.unwrap_or(false),
        restart_required: body.restart_required.unwrap_or(false),
    }
}

fn tool_toggle_action_result(body: &skene::api::types::NousToolsResponse) -> ToggleActionResult {
    ToggleActionResult {
        config_applied: body.config_applied.unwrap_or(true),
        live_applied: body.live_applied.unwrap_or(true),
        reload_required: body.reload_required.unwrap_or(false),
        restart_required: body.restart_required.unwrap_or(false),
    }
}

fn tool_enabled_for(body: &skene::api::types::NousToolsResponse, tool_name: &str) -> Option<bool> {
    body.tools
        .iter()
        .find(|tool| tool.name == tool_name)
        .map(|tool| tool.enabled)
}

fn fire_agent_toggle(
    mut store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    id: skene::id::ApiNousId,
) {
    let prev = store.write().flip_agent(&id);
    let Some(prev_val) = prev else { return };

    let cfg = config.read().clone();
    let agent_id = id.clone();

    spawn(async move {
        let client =
            match skene::api::client::ApiClient::new(&cfg.server_url, cfg.auth_token.clone()) {
                Ok(client) => client,
                Err(err) => {
                    store
                        .write()
                        .resolve_agent(&id, false, prev_val, Some(err.to_string()));
                    return;
                }
            };
        let new_enabled = !prev_val;

        // WHY(#4565): `update_agent_enabled`'s response already carries a
        // freshly computed live `status` (pylon's handler always fills it
        // in, never leaves it absent), so the second best-effort
        // `agent_status` fetch the pre-migration raw-client version needed
        // to backfill a sometimes-missing status is no longer necessary.
        match client
            .update_agent_enabled(agent_id.as_ref(), new_enabled)
            .await
        {
            Ok(body) => {
                let action_result = agent_toggle_action_result(&body);
                store.write().resolve_agent_result(
                    &id,
                    prev_val,
                    Some(body.enabled),
                    Some(body.status),
                    action_result,
                    None,
                );
            }
            Err(err) => {
                store
                    .write()
                    .resolve_agent(&id, false, prev_val, Some(err.to_string()));
            }
        }
    });
}

fn fire_tool_toggle(
    mut store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    agent_id: skene::id::ApiNousId,
    tool_name: String,
) {
    let prev = store.write().flip_tool(&agent_id, &tool_name);
    let Some(prev_val) = prev else { return };

    let cfg = config.read().clone();
    let aid = agent_id.clone();
    let tname = tool_name.clone();

    spawn(async move {
        let client =
            match skene::api::client::ApiClient::new(&cfg.server_url, cfg.auth_token.clone()) {
                Ok(client) => client,
                Err(err) => {
                    store.write().resolve_tool(
                        &agent_id,
                        &tool_name,
                        false,
                        prev_val,
                        Some(err.to_string()),
                    );
                    return;
                }
            };
        let new_enabled = !prev_val;

        match client
            .update_agent_tool(aid.as_ref(), &tname, new_enabled)
            .await
        {
            Ok(body) => {
                store.write().resolve_tool_result(
                    &agent_id,
                    &tool_name,
                    prev_val,
                    tool_enabled_for(&body, &tool_name),
                    tool_toggle_action_result(&body),
                    None,
                );
            }
            Err(err) => {
                store.write().resolve_tool(
                    &agent_id,
                    &tool_name,
                    false,
                    prev_val,
                    Some(err.to_string()),
                );
            }
        }
    });
}

fn fire_feature_toggle(
    mut store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    key: String, // kanon:ignore RUST/plain-string-secret -- feature flag identifier, not credential material (#3988)
) {
    // WHY(#4986): `flip_feature` itself refuses when another feature-flag
    // write is already in flight (whole-section writes can otherwise race),
    // so this also doubles as that guard.
    let prev = store.write().flip_feature(&key);
    let Some(prev_val) = prev else { return };

    let cfg = config.read().clone();
    let flag_key = key.clone();

    spawn(async move {
        let client =
            match skene::api::client::ApiClient::new(&cfg.server_url, cfg.auth_token.clone()) {
                Ok(client) => client,
                Err(err) => {
                    store.write().resolve_feature(
                        &flag_key,
                        false,
                        prev_val,
                        Some(err.to_string()),
                        Vec::new(),
                        None,
                    );
                    return;
                }
            };

        // WHY: Send the complete feature_flags section so the server replaces
        // the array wholesale; a partial PATCH would silently drop sibling flags.
        let payload = {
            let data = store.read();
            data.feature_flags_payload()
        };
        let payload_value = match serde_json::to_value(&payload) {
            Ok(value) => value,
            Err(err) => {
                store.write().resolve_feature(
                    &flag_key,
                    false,
                    prev_val,
                    Some(format!("failed to encode feature flags: {err}")),
                    Vec::new(),
                    None,
                );
                return;
            }
        };

        match client.update_feature_flags(&payload_value).await {
            Ok(body) => {
                // WHY(#4986): `config` carries the canonical, server-normalized
                // section -- read it back into the store on every success
                // instead of trusting the client's optimistic guess.
                match serde_json::from_value::<Vec<FeatureFlagConfigEntry>>(body.config) {
                    Ok(entries) => {
                        store.write().resolve_feature(
                            &flag_key,
                            true,
                            prev_val,
                            None,
                            body.restart_required,
                            Some(entries),
                        );
                    }
                    Err(err) => {
                        store.write().resolve_feature(
                            &flag_key,
                            false,
                            prev_val,
                            Some(format!("failed to parse config response: {err}")),
                            Vec::new(),
                            None,
                        );
                    }
                }
            }
            Err(err) => {
                store.write().resolve_feature(
                    &flag_key,
                    false,
                    prev_val,
                    Some(err.to_string()),
                    Vec::new(),
                    None,
                );
            }
        }
    });
}

fn fire_config_reload(mut store: Signal<ToggleStore>, config: Signal<ConnectionConfig>) {
    store.write().begin_reload();

    let cfg = config.read().clone();

    spawn(async move {
        let client =
            match skene::api::client::ApiClient::new(&cfg.server_url, cfg.auth_token.clone()) {
                Ok(client) => client,
                Err(err) => {
                    store.write().resolve_reload_failure(err.to_string());
                    return;
                }
            };

        // NOTE: no body -- POST /api/v1/config/reload re-reads aletheia.toml
        // + env overrides from disk; there is nothing for the client to send.
        match client.reload_config().await {
            Ok(body) => {
                store.write().resolve_reload_success(
                    body.hot_reloaded,
                    body.changed.len(),
                    body.restart_required,
                );
            }
            Err(err) => {
                store.write().resolve_reload_failure(err.to_string());
            }
        }
    });
}

/// Summarize a recovery outcome as `(text, is_error)`.
///
/// WHY `recovered: false` is not an error: the server accepted and ran the
/// request, it simply reports the actor did not leave the degraded state.
/// Rendering that as a failed request would misattribute the cause.
fn recover_summary(outcome: Option<&RecoverOutcome>) -> Option<(String, bool)> {
    match outcome {
        None => None,
        Some(RecoverOutcome::Applied { recovered: true }) => {
            Some(("Agent reset to idle.".to_string(), false))
        }
        Some(RecoverOutcome::Applied { recovered: false }) => Some((
            "Server reported the agent did not leave the degraded state.".to_string(),
            true,
        )),
        Some(RecoverOutcome::Failed(message)) => Some((message.clone(), true)),
    }
}

// WHY(#5800): backend exposes POST /api/v1/nous/{id}/recover (reset a
// degraded actor to idle) with no UI caller, so an operator watching an
// agent sit in "degraded" had no action to take. This mirrors the
// fire_config_reload spawn/request/state-update shape: no optimistic flip,
// busy button while pending, same connection/parse/status error surface.
fn fire_agent_recover(
    mut store: Signal<ToggleStore>,
    config: Signal<ConnectionConfig>,
    id: skene::id::ApiNousId,
) {
    store.write().begin_recover(&id);

    let cfg = config.read().clone();

    spawn(async move {
        let client =
            match skene::api::client::ApiClient::new(&cfg.server_url, cfg.auth_token.clone()) {
                Ok(client) => client,
                Err(err) => {
                    store.write().resolve_recover_failure(&id, err.to_string());
                    return;
                }
            };

        // NOTE: no body -- the agent is identified by the path segment and
        // recovery takes no parameters.
        match client.agent_recover(id.as_str()).await {
            Ok(body) => {
                store.write().resolve_recover_success(&id, body.recovered);
            }
            Err(err) => {
                store.write().resolve_recover_failure(&id, err.to_string());
            }
        }
    });
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use skene::id::ApiNousId;

    use super::{recover_summary, reload_summary};
    use crate::state::ops::{FeatureFlag, RecoverOutcome, ReloadOutcome, ToggleStore};

    #[test]
    fn recover_summary_none_when_no_outcome_yet() {
        assert_eq!(recover_summary(None), None);
    }

    #[test]
    fn recover_summary_reports_reset_when_recovered() {
        let summary = recover_summary(Some(&RecoverOutcome::Applied { recovered: true })).unwrap();
        assert_eq!(summary.0, "Agent reset to idle.");
        assert!(!summary.1, "a successful reset is not an error");
    }

    #[test]
    fn recover_summary_flags_a_server_reported_non_recovery() {
        let summary = recover_summary(Some(&RecoverOutcome::Applied { recovered: false })).unwrap();
        assert!(
            summary.1,
            "the actor staying degraded must surface as an error"
        );
    }

    #[test]
    fn recover_summary_surfaces_the_failure_message() {
        let summary =
            recover_summary(Some(&RecoverOutcome::Failed("server returned 404".into()))).unwrap();
        assert_eq!(summary.0, "server returned 404");
        assert!(summary.1);
    }

    #[test]
    fn recover_state_is_scoped_to_the_agent_it_targets() {
        let mut store = ToggleStore::new();
        let alpha: ApiNousId = "alpha".into();
        let beta: ApiNousId = "beta".into();

        store.begin_recover(&alpha);
        assert!(store.is_recovering(&alpha));
        assert!(
            !store.is_recovering(&beta),
            "an in-flight recovery must not mark a different agent busy"
        );

        store.resolve_recover_success(&alpha, true);
        assert!(!store.is_recovering(&alpha));
        assert!(store.recover_outcome_for(&alpha).is_some());
        assert!(
            store.recover_outcome_for(&beta).is_none(),
            "an outcome must not render against a different agent"
        );
    }

    #[test]
    fn beginning_a_recovery_clears_the_previous_outcome() {
        let mut store = ToggleStore::new();
        let alpha: ApiNousId = "alpha".into();

        store.resolve_recover_failure(&alpha, "connection error".into());
        assert!(store.recover_outcome_for(&alpha).is_some());

        store.begin_recover(&alpha);
        assert!(
            store.recover_outcome_for(&alpha).is_none(),
            "a stale outcome beside a spinner reads as this attempt's result"
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
        assert_eq!(text, "Reloaded 2 of 5 changed value(s) without a restart.");
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
