//! Credential management panel: display, validate, rotate, add, and remove credentials.

use dioxus::prelude::*;
use reqwest::StatusCode;
use serde_json::Value;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::credentials::{
    CredentialEntry, CredentialRole, CredentialStore, ValidationStatus, mask_key,
};

const CREDENTIALS_PATH: &str = "/api/v1/system/credentials";

fn credentials_url(base: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), CREDENTIALS_PATH)
}

fn credential_url(base: &str, id: &str) -> String {
    format!("{}/{id}", credentials_url(base))
}

fn credential_validate_url(base: &str, id: &str) -> String {
    format!("{}/{id}/validate", credentials_url(base))
}

fn credential_rotate_url(base: &str, provider: &str) -> String {
    let encoded = keryx::url::encode_path_segment(provider);
    format!("{}/rotate?provider={encoded}", credentials_url(base))
}

// ── API types ──

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
    masked_key: String, // kanon:ignore RUST/plain-string-secret -- API payload is masked before it enters UI state (#3988)
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

#[derive(Debug, Clone, serde::Serialize)]
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

#[derive(Debug, Clone, PartialEq)]
enum CredentialsPanelState {
    Loading,
    Ready(CredentialStore),
    PermissionDenied(String),
    Error(String),
}

impl CredentialsPanelState {
    fn can_manage(&self) -> bool {
        matches!(self, Self::Ready(_))
    }
}

#[derive(Clone, PartialEq, Eq)]
struct AddCredentialFormState {
    provider: String,
    key: String, // kanon:ignore RUST/plain-string-secret -- transient password input, cleared before async submission and never logged
    role: CredentialRole,
    error: Option<String>,
    pending: bool,
}

impl Default for AddCredentialFormState {
    fn default() -> Self {
        Self {
            provider: String::new(),
            key: String::new(),
            role: CredentialRole::Primary,
            error: None,
            pending: false,
        }
    }
}

impl AddCredentialFormState {
    fn reset(&mut self) {
        *self = Self::default();
    }

    fn set_provider(&mut self, provider: String) {
        self.provider = provider;
        self.error = None;
    }

    fn set_key(&mut self, key: String) {
        self.key = key;
        self.error = None;
    }

    fn set_role_from_wire_value(&mut self, value: &str) {
        self.role = CredentialRole::from_wire_value(value);
        self.error = None;
    }

    fn selected_role_value(&self) -> &'static str {
        self.role.wire_value()
    }

    fn begin_submit(&mut self) -> Option<AddCredentialSubmission> {
        if self.pending {
            return None;
        }

        let provider = self.provider.trim().to_string();
        let key = self.key.trim().to_string();

        if provider.is_empty() {
            self.error = Some("Provider is required.".to_string());
            return None;
        }
        if key.is_empty() {
            self.error = Some("Key is required.".to_string());
            return None;
        }

        self.pending = true;
        self.error = None;
        self.key.clear();
        Some(AddCredentialSubmission {
            provider,
            key: koina::secret::SecretString::from(key),
            role: self.role,
        })
    }

    fn finish_success(&mut self) {
        self.reset();
    }

    fn finish_failure(&mut self, message: String) {
        self.pending = false;
        self.key.clear();
        self.error = Some(message);
    }
}

struct AddCredentialSubmission {
    provider: String,
    key: koina::secret::SecretString,
    role: CredentialRole,
}

