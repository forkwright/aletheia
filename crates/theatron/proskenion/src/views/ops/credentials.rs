//! Credential management panel: display, validate, rotate, add, and remove credentials.

use dioxus::prelude::*;
use skene::api::routes::system::{
    credential_rotate_url, credential_url, credential_validate_url, credentials_url,
};

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::credentials::{
    CredentialEntry, CredentialRole, CredentialStore, ValidationStatus, canonicalize_masked_key,
};
use crate::state::fetch::FetchState;

// ── API types ──

#[derive(Clone, serde::Deserialize)]
struct CredentialsListResponse {
    #[serde(default)]
    credentials: Vec<CredentialApiEntry>,
}

#[derive(Clone, serde::Deserialize)]
struct CredentialApiEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    provider: String,
    #[serde(default)]
    role: String,
    /// Key value from API. Must be canonicalized before use.
    #[serde(default)]
    masked_key: String, // kanon:ignore RUST/plain-string-secret -- transient API field is canonicalized before entering reactive CredentialEntry state (#4876); this type does not derive Debug
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
        let masked = canonicalize_masked_key(&self.masked_key);
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

/// Serialise a `SecretString` by exposing its inner value so the raw
/// API key reaches the aletheia server during credential creation.
///
/// WHY: the HTTP body must carry the actual key; `SecretString`'s default
/// `Serialize` would emit `"[REDACTED]"` and break the request. The
/// secret is still zeroised on drop and redacted in `Debug`/`Display`.
fn serialize_secret_expose<S: serde::Serializer>(
    secret: &koina::secret::SecretString,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(secret.expose_secret())
}

#[derive(Clone, serde::Serialize)]
struct AddCredentialRequest {
    provider: String,
    /// Raw key -- cleared from reactive state immediately after spawn.
    ///
    /// WHY: wrapped in `SecretString` so `Debug`/stray logging cannot
    /// leak the plaintext API key; serialised via `expose_secret` so the
    /// JSON body still reaches aletheia's credential endpoint intact.
    #[serde(serialize_with = "serialize_secret_expose")]
    key: koina::secret::SecretString,
    role: String,
}

// ── Styles ──

const PANEL_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: var(--space-3); \
    flex: 1; \
    overflow-y: auto;\
";

const CRED_CARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-5);\
";

const CARD_HEADER: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: var(--space-3);\
";

const PROVIDER_NAME: &str = "\
    font-size: var(--text-md); \
    font-weight: var(--weight-bold); \
    color: var(--text-primary);\
";

const META_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-4); \
    margin-bottom: var(--space-2); \
    font-size: var(--text-sm);\
";

const STATS_ROW: &str = "\
    display: flex; \
    gap: var(--space-4); \
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    margin-bottom: var(--space-3);\
";

const ACTIONS_ROW: &str = "\
    display: flex; \
    gap: var(--space-2); \
    align-items: center; \
    flex-wrap: wrap;\
";

const BTN_STD: &str = "\
    background: var(--border); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer;\
    transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);\
";

const BTN_DANGER: &str = "\
    background: var(--status-error-bg); \
    color: var(--status-error); \
    border: 1px solid var(--status-error); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer;\
    transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);\
";

const BTN_CONFIRM: &str = "\
    background: var(--status-error); \
    color: var(--text-primary); \
    border: none; \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer;\
    transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);\
";

const BTN_CANCEL: &str = "\
    background: var(--bg-surface-bright); \
    color: var(--text-secondary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer;\
    transition: background-color var(--transition-quick), color var(--transition-quick), border-color var(--transition-quick);\
";

const BTN_DISABLED: &str = "\
    background: var(--bg-surface); \
    color: var(--text-muted); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: not-allowed;\
";

const CONFIRM_BANNER: &str = "\
    display: flex; \
    gap: var(--space-2); \
    align-items: center; \
    padding: var(--space-2) 0; \
    border-top: 1px solid var(--border); \
    margin-top: var(--space-3);\
";

const WARN_TEXT: &str = "\
    font-size: var(--text-xs); \
    color: var(--status-warning); \
    flex: 1;\
";

const ADD_CARD_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-5);\
";

const FORM_TITLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-bold); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-3);\
";

const FORM_ROW: &str = "\
    display: flex; \
    gap: var(--space-3); \
    align-items: flex-end; \
    flex-wrap: wrap; \
    margin-bottom: var(--space-3);\
";

