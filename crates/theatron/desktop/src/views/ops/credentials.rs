//! Credential management panel: display, validate, rotate, add, and remove credentials.
//!
//! TODO(#107): move CredentialApiEntry and related request types to theatron-core
//! when /api/system/credentials is implemented in pylon.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::credentials::{
    CredentialEntry, CredentialRole, CredentialStore, ValidationStatus, mask_key,
};
use crate::state::fetch::FetchState;

// --- API types (local until pylon implements the endpoint) ---

#[derive(Debug, Clone, serde::Deserialize)]
struct CredentialsListResponse {
    #[serde(default)]
    credentials: Vec<CredentialApiEntry>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct CredentialApiEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    provider: String,
    #[serde(default)]
    role: String,
    /// Key value from API. Must be masked before use if it is not already masked.
    #[serde(default)]
    masked_key: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    last_validated: Option<String>,
    #[serde(default)]
    requests_today: u64,
    #[serde(default)]
    tokens_today: u64,
}

impl CredentialApiEntry {
    fn into_entry(self) -> CredentialEntry {
        let role = if self.role == "primary" {
            CredentialRole::Primary
        } else {
            CredentialRole::Backup
        };
        let status = match self.status.as_str() {
            "valid" => ValidationStatus::Valid,
            "expired" => ValidationStatus::Expired,
            _ => ValidationStatus::Untested,
        };
        // SAFETY: mask any full key value before it enters reactive state.
        let masked = if self.masked_key.starts_with("...") {
            self.masked_key
        } else {
            mask_key(&self.masked_key)
        };
        CredentialEntry {
            id: self.id,
            provider: self.provider,
            role,
            masked_key: masked,
            status,
            last_validated: self.last_validated,
            requests_today: self.requests_today,
            tokens_today: self.tokens_today,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct AddCredentialRequest {
    provider: String,
    /// Raw key — cleared from reactive state immediately after spawn.
    key: String,
    role: String,
}

// --- Styles ---

const PANEL_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 12px; \
    flex: 1; \
    overflow-y: auto;\
";

const CRED_CARD_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px 20px;\
";

const CARD_HEADER: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: 10px;\
";

const PROVIDER_NAME: &str = "\
    font-size: 15px; \
    font-weight: bold; \
    color: #e0e0e0;\
";

const META_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: 14px; \
    margin-bottom: 6px; \
    font-size: 13px;\
";

const STATS_ROW: &str = "\
    display: flex; \
    gap: 16px; \
    font-size: 12px; \
    color: #666; \
    margin-bottom: 12px;\
";

const ACTIONS_ROW: &str = "\
    display: flex; \
    gap: 8px; \
    align-items: center; \
    flex-wrap: wrap;\
";

const BTN_STD: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

const BTN_DANGER: &str = "\
    background: #3a1a1a; \
    color: #ef4444; \
    border: 1px solid #5a2a2a; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

const BTN_CONFIRM: &str = "\
    background: #ef4444; \
    color: #fff; \
    border: none; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

const BTN_CANCEL: &str = "\
    background: #2a2a2a; \
    color: #aaa; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

const BTN_DISABLED: &str = "\
    background: #1a1a2e; \
    color: #555; \
    border: 1px solid #2a2a3a; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: not-allowed;\
";

const CONFIRM_BANNER: &str = "\
    display: flex; \
    gap: 8px; \
    align-items: center; \
    padding: 8px 0; \
    border-top: 1px solid #333; \
    margin-top: 10px;\
";

const WARN_TEXT: &str = "\
    font-size: 12px; \
    color: #f59e0b; \
    flex: 1;\
";

const ADD_CARD_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #2a2a4a; \
    border-radius: 8px; \
    padding: 16px 20px;\
";

const FORM_TITLE: &str = "\
    font-size: 14px; \
    font-weight: bold; \
    color: #aaa; \
    margin-bottom: 12px;\
";

const FORM_ROW: &str = "\
    display: flex; \
    gap: 10px; \
    align-items: flex-end; \
    flex-wrap: wrap; \
    margin-bottom: 10px;\
";

const FORM_GROUP: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 4px;\
";

const FORM_LABEL: &str = "\
    font-size: 11px; \
    color: #888; \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const FORM_INPUT: &str = "\
    background: #0f0f1a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 6px 10px; \
    font-size: 13px; \
    width: 160px;\
";

const FORM_SELECT: &str = "\
    background: #0f0f1a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 6px 10px; \
    font-size: 13px;\
";

const ERROR_TEXT: &str = "\
    font-size: 12px; \
    color: #ef4444; \
    margin-top: 4px;\
";

// --- Components ---

/// Credential management panel.
#[component]
pub(crate) fn CredentialsView() -> Element {
    let mut fetch_trigger = use_signal(|| 0u32);
    let mut fetch_state: Signal<FetchState<CredentialStore>> = use_signal(|| FetchState::Loading);
    let config: Signal<ConnectionConfig> = use_context();

    let mut show_add = use_signal(|| false);
    let mut add_provider = use_signal(String::new);
    let mut add_key = use_signal(String::new);
    let mut add_role: Signal<CredentialRole> = use_signal(|| CredentialRole::Primary);
    let mut add_error: Signal<Option<String>> = use_signal(|| None);

    use_effect(move || {
        let _trigger = *fetch_trigger.read();
        let cfg = config.read().clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/system/credentials",
                cfg.server_url.trim_end_matches('/')
            );
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<CredentialsListResponse>().await {
                        Ok(data) => {
                            let entries = data
                                .credentials
                                .into_iter()
                                .map(CredentialApiEntry::into_entry)
                                .collect();
                            fetch_state.set(FetchState::Loaded(CredentialStore { entries }));
                        }
                        Err(e) => {
                            fetch_state
                                .set(FetchState::Error(format!("parse error: {e}")));
                        }
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    fetch_state
                        .set(FetchState::Error(format!("server returned {status}")));
                }
                Err(e) => {
                    fetch_state
                        .set(FetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    });

    let mut do_add = move || {
        let provider = add_provider.read().trim().to_string();
        let key = add_key.read().trim().to_string();
        let role = *add_role.read();

        if provider.is_empty() {
            add_error.set(Some("Provider is required.".to_string()));
            return;
        }
        if key.is_empty() {
            add_error.set(Some("Key is required.".to_string()));
            return;
        }
        add_error.set(None);

        let role_str = match role {
            CredentialRole::Primary => "primary".to_string(),
            CredentialRole::Backup => "backup".to_string(),
        };
        let payload = AddCredentialRequest {
            provider,
            key,
            role: role_str,
        };
        let cfg = config.read().clone();

        // WHY: Clear key immediately before spawning so the raw value does not
        // linger in reactive state after the async task begins.
        add_key.set(String::new());

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/system/credentials",
                cfg.server_url.trim_end_matches('/')
            );
            match client.post(&url).json(&payload).send().await {
                Ok(resp) if resp.status().is_success() => {
                    add_provider.set(String::new());
                    show_add.set(false);
                    fetch_trigger.set(fetch_trigger() + 1);
                }
                Ok(resp) => {
                    let status = resp.status();
                    add_error.set(Some(format!("Add failed: {status}")));
                }
                Err(e) => {
                    add_error.set(Some(format!("Connection error: {e}")));
                }
            }
        });
    };

    // Collect card data from the loaded state (owned values for the RSX loop).
    let (cards, fetch_loading, fetch_error_msg) = {
        let state = fetch_state.read();
        match &*state {
            FetchState::Loading => (Vec::new(), true, None),
            FetchState::Error(e) => (Vec::new(), false, Some(e.clone())),
            FetchState::Loaded(store) => {
                let cards: Vec<(CredentialEntry, bool, bool)> = store
                    .entries
                    .iter()
                    .map(|e| {
                        (
                            e.clone(),
                            store.can_rotate(&e.provider),
                            store.is_last_primary(&e.id),
                        )
                    })
                    .collect();
                (cards, false, None)
            }
        }
    };

    rsx! {
        div {
            style: "{PANEL_STYLE}",

            if fetch_loading {
                div { style: "color: #888; font-size: 13px;", "Loading credentials..." }
            }

            if let Some(err) = &fetch_error_msg {
                div { style: "color: #ef4444; font-size: 13px;", "Error: {err}" }
            }

            if !fetch_loading && fetch_error_msg.is_none() && cards.is_empty() {
                div { style: "color: #555; font-size: 13px;", "No credentials configured." }
            }

            for (entry, can_rot, is_last_prim) in cards {
                CredentialCard {
                    key: "{entry.id}",
                    entry,
                    can_rotate: can_rot,
                    is_last_primary: is_last_prim,
                    on_change: move |_| fetch_trigger.set(fetch_trigger() + 1),
                }
            }

            if *show_add.read() {
                div {
                    style: "{ADD_CARD_STYLE}",
                    div { style: "{FORM_TITLE}", "Add Credential" }
                    div {
                        style: "{FORM_ROW}",
                        div {
                            style: "{FORM_GROUP}",
                            span { style: "{FORM_LABEL}", "Provider" }
                            input {
                                style: "{FORM_INPUT}",
                                r#type: "text",
                                placeholder: "anthropic",
                                value: "{add_provider}",
                                oninput: move |evt: Event<FormData>| {
                                    add_provider.set(evt.value().clone());
                                    add_error.set(None);
                                },
                            }
                        }
                        div {
                            style: "{FORM_GROUP}",
                            span { style: "{FORM_LABEL}", "API Key" }
                            input {
                                style: "{FORM_INPUT}",
                                r#type: "password",
                                placeholder: "sk-...",
                                value: "{add_key}",
                                oninput: move |evt: Event<FormData>| {
                                    add_key.set(evt.value().clone());
                                    add_error.set(None);
                                },
                            }
                        }
                        div {
                            style: "{FORM_GROUP}",
                            span { style: "{FORM_LABEL}", "Role" }
                            select {
                                style: "{FORM_SELECT}",
                                onchange: move |evt: Event<FormData>| {
                                    let role = if evt.value() == "primary" {
                                        CredentialRole::Primary
                                    } else {
                                        CredentialRole::Backup
                                    };
                                    add_role.set(role);
                                },
                                option { value: "primary", selected: true, "Primary" }
                                option { value: "backup", "Backup" }
                            }
                        }
                    }
                    if let Some(err) = &*add_error.read() {
                        div { style: "{ERROR_TEXT}", "{err}" }
                    }
                    div {
                        style: "display: flex; gap: 8px; margin-top: 4px;",
                        button {
                            style: "{BTN_STD}",
                            onclick: move |_| do_add(),
                            "Add"
                        }
                        button {
                            style: "{BTN_CANCEL}",
                            onclick: move |_| {
                                show_add.set(false);
                                add_error.set(None);
                                // WHY: Clear key field on cancel to avoid stale
                                // credential values persisting in state.
                                add_key.set(String::new());
                            },
                            "Cancel"
                        }
                    }
                }
            } else {
                button {
                    style: "{BTN_STD}",
                    onclick: move |_| show_add.set(true),
                    "+ Add Credential"
                }
            }
        }
    }
}

/// A single credential card with validation, rotation, and removal actions.
#[component]
fn CredentialCard(
    entry: CredentialEntry,
    can_rotate: bool,
    is_last_primary: bool,
    on_change: EventHandler<()>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut is_validating = use_signal(|| false);
    let mut confirm_rotate = use_signal(|| false);
    let mut confirm_remove = use_signal(|| false);
    let mut card_error: Signal<Option<String>> = use_signal(|| None);

    let entry_id = entry.id.clone();
    let entry_provider = entry.provider.clone();

    let mut do_validate = {
        let id = entry_id.clone();
        move || {
            let cfg = config.read().clone();
            let id_v = id.clone();
            is_validating.set(true);
            card_error.set(None);

            spawn(async move {
                let client = authenticated_client(&cfg);
                let url = format!(
                    "{}/api/system/credentials/{}/validate",
                    cfg.server_url.trim_end_matches('/'),
                    id_v
                );
                match client.post(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        is_validating.set(false);
                        on_change.call(());
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        is_validating.set(false);
                        card_error.set(Some(format!("Validate failed: {status}")));
                    }
                    Err(e) => {
                        is_validating.set(false);
                        card_error.set(Some(format!("Connection error: {e}")));
                    }
                }
            });
        }
    };

