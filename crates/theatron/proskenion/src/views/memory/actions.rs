//! Entity action dialogs: merge, flag, and delete.

use dioxus::prelude::*;

use crate::state::connection::ConnectionConfig;
use crate::state::memory::{EntityListStore, FlagSeverity};
use crate::state::toasts::{ToastSeverity, ToastStore};

const OVERLAY_STYLE: &str = "\
    position: fixed; \
    top: 0; left: 0; right: 0; bottom: 0; \
    background: var(--bg-overlay); \
    display: flex; \
    align-items: center; \
    justify-content: center; \
    z-index: 100;\
";

const DIALOG_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-lg); \
    padding: var(--space-6); \
    min-width: 400px; \
    max-width: 600px; \
    max-height: 80vh; \
    overflow-y: auto; \
    color: var(--text-primary);\
";

const DIALOG_TITLE_STYLE: &str = "\
    font-size: var(--text-lg); \
    font-weight: var(--weight-bold); \
    margin-bottom: var(--space-4);\
";

const DIALOG_BODY_STYLE: &str = "\
    font-size: var(--text-base); \
    line-height: var(--leading-normal); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-4);\
";

const DIALOG_ACTIONS_STYLE: &str = "\
    display: flex; \
    justify-content: flex-end; \
    gap: var(--space-2); \
    margin-top: var(--space-4);\
";

const BTN_CANCEL_STYLE: &str = "\
    background: var(--border); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const BTN_PRIMARY_STYLE: &str = "\
    background: var(--accent); \
    color: var(--text-primary); \
    border: none; \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const BTN_DANGER_STYLE: &str = "\
    background: var(--status-error); \
    color: var(--text-primary); \
    border: none; \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const INPUT_STYLE: &str = "\
    width: 100%; \
    background: var(--bg-surface-dim); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-3); \
    color: var(--text-primary); \
    font-size: var(--text-base); \
    margin-bottom: var(--space-3);\
";

const SELECT_STYLE: &str = "\
    width: 100%; \
    background: var(--bg-surface-dim); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-3); \
    color: var(--text-primary); \
    font-size: var(--text-base); \
    margin-bottom: var(--space-3); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const MERGE_ENTITY_CARD: &str = "\
    background: var(--bg-surface-dim); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-3); \
    flex: 1;\
";

const MERGE_SIDE_STYLE: &str = "\
    display: flex; \
    gap: var(--space-3); \
    margin-bottom: var(--space-4);\
";

const MERGE_LABEL_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-secondary); \
    text-transform: uppercase; \
    margin-bottom: var(--space-1);\
";

const MERGE_NAME_STYLE: &str = "\
    font-size: var(--text-md); \
    font-weight: var(--weight-semibold); \
    color: var(--text-primary);\
";

const WARNING_STYLE: &str = "\
    background: var(--status-warning-bg); \
    border: 1px solid color-mix(in srgb, var(--status-warning) 35%, transparent); \
    border-radius: var(--radius-md); \
    padding: var(--space-3); \
    color: var(--status-warning); \
    font-size: var(--text-sm); \
    margin-bottom: var(--space-3);\
";

const IMPACT_STYLE: &str = "\
    background: var(--status-error-bg); \
    border: 1px solid color-mix(in srgb, var(--status-error) 35%, transparent); \
    border-radius: var(--radius-md); \
    padding: var(--space-3); \
    color: var(--status-error); \
    font-size: var(--text-sm); \
    margin-bottom: var(--space-3);\
";

/// Map the desktop's local severity enum onto skene's wire-level
/// `FlagSeverity` (mirrors `pylon::handlers::knowledge::dto::FlagSeverity`).
///
/// WHY: the two enums cannot share a `From` impl -- neither type is local
/// to this crate from the trait's perspective once the target is skene's
/// own type, so the orphan rule blocks it. A one-to-one match is the
/// smallest correct fix.
const fn to_api_severity(sev: FlagSeverity) -> skene::api::types::FlagSeverity {
    match sev {
        FlagSeverity::Low => skene::api::types::FlagSeverity::Low,
        FlagSeverity::Medium => skene::api::types::FlagSeverity::Medium,
        FlagSeverity::High => skene::api::types::FlagSeverity::High,
    }
}