const FORM_GROUP: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: var(--space-1);\
";

const FORM_LABEL: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const FORM_INPUT: &str = "\
    background: var(--bg-surface-dim); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-3); \
    font-size: var(--text-sm); \
    width: 160px;\
";

const FORM_SELECT: &str = "\
    background: var(--bg-surface-dim); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-3); \
    font-size: var(--text-sm);\
";

const ERROR_TEXT: &str = "\
    font-size: var(--text-xs); \
    color: var(--status-error); \
    margin-top: var(--space-1);\
";

// ── Components ──

/// Credential management panel.
#[component]
pub(crate) fn CredentialsView() -> Element {
    let mut fetch_trigger = use_signal(|| 0u32);
    let mut fetch_state: Signal<FetchState<CredentialStore>> = use_signal(|| FetchState::Loading);
    let config: Signal<ConnectionConfig> = use_context();

    let mut show_add = use_signal(|| false);
    let mut add_provider = use_signal(String::new);
    // WHY(#4876): browser password controls and input events necessarily carry
    // plaintext while typing. Keep the value in SecretString, never derive
    // Debug for request/payload types that can contain it, and remount the input
    // whenever the signal is cleared so plaintext does not linger in UI state.
    let mut add_key: Signal<koina::secret::SecretString> =
        use_signal(|| koina::secret::SecretString::from(String::new()));
    let mut add_key_epoch = use_signal(|| 0u64);
    let mut add_role: Signal<CredentialRole> = use_signal(|| CredentialRole::Primary);
    let mut add_error: Signal<Option<String>> = use_signal(|| None);

    use_effect(move || {
        let _trigger = *fetch_trigger.read();
        let cfg = config.read().clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let client = match authenticated_client(&cfg) {
                Ok(client) => client,
                Err(err) => {
                    fetch_state.set(FetchState::Error(err.to_string()));
                    return;
                }
            };
            let url = credentials_url(&cfg.server_url);
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
                            fetch_state.set(FetchState::Error(format!("parse error: {e}")));
                        }
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    fetch_state.set(FetchState::Error(format!("server returned {status}")));
                }
                Err(e) => {
                    fetch_state.set(FetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    });

    let mut do_add = move || {
        let provider = add_provider.read().trim().to_string();
        let role = *add_role.read();

        if provider.is_empty() {
            add_error.set(Some("Provider is required.".to_string()));
            add_key.set(koina::secret::SecretString::from(String::new()));
            add_key_epoch.set(add_key_epoch() + 1);
            return;
        }
        let key_is_empty = {
            let key = add_key.read();
            key.expose_secret().trim().is_empty()
        };
        if key_is_empty {
            add_error.set(Some("Key is required.".to_string()));
            add_key.set(koina::secret::SecretString::from(String::new()));
            add_key_epoch.set(add_key_epoch() + 1);
            return;
        }
        add_error.set(None);

        let role_str = match role {
            CredentialRole::Primary => "primary".to_string(),
            CredentialRole::Backup => "backup".to_string(),
        };
        let payload = AddCredentialRequest {
            provider,
            key: {
                let key = add_key.read();
                koina::secret::SecretString::from(key.expose_secret().trim().to_owned())
            },
            role: role_str,
        };
        let cfg = config.read().clone();

        // WHY: Clear key immediately before spawning so the raw value does not
        // linger in reactive state after the async task begins.
        add_key.set(koina::secret::SecretString::from(String::new()));
        add_key_epoch.set(add_key_epoch() + 1);

        spawn(async move {
            let client = match authenticated_client(&cfg) {
                Ok(client) => client,
                Err(err) => {
                    add_error.set(Some(err.to_string()));
                    return;
                }
            };
            let url = credentials_url(&cfg.server_url);
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
                div { style: "color: var(--text-secondary); font-size: var(--text-sm);", "Loading credentials..." }
            }

            if let Some(err) = &fetch_error_msg {
                div { style: "color: var(--status-error); font-size: var(--text-sm);", "Error: {err}" }
            }

            if !fetch_loading && fetch_error_msg.is_none() && cards.is_empty() {
                div { style: "color: var(--text-muted); font-size: var(--text-sm);", "No credentials configured." }
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
                                key: "credential-key-{add_key_epoch}",
                                style: "{FORM_INPUT}",
                                r#type: "password",
                                placeholder: "sk-...",
                                oninput: move |evt: Event<FormData>| {
                                    add_key.set(koina::secret::SecretString::from(evt.value().clone()));
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
                        style: "display: flex; gap: var(--space-2); margin-top: var(--space-1);",
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
                                add_key.set(koina::secret::SecretString::from(String::new()));
                                add_key_epoch.set(add_key_epoch() + 1);
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
                let client = match authenticated_client(&cfg) {
                    Ok(client) => client,
                    Err(err) => {
                        is_validating.set(false);
                        card_error.set(Some(err.to_string()));
                        return;
                    }
                };
                let url = credential_validate_url(&cfg.server_url, &id_v);
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
                let client = match authenticated_client(&cfg) {
                    Ok(client) => client,
                    Err(err) => {
                        card_error.set(Some(err.to_string()));
                        return;
                    }
                };
                let url = credential_rotate_url(&cfg.server_url, &prov);
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
                let client = match authenticated_client(&cfg) {
                    Ok(client) => client,
                    Err(err) => {
                        card_error.set(Some(err.to_string()));
                        return;
                    }
                };
                let url = credential_url(&cfg.server_url, &id_r);
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
        "background: var(--status-info-bg); color: var(--status-info);"
    } else {
        "background: var(--bg-surface-bright); color: var(--text-secondary);"
    };

    rsx! {
        div {
            style: "{CRED_CARD_STYLE}",

            div {
                style: "{CARD_HEADER}",
                span { style: "{PROVIDER_NAME}", "{entry.provider}" }
                span {
                    style: "font-size: var(--text-xs); padding: var(--space-1) var(--space-2); border-radius: var(--radius-sm); \
                            font-weight: var(--weight-bold); text-transform: uppercase; letter-spacing: 0.5px; \
                            {role_bg}",
                    "{entry.role.label()}"
                }
            }

            div {
                style: "{META_ROW}",
                span {
                    style: "font-family: var(--font-mono); color: var(--text-secondary); font-size: var(--text-sm);",
                    "{entry.masked_key}"
                }
                span {
                    style: "display: inline-flex; align-items: center; gap: var(--space-1); font-size: var(--text-sm); \
                            color: {entry.status.color()};",
                    span {
                        style: "width: 8px; height: 8px; border-radius: 50%; \
                                background: {entry.status.color()}; display: inline-block;",
                    }
                    "{entry.status.label()}"
                }
            }

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

            if let Some(err) = &*card_error.read() {
                div { style: "color: var(--status-error); font-size: var(--text-xs); margin-top: var(--space-2);", "{err}" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_entry(masked_key: &str) -> CredentialApiEntry {
        CredentialApiEntry {
            id: "anthropic:primary".to_string(),
            provider: "anthropic".to_string(),
            role: "primary".to_string(),
            masked_key: masked_key.to_string(),
            status: "valid".to_string(),
            last_validated: None,
            requests_today: 0,
            tokens_today: 0,
        }
    }

    #[test]
    fn credentials_urls_use_versioned_system_api() {
        let base = "http://localhost:8080/";

        assert_eq!(
            credentials_url(base),
            "http://localhost:8080/api/v1/system/credentials"
        );
        assert_eq!(
            credential_url(base, "anthropic:backup"),
            "http://localhost:8080/api/v1/system/credentials/anthropic%3Abackup"
        );
        assert_eq!(
            credential_validate_url(base, "anthropic:primary"),
            "http://localhost:8080/api/v1/system/credentials/anthropic%3Aprimary/validate"
        );
        assert_eq!(
            credential_rotate_url(base, "open ai/a?b#c:100%"),
            "http://localhost:8080/api/v1/system/credentials/rotate?provider=open+ai%2Fa%3Fb%23c%3A100%25"
        );
    }

    #[test]
    fn api_entry_canonicalizes_malformed_prefixed_mask() {
        let entry = api_entry("...raw-secret-material").into_entry();

        assert_eq!(entry.masked_key, "...????");
        assert!(!entry.masked_key.contains("raw"));
        assert!(!entry.masked_key.contains("material"));
    }

    #[test]
    fn api_entry_masks_unprefixed_raw_key() {
        let entry = api_entry("sk-test-secret-1234").into_entry();

        assert_eq!(entry.masked_key, "...1234");
        assert!(!entry.masked_key.contains("test-secret"));
    }
}