    let mut do_rotate = {
        let provider = entry_provider.clone();
        move || {
            let cfg = config.read().clone();
            let prov = provider.clone();
            confirm_rotate.set(false);
            card_error.set(None);

            spawn(async move {
                let client = authenticated_client(&cfg);
                let encoded =
                    form_urlencoded::byte_serialize(prov.as_bytes()).collect::<String>();
                let url = format!(
                    "{}/api/system/credentials/rotate?provider={}",
                    cfg.server_url.trim_end_matches('/'),
                    encoded
                );
                match client.post(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        on_change.call(());
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        card_error.set(Some(format!("Rotate failed: {status}")));
                    }
                    Err(e) => {
                        card_error.set(Some(format!("Connection error: {e}")));
                    }
                }
            });
        }
    };

    let mut do_remove = {
        let id = entry_id.clone();
        move || {
            let cfg = config.read().clone();
            let id_r = id.clone();
            confirm_remove.set(false);
            card_error.set(None);

            spawn(async move {
                let client = authenticated_client(&cfg);
                let url = format!(
                    "{}/api/system/credentials/{}",
                    cfg.server_url.trim_end_matches('/'),
                    id_r
                );
                match client.delete(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        on_change.call(());
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        card_error.set(Some(format!("Remove failed: {status}")));
                    }
                    Err(e) => {
                        card_error.set(Some(format!("Connection error: {e}")));
                    }
                }
            });
        }
    };

    let validating = *is_validating.read();
    let show_rotate = *confirm_rotate.read();
    let show_remove = *confirm_remove.read();

    let role_bg = if entry.role == CredentialRole::Primary {
        "background: #1a2a4a; color: #4a9aff;"
    } else {
        "background: #2a2a2a; color: #888;"
    };

    rsx! {
        div {
            style: "{CRED_CARD_STYLE}",

            // Header: provider name + role badge
            div {
                style: "{CARD_HEADER}",
                span { style: "{PROVIDER_NAME}", "{entry.provider}" }
                span {
                    style: "font-size: 11px; padding: 2px 8px; border-radius: 4px; \
                            font-weight: bold; text-transform: uppercase; letter-spacing: 0.5px; \
                            {role_bg}",
                    "{entry.role.label()}"
                }
            }

            // Masked key + validation status
            div {
                style: "{META_ROW}",
                span {
                    style: "font-family: monospace; color: #888; font-size: 13px;",
                    "{entry.masked_key}"
                }
                span {
                    style: "display: inline-flex; align-items: center; gap: 5px; font-size: 13px; \
                            color: {entry.status.color()};",
                    span {
                        style: "width: 8px; height: 8px; border-radius: 50%; \
                                background: {entry.status.color()}; display: inline-block;",
                    }
                    "{entry.status.label()}"
                }
            }

            // Last validated + usage stats
            div {
                style: "{STATS_ROW}",
                if let Some(ref ts) = entry.last_validated {
                    span { "Validated: {ts}" }
                } else {
                    span { "Never validated" }
                }
                span { "{entry.requests_today} req today" }
                span { "{entry.tokens_today} tok today" }
            }

            // Action buttons
            div {
                style: "{ACTIONS_ROW}",
                if validating {
                    button { style: "{BTN_DISABLED}", disabled: true, "Validating..." }
                } else {
                    button {
                        style: "{BTN_STD}",
                        onclick: move |_| do_validate(),
                        "Validate"
                    }
                }

                if can_rotate {
                    button {
                        style: "{BTN_STD}",
                        onclick: move |_| {
                            confirm_rotate.set(true);
                            confirm_remove.set(false);
                        },
                        "Rotate"
                    }
                }

                if is_last_primary {
                    button {
                        style: "{BTN_DISABLED}",
                        disabled: true,
                        title: "Cannot remove the last primary credential",
                        "Remove"
                    }
                } else {
                    button {
                        style: "{BTN_DANGER}",
                        onclick: move |_| {
                            confirm_remove.set(true);
                            confirm_rotate.set(false);
                        },
                        "Remove"
                    }
                }
            }

            // Rotate confirmation
            if show_rotate {
                div {
                    style: "{CONFIRM_BANNER}",
                    span {
                        style: "{WARN_TEXT}",
                        "Swap primary and backup for {entry_provider}? \
                        If backup is untested or expired, API calls may fail."
                    }
                    button {
                        style: "{BTN_CONFIRM}",
                        onclick: move |_| do_rotate(),
                        "Confirm"
                    }
                    button {
                        style: "{BTN_CANCEL}",
                        onclick: move |_| confirm_rotate.set(false),
                        "Cancel"
                    }
                }
            }

            // Remove confirmation
            if show_remove {
                div {
                    style: "{CONFIRM_BANNER}",
                    span { style: "{WARN_TEXT}", "Permanently remove this credential?" }
                    button {
                        style: "{BTN_CONFIRM}",
                        onclick: move |_| do_remove(),
                        "Remove"
                    }
                    button {
                        style: "{BTN_CANCEL}",
                        onclick: move |_| confirm_remove.set(false),
                        "Cancel"
                    }
                }
            }

            // Per-card error
            if let Some(err) = &*card_error.read() {
                div { style: "color: #ef4444; font-size: 12px; margin-top: 8px;", "{err}" }
            }
        }
    }
}