/// Merge dialog: select a secondary entity to merge into the primary.
#[component]
pub(crate) fn MergeDialog(
    entity_id: String,
    entity_name: String,
    list_store: Signal<EntityListStore>,
    on_close: EventHandler<()>,
    on_merged: EventHandler<()>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut selected_merge_id = use_signal(String::new);
    let mut is_submitting = use_signal(|| false);

    let store = list_store.read();
    let candidates: Vec<(String, String)> = store
        .entities
        .iter()
        .filter(|e| e.id != entity_id)
        .map(|e| (e.id.clone(), e.name.clone()))
        .collect();
    drop(store);

    let selected_name = {
        let sel_id = selected_merge_id.read().clone();
        candidates
            .iter()
            .find(|(id, _)| id == &sel_id)
            .map(|(_, n)| n.clone())
            .unwrap_or_default()
    };

    let has_selection = !selected_merge_id.read().is_empty();

    rsx! {
        div {
            style: "{OVERLAY_STYLE}",
            "aria-label": "Merge entities backdrop",
            onclick: move |_| on_close.call(()),
            div {
                style: "{DIALOG_STYLE}",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Merge entities",
                onclick: |evt: Event<MouseData>| evt.stop_propagation(),
                div { style: "{DIALOG_TITLE_STYLE}", "Merge Entities" }

                div {
                    style: "{WARNING_STYLE}",
                    "Merging is destructive. The secondary entity will be deleted and its properties, \
                     relationships, and memories merged into the primary entity."
                }

                div {
                    style: "{MERGE_SIDE_STYLE}",
                    div {
                        style: "{MERGE_ENTITY_CARD}",
                        div { style: "{MERGE_LABEL_STYLE}", "Primary (keeps ID)" }
                        div { style: "{MERGE_NAME_STYLE}", "{entity_name}" }
                    }
                    div {
                        style: "{MERGE_ENTITY_CARD}",
                        div { style: "{MERGE_LABEL_STYLE}", "Secondary (deleted)" }
                        if has_selection {
                            div { style: "{MERGE_NAME_STYLE}", "{selected_name}" }
                        } else {
                            div { style: "color: var(--text-muted); font-size: var(--text-base);", "Select entity below" }
                        }
                    }
                }

                div {
                    style: "{DIALOG_BODY_STYLE}",
                    "Select the entity to merge into "
                    strong { "{entity_name}" }
                    ":"
                }
                select {
                    style: "{SELECT_STYLE}",
                    value: "{selected_merge_id}",
                    "aria-label": "Entity to merge into {entity_name}",
                    onchange: move |evt: Event<FormData>| {
                        selected_merge_id.set(evt.value().clone());
                    },
                    option { value: "", "Choose entity..." }
                    for (id, name) in candidates.iter() {
                        option {
                            value: "{id}",
                            "{name}"
                        }
                    }
                }

                div {
                    style: "{DIALOG_ACTIONS_STYLE}",
                    button {
                        style: "{BTN_CANCEL_STYLE}",
                        onclick: move |_| on_close.call(()),
                        "Cancel"
                    }
                    button {
                        style: "{BTN_PRIMARY_STYLE}",
                        disabled: !has_selection || *is_submitting.read(),
                        onclick: {
                            let primary_id = entity_id.clone();

                            let on_merged = on_merged;
                            move |_| {
                                let secondary_id = selected_merge_id.read().clone();
                                if secondary_id.is_empty() {
                                    return;
                                }
                                is_submitting.set(true);
                                let cfg = config.read().clone();
                                let primary = primary_id.clone();

                                spawn(async move {
                                    let client = match skene::api::client::ApiClient::new(
                                        &cfg.server_url,
                                        cfg.auth_token.clone(),
                                    ) {
                                        Ok(client) => client,
                                        Err(err) => {
                                            if let Some(mut ts) =
                                                try_consume_context::<Signal<ToastStore>>()
                                            {
                                                ts.write()
                                                    .push(ToastSeverity::Error, err.to_string());
                                            }
                                            is_submitting.set(false);
                                            return;
                                        }
                                    };

                                    match client.merge_entities(&primary, &secondary_id).await {
                                        Ok(()) => {
                                            tracing::info!("merged entities: {primary} <- {secondary_id}");
                                            if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                                                ts.write().push(ToastSeverity::Info, "Entities merged");
                                            }
                                            on_merged.call(());
                                        }
                                        Err(err) => {
                                            tracing::warn!("merge failed: {err}");
                                            if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                                                ts.write().push(ToastSeverity::Error, format!("Merge failed: {err}"));
                                            }
                                            is_submitting.set(false);
                                        }
                                    }
                                });
                            }
                        },
                        if *is_submitting.read() { "Merging..." } else { "Merge" }
                    }
                }
            }
        }
    }
}

