//! Credential management panel: display, validate, rotate, add, and remove credentials.

use dioxus::prelude::*;
use skene::api::routes::system::{
    credential_rotate_url, credential_url, credential_validate_url, credentials_url,
};

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::credentials::{
    CredentialEntry, CredentialRole, CredentialStore, ValidationStatus, can_manage_credentials,
    canonicalize_masked_key, decode_role_claim,
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
    /// `true` when `status` reflects an actual provider round trip (#4875).
    #[serde(default)]
    provider_verified: bool,
    #[serde(default)]
    last_validated: Option<String>,
    #[serde(default)]
    requests_today: u64,
    #[serde(default)]
    tokens_today: u64,
}

/// Structured error envelope pylon returns for non-2xx responses.
///
/// WHY(#4877): the backend already returns `{"error": {"code", "message"}}`
/// on every failure; surfacing only the HTTP status code discarded that.
#[derive(Clone, serde::Deserialize)]
struct ApiErrorEnvelope {
    error: ApiErrorBody,
}

#[derive(Clone, serde::Deserialize)]
struct ApiErrorBody {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

/// Build a user-facing error message for `action` from a non-2xx response,
/// preferring the structured `{"error": {code, message}}` envelope pylon
/// returns and falling back to the bare HTTP status when the body doesn't
/// parse as that shape (e.g. a proxy-generated error page).
async fn describe_error_response(action: &str, resp: reqwest::Response) -> String {
    let status = resp.status();
    match resp.json::<ApiErrorEnvelope>().await {
        Ok(envelope) if !envelope.error.message.is_empty() => {
            format!(
                "{action} failed: {} ({})",
                envelope.error.message, envelope.error.code
            )
        }
        _ => format!("{action} failed: {status}"),
    }
}

impl CredentialApiEntry {
    fn into_entry(self) -> CredentialEntry {
        let role = if self.role == "primary" {
            CredentialRole::Primary
        } else {
            CredentialRole::Backup
        };
        let status = ValidationStatus::from_wire(&self.status);
        let masked = canonicalize_masked_key(&self.masked_key);
        CredentialEntry {
            id: self.id,
            provider: self.provider,
            role,
            masked_key: masked,
            status,
            provider_verified: self.provider_verified,
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
///
/// `refresh_trigger` is bumped by the Ops-level Refresh button (WHY(#4877):
/// that button was previously a no-op on this tab, since credentials fetch
/// state lived entirely inside this component with nothing external able to
/// drive it) -- a bump re-runs the same fetch effect as the internal
/// `fetch_trigger` that mutation success handlers already use.
#[component]
pub(crate) fn CredentialsView(refresh_trigger: Signal<u32>) -> Element {
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
    let mut is_adding = use_signal(|| false);

    // WHY(#4877): decoded from the locally-held access token so the panel
    // knows the caller's capability before rendering controls it cannot use.
    // A UI-affordance check only -- see `decode_role_claim` -- the server
    // remains the sole enforcement authority on every request either way.
    let can_manage = {
        let cfg = config.read();
        let role = cfg.auth_token.as_deref().and_then(decode_role_claim);
        can_manage_credentials(role.as_deref())
    };

    use_effect(move || {
        let _trigger = *fetch_trigger.read();
        let _external_trigger = *refresh_trigger.read();
        let cfg = config.read().clone();
        let allowed = {
            let role = cfg.auth_token.as_deref().and_then(decode_role_claim);
            can_manage_credentials(role.as_deref())
        };
        if !allowed {
            // WHY: never issue a request the server is guaranteed to 403 --
            // matches the permission gate this component renders below.
            return;
        }
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
        // WHY(#4877): guard against a double-click submitting the same add
        // twice while the first request is still in flight.
        if *is_adding.read() {
            return;
        }

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
        is_adding.set(true);

        spawn(async move {
            let client = match authenticated_client(&cfg) {
                Ok(client) => client,
                Err(err) => {
                    add_error.set(Some(err.to_string()));
                    is_adding.set(false);
                    return;
                }
            };
            let url = credentials_url(&cfg.server_url);
            match client.post(&url).json(&payload).send().await {
                Ok(resp) if resp.status().is_success() => {
                    add_provider.set(String::new());
                    add_role.set(CredentialRole::Primary);
                    show_add.set(false);
                    is_adding.set(false);
                    fetch_trigger.set(fetch_trigger() + 1);
                }
                Ok(resp) => {
                    let msg = describe_error_response("Add", resp).await;
                    add_error.set(Some(msg));
                    is_adding.set(false);
                }
                Err(e) => {
                    add_error.set(Some(format!("Connection error: {e}")));
                    is_adding.set(false);
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

    if !can_manage {
        // WHY(#4877): non-operators/admins never even issue the list
        // request (see the `use_effect` guard above) -- this is the paired
        // rendering half: a clear permission state instead of a raw 403, and
        // no mutation controls of any kind, since every credentials endpoint
        // (including list) requires the same ManageCredentials action.
        return rsx! {
            div {
                style: "{PANEL_STYLE}",
                div {
                    style: "color: var(--text-secondary); font-size: var(--text-sm); padding: var(--space-3) 0;",
                    "You do not have permission to manage credentials. This requires the Operator or Admin role."
                }
            }
        };
    }

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

            // WHY(#4877): after a fetch error, controls the caller cannot
            // meaningfully use (the list they'd mutate is unknown) must not
            // still render as though nothing is wrong.
            if fetch_error_msg.is_none() {
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
                                    // WHY(#4877): bind `selected` to the actual
                                    // signal value -- it was previously
                                    // hardcoded to Primary regardless of what
                                    // the caller had chosen.
                                    option {
                                        value: "primary",
                                        selected: *add_role.read() == CredentialRole::Primary,
                                        "Primary"
                                    }
                                    option {
                                        value: "backup",
                                        selected: *add_role.read() == CredentialRole::Backup,
                                        "Backup"
                                    }
                                }
                            }
                        }
                        if let Some(err) = &*add_error.read() {
                            div { style: "{ERROR_TEXT}", "{err}" }
                        }
                        div {
                            style: "display: flex; gap: var(--space-2); margin-top: var(--space-1);",
                            if *is_adding.read() {
                                button { style: "{BTN_DISABLED}", disabled: true, "Adding..." }
                            } else {
                                button {
                                    style: "{BTN_STD}",
                                    onclick: move |_| do_add(),
                                    "Add"
                                }
                            }
                            button {
                                style: "{BTN_CANCEL}",
                                disabled: *is_adding.read(),
                                onclick: move |_| {
                                    show_add.set(false);
                                    add_error.set(None);
                                    // WHY(#4877): reset provider/role too, not
                                    // just the key -- otherwise a stale
                                    // provider/role from a cancelled add
                                    // reappears the next time the form opens.
                                    add_provider.set(String::new());
                                    add_role.set(CredentialRole::Primary);
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
                        onclick: move |_| {
                            // WHY(#4877): reset all add-form state on open, so
                            // a value left over from a prior cancelled/failed
                            // attempt never reappears as though still current.
                            add_provider.set(String::new());
                            add_role.set(CredentialRole::Primary);
                            add_error.set(None);
                            add_key.set(koina::secret::SecretString::from(String::new()));
                            add_key_epoch.set(add_key_epoch() + 1);
                            show_add.set(true);
                        },
                        "+ Add Credential"
                    }
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
    let mut is_rotating = use_signal(|| false);
    let mut is_removing = use_signal(|| false);
    let mut confirm_rotate = use_signal(|| false);
    let mut confirm_remove = use_signal(|| false);
    let mut card_error: Signal<Option<String>> = use_signal(|| None);

    let entry_id = entry.id.clone();
    let entry_provider = entry.provider.clone();

    let mut do_validate = {
        let id = entry_id.clone();
        move || {
            // WHY(#4877): guard against a double-click submitting a second
            // validate request while the first is still in flight.
            if *is_validating.read() {
                return;
            }
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
                        let msg = describe_error_response("Validate", resp).await;
                        is_validating.set(false);
                        card_error.set(Some(msg));
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
            // WHY(#4877): the confirm banner already hides once a rotate is
            // triggered, but the underlying request could still be
            // in-flight when the (now-hidden) Confirm is clicked again via a
            // queued event -- guard on the pending flag itself, not just the
            // banner's visibility.
            if *is_rotating.read() {
                return;
            }
            let cfg = config.read().clone();
            let prov = provider.clone();
            confirm_rotate.set(false);
            card_error.set(None);
            is_rotating.set(true);

            spawn(async move {
                let client = match authenticated_client(&cfg) {
                    Ok(client) => client,
                    Err(err) => {
                        card_error.set(Some(err.to_string()));
                        is_rotating.set(false);
                        return;
                    }
                };
                let url = credential_rotate_url(&cfg.server_url, &prov);
                match client.post(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        is_rotating.set(false);
                        on_change.call(());
                    }
                    Ok(resp) => {
                        let msg = describe_error_response("Rotate", resp).await;
                        is_rotating.set(false);
                        card_error.set(Some(msg));
                    }
                    Err(e) => {
                        is_rotating.set(false);
                        card_error.set(Some(format!("Connection error: {e}")));
                    }
                }
            });
        }
    };

    let mut do_remove = {
        let id = entry_id.clone();
        move || {
            if *is_removing.read() {
                return;
            }
            let cfg = config.read().clone();
            let id_r = id.clone();
            confirm_remove.set(false);
            card_error.set(None);
            is_removing.set(true);

            spawn(async move {
                let client = match authenticated_client(&cfg) {
                    Ok(client) => client,
                    Err(err) => {
                        card_error.set(Some(err.to_string()));
                        is_removing.set(false);
                        return;
                    }
                };
                let url = credential_url(&cfg.server_url, &id_r);
                match client.delete(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        is_removing.set(false);
                        on_change.call(());
                    }
                    Ok(resp) => {
                        let msg = describe_error_response("Remove", resp).await;
                        is_removing.set(false);
                        card_error.set(Some(msg));
                    }
                    Err(e) => {
                        is_removing.set(false);
                        card_error.set(Some(format!("Connection error: {e}")));
                    }
                }
            });
        }
    };

    let validating = *is_validating.read();
    let rotating = *is_rotating.read();
    let removing = *is_removing.read();
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
                    // WHY(#4875): "Valid" alone is ambiguous -- it is the one
                    // status value local inspection and a real provider
                    // acceptance can both produce. Every other status is
                    // unambiguous evidence either way (a rejection, a known
                    // expiry, malformed content) and needs no qualifier.
                    if entry.status == ValidationStatus::Valid && !entry.provider_verified {
                        span {
                            style: "color: var(--text-muted); font-size: var(--text-xs);",
                            "(local only, not provider-verified)"
                        }
                    }
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
                        style: if rotating { "{BTN_DISABLED}" } else { "{BTN_STD}" },
                        disabled: rotating,
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
                        style: if removing { "{BTN_DISABLED}" } else { "{BTN_DANGER}" },
                        disabled: removing,
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
                    // WHY(#4877): rotate now has real in-flight state -- the
                    // banner used to hide immediately on click, so a fast
                    // second click on the (already-vanished) Confirm could
                    // still queue a duplicate request.
                    if rotating {
                        button { style: "{BTN_DISABLED}", disabled: true, "Rotating..." }
                    } else {
                        button {
                            style: "{BTN_CONFIRM}",
                            onclick: move |_| do_rotate(),
                            "Confirm"
                        }
                    }
                    button {
                        style: "{BTN_CANCEL}",
                        disabled: rotating,
                        onclick: move |_| confirm_rotate.set(false),
                        "Cancel"
                    }
                }
            }

            if show_remove {
                div {
                    style: "{CONFIRM_BANNER}",
                    span { style: "{WARN_TEXT}", "Permanently remove this credential?" }
                    if removing {
                        button { style: "{BTN_DISABLED}", disabled: true, "Removing..." }
                    } else {
                        button {
                            style: "{BTN_CONFIRM}",
                            onclick: move |_| do_remove(),
                            "Remove"
                        }
                    }
                    button {
                        style: "{BTN_CANCEL}",
                        disabled: removing,
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
            provider_verified: false,
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