impl AddCredentialSubmission {
    fn into_request(self) -> AddCredentialRequest {
        AddCredentialRequest {
            provider: self.provider,
            key: self.key,
            role: self.role.wire_value().to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CredentialActionPending {
    validating: bool,
    rotating: bool,
    removing: bool,
}

impl CredentialActionPending {
    fn busy(self) -> bool {
        self.validating || self.rotating || self.removing
    }

    fn begin_validate(&mut self) -> bool {
        if self.busy() {
            return false;
        }
        self.validating = true;
        true
    }

    fn finish_validate(&mut self) {
        self.validating = false;
    }

    fn begin_rotate(&mut self) -> bool {
        if self.busy() {
            return false;
        }
        self.rotating = true;
        true
    }

    fn finish_rotate(&mut self) {
        self.rotating = false;
    }

    fn begin_remove(&mut self) -> bool {
        if self.busy() {
            return false;
        }
        self.removing = true;
        true
    }

    fn finish_remove(&mut self) {
        self.removing = false;
    }
}

fn credential_error_message(action: &str, status: StatusCode, body: &str) -> String {
    let detail = structured_credential_error(status, body).unwrap_or_else(|| {
        format!("server returned {status}")
    });
    format!("{action} failed: {detail}")
}

fn credential_load_error_state(status: StatusCode, body: &str) -> CredentialsPanelState {
    let detail = structured_credential_error(status, body)
        .unwrap_or_else(|| format!("server returned {status}"));

    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        CredentialsPanelState::PermissionDenied(format!(
            "Credential management requires ManageCredentials permission. {detail}"
        ))
    } else {
        CredentialsPanelState::Error(detail)
    }
}

fn structured_credential_error(status: StatusCode, body: &str) -> Option<String> {
    let envelope = skene::api::error::parse_pylon_error_envelope(status.as_u16(), body)?;
    let mut parts = Vec::new();
    if !envelope.error.code.is_empty() {
        parts.push(format!("code {}", envelope.error.code));
    }
    if let Some(request_id) = envelope
        .error
        .request_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        parts.push(format!("request_id {request_id}"));
    }
    let field_details = validation_field_messages(envelope.error.details.as_ref());
    if !field_details.is_empty() {
        parts.push(format!("fields {}", field_details.join(", ")));
    }

    if parts.is_empty() {
        Some(envelope.error.message)
    } else {
        Some(format!("{} ({})", envelope.error.message, parts.join("; ")))
    }
}

fn validation_field_messages(details: Option<&Value>) -> Vec<String> {
    details
        .and_then(|value| value.get("fields"))
        .and_then(Value::as_array)
        .map(|fields| {
            fields
                .iter()
                .filter_map(|field| {
                    let name = field
                        .get("field")
                        .and_then(Value::as_str)
                        .unwrap_or("_body");
                    let code = field
                        .get("code")
                        .and_then(Value::as_str)
                        .unwrap_or("invalid");
                    let message = field
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("invalid value");
                    if name.is_empty() && message.is_empty() {
                        None
                    } else {
                        Some(format!("{name} {code}: {message}"))
                    }
                })
                .collect()
        })
        .unwrap_or_default()
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
pub(crate) fn CredentialsView(refresh_token: u32) -> Element {
    let mut fetch_trigger = use_signal(|| 0u32);
    let mut fetch_state: Signal<CredentialsPanelState> =
        use_signal(|| CredentialsPanelState::Loading);
    let config: Signal<ConnectionConfig> = use_context();

    let mut show_add = use_signal(|| false);
    let mut add_form = use_signal(AddCredentialFormState::default);

    use_effect(move || {
        let _parent_refresh = refresh_token;
        let _trigger = *fetch_trigger.read();
        let cfg = config.read().clone();
        fetch_state.set(CredentialsPanelState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
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
                            fetch_state.set(CredentialsPanelState::Ready(CredentialStore {
                                entries,
                            }));
                        }
                        Err(e) => {
                            fetch_state.set(CredentialsPanelState::Error(format!(
                                "parse error: {e}"
                            )));
                        }
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    let state = credential_load_error_state(status, &body);
                    if !state.can_manage() {
                        show_add.set(false);
                        add_form.write().reset();
                    }
                    fetch_state.set(state);
                }
                Err(e) => {
                    show_add.set(false);
                    add_form.write().reset();
                    fetch_state.set(CredentialsPanelState::Error(format!(
                        "connection error: {e}"
                    )));
                }
            }
        });
    });

    let mut do_add = move || {
        if !fetch_state.read().can_manage() {
            return;
        }

        let Some(submission) = add_form.write().begin_submit() else {
            return;
        };
        let cfg = config.read().clone();
        let payload = submission.into_request();

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = credentials_url(&cfg.server_url);
            match client.post(&url).json(&payload).send().await {
                Ok(resp) if resp.status().is_success() => {
                    add_form.write().finish_success();
                    show_add.set(false);
                    fetch_trigger.set(fetch_trigger() + 1);
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    add_form
                        .write()
                        .finish_failure(credential_error_message("Add", status, &body));
                }
                Err(e) => {
                    add_form
                        .write()
                        .finish_failure(format!("Connection error: {e}"));
                }
            }
        });
    };

    // Collect card data from the loaded state (owned values for the RSX loop).
    let (cards, fetch_loading, access_message, fetch_error_msg, can_manage) = {
        let state = fetch_state.read();
        match &*state {
            CredentialsPanelState::Loading => (Vec::new(), true, None, None, false),
            CredentialsPanelState::PermissionDenied(message) => {
                (Vec::new(), false, Some(message.clone()), None, false)
            }
            CredentialsPanelState::Error(e) => (Vec::new(), false, None, Some(e.clone()), false),
            CredentialsPanelState::Ready(store) => {
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
                (cards, false, None, None, true)
            }
        }
    };
    let add_snapshot = add_form.read().clone();
    let add_role_value = add_snapshot.selected_role_value();

    rsx! {
        div {
            style: "{PANEL_STYLE}",

            if fetch_loading {
                div { style: "color: var(--text-secondary); font-size: var(--text-sm);", "Loading credentials..." }
            }

            if let Some(message) = &access_message {
                div { style: "color: var(--status-warning); font-size: var(--text-sm);", "{message}" }
            }

            if let Some(err) = &fetch_error_msg {
                div { style: "color: var(--status-error); font-size: var(--text-sm);", "Error: {err}" }
            }

            if can_manage && cards.is_empty() {
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

            if can_manage && *show_add.read() {
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
                                value: "{add_snapshot.provider}",
                                disabled: add_snapshot.pending,
                                oninput: move |evt: Event<FormData>| {
                                    add_form.write().set_provider(evt.value().clone());
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
                                value: "{add_snapshot.key}",
                                disabled: add_snapshot.pending,
                                oninput: move |evt: Event<FormData>| {
                                    add_form.write().set_key(evt.value().clone());
                                },
                            }
                        }
                        div {
                            style: "{FORM_GROUP}",
                            span { style: "{FORM_LABEL}", "Role" }
                            select {
                                style: "{FORM_SELECT}",
                                value: "{add_role_value}",
                                disabled: add_snapshot.pending,
                                onchange: move |evt: Event<FormData>| {
                                    add_form.write().set_role_from_wire_value(&evt.value());
                                },
                                option { value: "primary", "Primary" }
                                option { value: "backup", "Backup" }
                            }
                        }
                    }
                    if let Some(err) = &add_snapshot.error {
                        div { style: "{ERROR_TEXT}", "{err}" }
                    }
                    div {
                        style: "display: flex; gap: var(--space-2); margin-top: var(--space-1);",
                        if add_snapshot.pending {
                            button {
                                style: "{BTN_DISABLED}",
                                disabled: true,
                                "Adding..."
                            }
                        } else {
                            button {
                                style: "{BTN_STD}",
                                onclick: move |_| do_add(),
                                "Add"
                            }
                        }
                        button {
                            style: if add_snapshot.pending { BTN_DISABLED } else { BTN_CANCEL },
                            disabled: add_snapshot.pending,
                            onclick: move |_| {
                                if add_form.read().pending {
                                    return;
                                }
                                show_add.set(false);
                                add_form.write().reset();
                            },
                            "Cancel"
                        }
                    }
                }
            } else if can_manage {
                button {
                    style: "{BTN_STD}",
                    onclick: move |_| {
                        add_form.write().reset();
                        show_add.set(true);
                    },
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
    let mut pending = use_signal(CredentialActionPending::default);
    let mut confirm_rotate = use_signal(|| false);
    let mut confirm_remove = use_signal(|| false);
    let mut card_error: Signal<Option<String>> = use_signal(|| None);

    let entry_id = entry.id.clone();
    let entry_provider = entry.provider.clone();

    let mut do_validate = {
        let id = entry_id.clone();
        move || {
            if !pending.write().begin_validate() {
                return;
            }
            let cfg = config.read().clone();
            let id_v = id.clone();
            card_error.set(None);

            spawn(async move {
                let client = authenticated_client(&cfg);
                let url = credential_validate_url(&cfg.server_url, &id_v);
                match client.post(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        pending.write().finish_validate();
                        on_change.call(());
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        pending.write().finish_validate();
                        card_error.set(Some(credential_error_message(
                            "Validate", status, &body,
                        )));
                    }
                    Err(e) => {
                        pending.write().finish_validate();
                        card_error.set(Some(format!("Connection error: {e}")));
                    }
                }
            });
        }
    };

    let mut do_rotate = {
        let provider = entry_provider.clone();
        move || {
            if !pending.write().begin_rotate() {
                return;
            }
            let cfg = config.read().clone();
            let prov = provider.clone();
            card_error.set(None);

            spawn(async move {
                let client = authenticated_client(&cfg);
                let url = credential_rotate_url(&cfg.server_url, &prov);
                match client.post(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        pending.write().finish_rotate();
                        confirm_rotate.set(false);
                        on_change.call(());
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        pending.write().finish_rotate();
                        card_error.set(Some(credential_error_message("Rotate", status, &body)));
                    }
                    Err(e) => {
                        pending.write().finish_rotate();
                        card_error.set(Some(format!("Connection error: {e}")));
                    }
                }
            });
        }
    };

    let mut do_remove = {
        let id = entry_id.clone();
        move || {
            if !pending.write().begin_remove() {
                return;
            }
            let cfg = config.read().clone();
            let id_r = id.clone();
            card_error.set(None);

            spawn(async move {
                let client = authenticated_client(&cfg);
                let url = credential_url(&cfg.server_url, &id_r);
                match client.delete(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        pending.write().finish_remove();
                        confirm_remove.set(false);
                        on_change.call(());
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        pending.write().finish_remove();
                        card_error.set(Some(credential_error_message("Remove", status, &body)));
                    }
                    Err(e) => {
                        pending.write().finish_remove();
                        card_error.set(Some(format!("Connection error: {e}")));
                    }
                }
            });
        }
    };

    let pending_actions = *pending.read();
    let validating = pending_actions.validating;
    let rotating = pending_actions.rotating;
    let removing = pending_actions.removing;
    let action_busy = pending_actions.busy();
    let rotate_confirm_label = if rotating { "Rotating..." } else { "Confirm" };
    let remove_confirm_label = if removing { "Removing..." } else { "Remove" };
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
                } else if action_busy {
                    button { style: "{BTN_DISABLED}", disabled: true, "Validate" }
                } else {
                    button {
                        style: "{BTN_STD}",
                        onclick: move |_| do_validate(),
                        "Validate"
                    }
                }

                if can_rotate {
                    if rotating {
                        button { style: "{BTN_DISABLED}", disabled: true, "Rotating..." }
                    } else if action_busy {
                        button { style: "{BTN_DISABLED}", disabled: true, "Rotate" }
                    } else {
                        button {
                            style: "{BTN_STD}",
                            onclick: move |_| {
                                confirm_rotate.set(true);
                                confirm_remove.set(false);
                            },
                            "Rotate"
                        }
                    }
                }

                if is_last_primary {
                    button {
                        style: "{BTN_DISABLED}",
                        disabled: true,
                        title: "Cannot remove the last primary credential",
                        "Remove"
                    }
                } else if removing {
                    button { style: "{BTN_DISABLED}", disabled: true, "Removing..." }
                } else if action_busy {
                    button { style: "{BTN_DISABLED}", disabled: true, "Remove" }
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
                        style: if rotating { BTN_DISABLED } else { BTN_CONFIRM },
                        disabled: rotating,
                        onclick: move |_| do_rotate(),
                        "{rotate_confirm_label}"
                    }
                    button {
                        style: if rotating { BTN_DISABLED } else { BTN_CANCEL },
                        disabled: rotating,
                        onclick: move |_| {
                            if !pending.read().rotating {
                                confirm_rotate.set(false);
                            }
                        },
                        "Cancel"
                    }
                }
            }

            if show_remove {
                div {
                    style: "{CONFIRM_BANNER}",
                    span { style: "{WARN_TEXT}", "Permanently remove this credential?" }
                    button {
                        style: if removing { BTN_DISABLED } else { BTN_CONFIRM },
                        disabled: removing,
                        onclick: move |_| do_remove(),
                        "{remove_confirm_label}"
                    }
                    button {
                        style: if removing { BTN_DISABLED } else { BTN_CANCEL },
                        disabled: removing,
                        onclick: move |_| {
                            if !pending.read().removing {
                                confirm_remove.set(false);
                            }
                        },
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

    #[test]
    fn credentials_urls_use_versioned_system_api() {
        let base = "http://localhost:8080/";

        assert_eq!(
            credentials_url(base),
            "http://localhost:8080/api/v1/system/credentials"
        );
        assert_eq!(
            credential_url(base, "anthropic:backup"),
            "http://localhost:8080/api/v1/system/credentials/anthropic:backup"
        );
        assert_eq!(
            credential_validate_url(base, "anthropic:primary"),
            "http://localhost:8080/api/v1/system/credentials/anthropic:primary/validate"
        );
        assert_eq!(
            credential_rotate_url(base, "open ai"),
            "http://localhost:8080/api/v1/system/credentials/rotate?provider=open%20ai"
        );
    }

    #[test]
    fn forbidden_load_surfaces_permission_state_without_mutations() {
        let body = r#"{"error":{"code":"forbidden","message":"insufficient permissions","request_id":"req-403"}}"#;

        let state = credential_load_error_state(StatusCode::FORBIDDEN, body);

        assert!(!state.can_manage());
        match state {
            CredentialsPanelState::PermissionDenied(message) => {
                assert!(message.contains("ManageCredentials"));
                assert!(message.contains("insufficient permissions"));
                assert!(message.contains("request_id req-403"));
            }
            other => panic!("expected permission state, got {other:?}"),
        }
    }

    #[test]
    fn add_form_submits_visible_role_and_clears_key_state() {
        let mut form = AddCredentialFormState::default();
        form.set_provider("anthropic".to_string());
        form.set_key(" sk-test-key ".to_string());
        form.set_role_from_wire_value("backup");

        assert_eq!(form.selected_role_value(), "backup");

        let submission = form
            .begin_submit()
            .expect("valid form should produce a submission");
        assert_eq!(submission.role, CredentialRole::Backup);
        assert_eq!(form.key, "");
        assert!(form.pending);

        let request = submission.into_request();
        assert_eq!(request.provider, "anthropic");
        assert_eq!(request.role, "backup");
    }

    #[test]
    fn add_form_resets_on_open_cancel_success_and_failure() {
        let mut form = AddCredentialFormState::default();
        form.set_provider("anthropic".to_string());
        form.set_key("sk-test-key".to_string());
        form.set_role_from_wire_value("backup");
        form.error = Some("previous error".to_string());
        form.pending = true;

        form.reset();
        assert_eq!(form.provider, "");
        assert_eq!(form.key, "");
        assert_eq!(form.role, CredentialRole::Primary);
        assert!(form.error.is_none());
        assert!(!form.pending);

        form.set_provider("openai".to_string());
        form.set_key("sk-other-key".to_string());
        form.set_role_from_wire_value("backup");
        let _submission = form
            .begin_submit()
            .expect("valid form should enter pending state");
        form.finish_failure("Add failed: conflict".to_string());

        assert_eq!(form.provider, "openai");
        assert_eq!(form.role, CredentialRole::Backup);
        assert_eq!(form.key, "");
        assert_eq!(form.error.as_deref(), Some("Add failed: conflict"));
        assert!(!form.pending);

        form.finish_success();
        assert_eq!(form.provider, "");
        assert_eq!(form.role, CredentialRole::Primary);
        assert!(form.error.is_none());
    }

    #[test]
    fn pending_state_blocks_duplicate_and_overlapping_mutations() {
        let mut pending = CredentialActionPending::default();

        assert!(pending.begin_rotate());
        assert!(!pending.begin_rotate());
        assert!(!pending.begin_remove());
        assert!(!pending.begin_validate());
        pending.finish_rotate();

        assert!(pending.begin_validate());
        assert!(!pending.begin_validate());
        pending.finish_validate();

        assert!(pending.begin_remove());
        assert!(!pending.begin_remove());
        pending.finish_remove();
        assert!(!pending.busy());
    }

    #[test]
    fn structured_error_display_keeps_operator_detail_without_secret_echo() {
        let body = r#"{"error":{"code":"validation_error","message":"invalid credential request","request_id":"req-abc","details":{"fields":[{"field":"provider","code":"required","message":"provider is required"}],"received_key":"sk-secret-value"}}}"#;

        let message = credential_error_message("Add", StatusCode::BAD_REQUEST, body);

        assert!(message.contains("Add failed: invalid credential request"));
        assert!(message.contains("code validation_error"));
        assert!(message.contains("request_id req-abc"));
        assert!(message.contains("provider required: provider is required"));
        assert!(!message.contains("sk-secret-value"));
    }

    #[test]
    fn add_form_pending_blocks_duplicate_submit() {
        let mut form = AddCredentialFormState::default();
        form.set_provider("anthropic".to_string());
        form.set_key("sk-test-key".to_string());

        assert!(form.begin_submit().is_some());
        assert!(form.begin_submit().is_none());
    }
}