/// Flag dialog: mark entity for human review.
#[component]
pub(crate) fn FlagDialog(
    entity_id: String,
    entity_name: String,
    on_close: EventHandler<()>,
    on_flagged: EventHandler<()>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut reason = use_signal(String::new);
    let mut severity = use_signal(|| FlagSeverity::Medium);
    let mut is_submitting = use_signal(|| false);

    rsx! {
        div {
            style: "{OVERLAY_STYLE}",
            "aria-label": "Flag entity backdrop",
            onclick: move |_| on_close.call(()),
            div {
                style: "{DIALOG_STYLE}",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Flag entity for review",
                onclick: |evt: Event<MouseData>| evt.stop_propagation(),
                div { style: "{DIALOG_TITLE_STYLE}", "Flag Entity for Review" }

                div {
                    style: "{DIALOG_BODY_STYLE}",
                    "Flag "
                    strong { "{entity_name}" }
                    " for human review."
                }

                label {
                    style: "font-size: var(--text-sm); color: var(--text-secondary); display: block; margin-bottom: var(--space-1);",
                    "Severity"
                }
                select {
                    style: "{SELECT_STYLE}",
                    value: "{severity.read().label()}",
                    "aria-label": "Severity",
                    onchange: move |evt: Event<FormData>| {
                        let label = evt.value();
                        for s in FlagSeverity::ALL {
                            if s.label() == label {
                                severity.set(*s);
                                break;
                            }
                        }
                    },
                    for s in FlagSeverity::ALL {
                        option {
                            value: "{s.label()}",
                            selected: *s == *severity.read(),
                            "{s.label()}"
                        }
                    }
                }

                label {
                    style: "font-size: var(--text-sm); color: var(--text-secondary); display: block; margin-bottom: var(--space-1);",
                    "Reason"
                }
                textarea {
                    style: "{INPUT_STYLE} min-height: 80px; resize: vertical;",
                    placeholder: "Why should this entity be reviewed?",
                    value: "{reason}",
                    "aria-label": "Reason",
                    oninput: move |evt: Event<FormData>| {
                        reason.set(evt.value().clone());
                    },
                }

                div {
                    style: "{DIALOG_ACTIONS_STYLE}",
                    button {
                        style: "{BTN_CANCEL_STYLE}",
                        onclick: move |_| on_close.call(()),
                        "Cancel"
                    }
                    button {
                        style: "{BTN_PRIMARY_STYLE}",
                        disabled: reason.read().is_empty() || *is_submitting.read(),
                        onclick: {
                            let eid = entity_id.clone();

                            let on_flagged = on_flagged;
                            move |_| {
                                let reason_text = reason.read().clone();
                                let sev = *severity.read();
                                is_submitting.set(true);
                                let cfg = config.read().clone();
                                let id = eid.clone();

                                spawn(async move {
                                    let client = match skene::api::client::ApiClient::new(
                                        &cfg.server_url,
                                        cfg.auth_token.clone(),
                                    ) {
                                        Ok(client) => client,
                                        Err(err) => {
                                            if let Some(mut ts) =
                                                try_consume_context::<Signal<ToastStore>>()
                                            {
                                                ts.write()
                                                    .push(ToastSeverity::Error, err.to_string());
                                            }
                                            is_submitting.set(false);
                                            return;
                                        }
                                    };

                                    match client
                                        .flag_entity(&id, &reason_text, to_api_severity(sev))
                                        .await
                                    {
                                        Ok(()) => {
                                            tracing::info!("flagged entity {id}");
                                            if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                                                ts.write().push(ToastSeverity::Info, "Entity flagged for review");
                                            }
                                            on_flagged.call(());
                                        }
                                        Err(err) => {
                                            tracing::warn!("flag failed: {err}");
                                            if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                                                ts.write().push(ToastSeverity::Error, format!("Flag failed: {err}"));
                                            }
                                            is_submitting.set(false);
                                        }
                                    }
                                });
                            }
                        },
                        if *is_submitting.read() { "Flagging..." } else { "Flag for Review" }
                    }
                }
            }
        }
    }
}

