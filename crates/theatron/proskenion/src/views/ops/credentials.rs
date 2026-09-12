//! Credential management panel: display, validate, rotate, add, and remove credentials.

use dioxus::prelude::*;

use crate::state::connection::ConnectionConfig;
use crate::state::credentials::{
    CredentialEntry, CredentialRole, CredentialStore, ValidationStatus, can_manage_credentials,
    canonicalize_masked_key, decode_role_claim,
};
use crate::state::fetch::FetchState;

// ── API types ──
//
// WHY(#4565, ruling B / aletheia#7187): this view previously built its own
// request/response DTOs and issued raw `authenticated_client()` requests
// against hand-built `/api/v1/system/credentials...` routes -- a second,
// untyped protocol boundary alongside skene's. `skene::api::client::ApiClient`
// now wraps all five credential routes (#4565), so every call site below
// goes through it instead; the wire types come from `skene::api::types`
// rather than being re-declared here.

/// Convert skene's secret-safe wire type into local UI state.
///
/// WHY: `canonicalize_masked_key` still runs on the server's `masked_key`
/// value as defense in depth -- a malformed or unexpectedly-raw preview
/// must never reach rendered state unmasked, regardless of which layer
/// deserialized it.
fn into_credential_entry(resp: skene::api::types::CredentialResponse) -> CredentialEntry {
    let role = if resp.role == "primary" {
        CredentialRole::Primary
    } else {
        CredentialRole::Backup
    };
    let status = ValidationStatus::from_wire(&resp.status);
    let masked_key = canonicalize_masked_key(&resp.redacted_preview);
    let (requests_today, tokens_today) = resp.usage_counters.as_ref().map_or((0, 0), |counters| {
        (counters.requests_today, counters.tokens_today)
    });
    CredentialEntry {
        id: resp.id,
        provider: resp.provider,
        role,
        masked_key,
        status,
        provider_verified: resp.provider_verified,
        last_validated: resp.last_validated,
        requests_today,
        tokens_today,
    }
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
            let client =
                match skene::api::client::ApiClient::new(&cfg.server_url, cfg.auth_token.clone()) {
                    Ok(client) => client,
                    Err(err) => {
                        fetch_state.set(FetchState::Error(err.to_string()));
                        return;
                    }
                };
            match client.list_credentials().await {
                Ok(data) => {
                    let entries = data
                        .credentials
                        .into_iter()
                        .map(into_credential_entry)
                        .collect();
                    fetch_state.set(FetchState::Loaded(CredentialStore { entries }));
                }
                Err(err) => {
                    fetch_state.set(FetchState::Error(err.to_string()));
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
        // WHY: the raw key is handed straight to
        // `skene::api::client::ApiClient::add_credential`, which builds and
        // serializes `skene::api::types::AddCredentialRequest` itself (its
        // `serialize_key` override is what puts the real secret on the wire
        // instead of `SecretString`'s default `"[REDACTED]"`). This view
        // never constructs that request type or re-serializes the secret.
        let key = {
            let key = add_key.read();
            koina::secret::SecretString::from(key.expose_secret().trim().to_owned())
        };
        let cfg = config.read().clone();

        // WHY: Clear key immediately before spawning so the raw value does not
        // linger in reactive state after the async task begins.
        add_key.set(koina::secret::SecretString::from(String::new()));
        add_key_epoch.set(add_key_epoch() + 1);
        is_adding.set(true);

        spawn(async move {
            let client =
                match skene::api::client::ApiClient::new(&cfg.server_url, cfg.auth_token.clone()) {
                    Ok(client) => client,
                    Err(err) => {
                        add_error.set(Some(err.to_string()));
                        is_adding.set(false);
                        return;
                    }
                };
            match client.add_credential(&provider, key, &role_str).await {
                Ok(_) => {
                    add_provider.set(String::new());
                    add_role.set(CredentialRole::Primary);
                    show_add.set(false);
                    is_adding.set(false);
                    fetch_trigger.set(fetch_trigger() + 1);
                }
                Err(err) => {
                    add_error.set(Some(err.to_string()));
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
            role: "region",
            "aria-label": "Credentials",

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
                                    "aria-label": "Provider",
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
                                    "aria-label": "API Key",
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
                                    "aria-label": "Role",
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
                        "aria-expanded": "false",
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
                let client = match skene::api::client::ApiClient::new(
                    &cfg.server_url,
                    cfg.auth_token.clone(),
                ) {
                    Ok(client) => client,
                    Err(err) => {
                        is_validating.set(false);
                        card_error.set(Some(err.to_string()));
                        return;
                    }
                };
                match client.validate_credential(&id_v).await {
                    Ok(_) => {
                        is_validating.set(false);
                        on_change.call(());
                    }
                    Err(err) => {
                        is_validating.set(false);
                        card_error.set(Some(err.to_string()));
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
                let client = match skene::api::client::ApiClient::new(
                    &cfg.server_url,
                    cfg.auth_token.clone(),
                ) {
                    Ok(client) => client,
                    Err(err) => {
                        card_error.set(Some(err.to_string()));
                        is_rotating.set(false);
                        return;
                    }
                };
                match client.rotate_credentials(&prov).await {
                    Ok(_) => {
                        is_rotating.set(false);
                        on_change.call(());
                    }
                    Err(err) => {
                        is_rotating.set(false);
                        card_error.set(Some(err.to_string()));
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
                let client = match skene::api::client::ApiClient::new(
                    &cfg.server_url,
                    cfg.auth_token.clone(),
                ) {
                    Ok(client) => client,
                    Err(err) => {
                        card_error.set(Some(err.to_string()));
                        is_removing.set(false);
                        return;
                    }
                };
                match client.remove_credential(&id_r).await {
                    Ok(_) => {
                        is_removing.set(false);
                        on_change.call(());
                    }
                    Err(err) => {
                        is_removing.set(false);
                        card_error.set(Some(err.to_string()));
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
            role: "group",
            "aria-label": "{entry.provider} credential ({entry.role.label()})",

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
                        "aria-label": "Validate {entry_provider} credential",
                        onclick: move |_| do_validate(),
                        "Validate"
                    }
                }

                if can_rotate {
                    button {
                        style: if rotating { "{BTN_DISABLED}" } else { "{BTN_STD}" },
                        disabled: rotating,
                        "aria-label": "Rotate {entry_provider} credential",
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
                        "aria-label": "Remove {entry_provider} credential (disabled: last primary)",
                        "Remove"
                    }
                } else {
                    button {
                        style: if removing { "{BTN_DISABLED}" } else { "{BTN_DANGER}" },
                        disabled: removing,
                        "aria-label": "Remove {entry_provider} credential",
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
                    role: "alert",
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
                    role: "alert",
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

    // WHY: route-path encoding (including the `open ai/a?b#c:100%` rotate
    // case) is now covered once, in skene, by
    // `system::credential_routes_encode_path_and_query_inputs` -- this view
    // no longer builds those paths itself, so re-asserting them here would
    // just be a second copy of skene's own contract test.

    fn api_entry(masked_key: &str) -> skene::api::types::CredentialResponse {
        skene::api::types::CredentialResponse {
            id: "anthropic:primary".to_string(),
            provider: "anthropic".to_string(),
            role: "primary".to_string(),
            redacted_preview: masked_key.to_string(),
            status: "valid".to_string(),
            provider_verified: false,
            validation_state: None,
            last_validated: None,
            usage_counters_available: false,
            usage_counters: None,
            runtime_effect: None,
        }
    }

    #[test]
    fn api_entry_canonicalizes_malformed_prefixed_mask() {
        let entry = into_credential_entry(api_entry("...raw-secret-material"));

        assert_eq!(entry.masked_key, "...????");
        assert!(!entry.masked_key.contains("raw"));
        assert!(!entry.masked_key.contains("material"));
    }

    #[test]
    fn api_entry_masks_unprefixed_raw_key() {
        let entry = into_credential_entry(api_entry("sk-test-secret-1234"));

        assert_eq!(entry.masked_key, "...1234");
        assert!(!entry.masked_key.contains("test-secret"));
    }

    #[test]
    fn api_entry_carries_usage_counters_when_present() {
        let mut entry = api_entry("sk-test-secret-1234");
        entry.usage_counters_available = true;
        entry.usage_counters = Some(skene::api::types::CredentialUsageCounters {
            requests_today: 42,
            tokens_today: 1_337,
            source: "provider".to_string(),
            freshness: "live".to_string(),
            scope: "credential".to_string(),
            state: "ok".to_string(),
        });

        let converted = into_credential_entry(entry);

        assert_eq!(converted.requests_today, 42);
        assert_eq!(converted.tokens_today, 1_337);
    }
}