/// Delete dialog with impact summary and confirmation.
#[component]
pub(crate) fn DeleteDialog(
    entity_id: String,
    entity_name: String,
    relationship_count: usize,
    memory_count: usize,
    on_close: EventHandler<()>,
    on_deleted: EventHandler<()>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut is_submitting = use_signal(|| false);

    rsx! {
        div {
            style: "{OVERLAY_STYLE}",
            "aria-label": "Delete entity backdrop",
            onclick: move |_| on_close.call(()),
            div {
                style: "{DIALOG_STYLE}",
                role: "dialog",
                "aria-modal": "true",
                "aria-label": "Delete entity: {entity_name}",
                onclick: |evt: Event<MouseData>| evt.stop_propagation(),
                div { style: "{DIALOG_TITLE_STYLE}", "Delete Entity" }

                div {
                    style: "{IMPACT_STYLE}",
                    div { style: "font-weight: var(--weight-semibold); margin-bottom: var(--space-2);", "Impact" }
                    div { "This will permanently delete " strong { "{entity_name}" } " and:" }
                    ul { style: "margin: var(--space-2) 0 0 var(--space-4); padding: 0;",
                        li { "Remove {relationship_count} relationship(s)" }
                        li { "Affect {memory_count} associated memory/memories" }
                    }
                }

                div {
                    style: "{DIALOG_BODY_STYLE}",
                    "This action cannot be undone. Are you sure?"
                }

                div {
                    style: "{DIALOG_ACTIONS_STYLE}",
                    button {
                        style: "{BTN_CANCEL_STYLE}",
                        onclick: move |_| on_close.call(()),
                        "Cancel"
                    }
                    button {
                        style: "{BTN_DANGER_STYLE}",
                        disabled: *is_submitting.read(),
                        onclick: {
                            let eid = entity_id.clone();

                            let on_deleted = on_deleted;
                            move |_| {
                                is_submitting.set(true);
                                let cfg = config.read().clone();
                                let id = eid.clone();

                                spawn(async move {
                                    let client = match skene::api::client::ApiClient::new(
                                        &cfg.server_url,
                                        cfg.auth_token.clone(),
                                    ) {
                                        Ok(client) => client,
                                        Err(err) => {
                                            if let Some(mut ts) =
                                                try_consume_context::<Signal<ToastStore>>()
                                            {
                                                ts.write()
                                                    .push(ToastSeverity::Error, err.to_string());
                                            }
                                            is_submitting.set(false);
                                            return;
                                        }
                                    };

                                    match client.delete_entity(&id).await {
                                        Ok(()) => {
                                            tracing::info!("deleted entity {id}");
                                            if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                                                ts.write().push(ToastSeverity::Info, "Entity deleted");
                                            }
                                            on_deleted.call(());
                                        }
                                        Err(err) => {
                                            tracing::warn!("delete failed: {err}");
                                            if let Some(mut ts) = try_consume_context::<Signal<ToastStore>>() {
                                                ts.write().push(ToastSeverity::Error, format!("Delete failed: {err}"));
                                            }
                                            is_submitting.set(false);
                                        }
                                    }
                                });
                            }
                        },
                        if *is_submitting.read() { "Deleting..." } else { "Delete Permanently" }
                    }
                }
            }
        }
    }
}
