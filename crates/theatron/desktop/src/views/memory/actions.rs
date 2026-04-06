//! Entity action dialogs: merge, flag, and delete.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::memory::{EntityListStore, FlagSeverity};

const OVERLAY_STYLE: &str = "\
    position: fixed; \
    top: 0; left: 0; right: 0; bottom: 0; \
    background: rgba(0, 0, 0, 0.6); \
    display: flex; \
    align-items: center; \
    justify-content: center; \
    z-index: 100;\
";

const DIALOG_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 12px; \
    padding: 24px; \
    min-width: 400px; \
    max-width: 600px; \
    max-height: 80vh; \
    overflow-y: auto; \
    color: #e0e0e0;\
";

const DIALOG_TITLE_STYLE: &str = "\
    font-size: 18px; \
    font-weight: 700; \
    margin-bottom: 16px;\
";

const DIALOG_BODY_STYLE: &str = "\
    font-size: 14px; \
    line-height: 1.5; \
    color: #aaa; \
    margin-bottom: 16px;\
";

const DIALOG_ACTIONS_STYLE: &str = "\
    display: flex; \
    justify-content: flex-end; \
    gap: 8px; \
    margin-top: 16px;\
";

const BTN_CANCEL_STYLE: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 8px 16px; \
    font-size: 13px; \
    cursor: pointer;\
";

const BTN_PRIMARY_STYLE: &str = "\
    background: #3b3bbb; \
    color: #ffffff; \
    border: none; \
    border-radius: 6px; \
    padding: 8px 16px; \
    font-size: 13px; \
    cursor: pointer;\
";

const BTN_DANGER_STYLE: &str = "\
    background: #b91c1c; \
    color: #ffffff; \
    border: none; \
    border-radius: 6px; \
    padding: 8px 16px; \
    font-size: 13px; \
    cursor: pointer;\
";

const INPUT_STYLE: &str = "\
    width: 100%; \
    background: #0f0f1a; \
    border: 1px solid #333; \
    border-radius: 6px; \
    padding: 8px 12px; \
    color: #e0e0e0; \
    font-size: 14px; \
    margin-bottom: 12px;\
";

const SELECT_STYLE: &str = "\
    width: 100%; \
    background: #0f0f1a; \
    border: 1px solid #333; \
    border-radius: 6px; \
    padding: 8px 12px; \
    color: #e0e0e0; \
    font-size: 14px; \
    margin-bottom: 12px; \
    cursor: pointer;\
";

const MERGE_ENTITY_CARD: &str = "\
    background: #0f0f1a; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 12px; \
    flex: 1;\
";

const MERGE_SIDE_STYLE: &str = "\
    display: flex; \
    gap: 12px; \
    margin-bottom: 16px;\
";

const MERGE_LABEL_STYLE: &str = "\
    font-size: 11px; \
    color: #888; \
    text-transform: uppercase; \
    margin-bottom: 4px;\
";

const MERGE_NAME_STYLE: &str = "\
    font-size: 16px; \
    font-weight: 600; \
    color: #e0e0e0;\
";

const WARNING_STYLE: &str = "\
    background: #4a2a1a; \
    border: 1px solid #f59e0b44; \
    border-radius: 6px; \
    padding: 12px; \
    color: #f59e0b; \
    font-size: 13px; \
    margin-bottom: 12px;\
";

const IMPACT_STYLE: &str = "\
    background: #4a1a1a; \
    border: 1px solid #ef444444; \
    border-radius: 6px; \
    padding: 12px; \
    color: #ef4444; \
    font-size: 13px; \
    margin-bottom: 12px;\
";

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
            onclick: move |_| on_close.call(()),
            div {
                style: "{DIALOG_STYLE}",
                onclick: |evt: Event<MouseData>| evt.stop_propagation(),
                div { style: "{DIALOG_TITLE_STYLE}", "Merge Entities" }

                div {
                    style: "{WARNING_STYLE}",
                    "Merging is destructive. The secondary entity will be deleted and its properties, \
                     relationships, and memories merged into the primary entity."
                }

                // Side by side preview
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
                            div { style: "color: #555; font-size: 14px;", "Select entity below" }
                        }
                    }
                }

                // Entity selection
                div {
                    style: "{DIALOG_BODY_STYLE}",
                    "Select the entity to merge into "
                    strong { "{entity_name}" }
                    ":"
                }
                select {
                    style: "{SELECT_STYLE}",
                    value: "{selected_merge_id}",
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
                                    let client = authenticated_client(&cfg);
                                    let base = cfg.server_url.trim_end_matches('/');
                                    let url = format!("{base}/api/v1/knowledge/entities/merge");

                                    let body = serde_json::json!({
                                        "primary_id": primary,
                                        "secondary_id": secondary_id,
                                    });

                                    match client.post(&url).json(&body).send().await {
                                        Ok(resp) if resp.status().is_success() => {
                                            tracing::info!("merged entities: {primary} <- {secondary_id}");
                                            on_merged.call(());
                                        }
                                        Ok(resp) => {
                                            tracing::warn!("merge failed: {}", resp.status());
                                            is_submitting.set(false);
                                        }
                                        Err(e) => {
                                            tracing::warn!("merge error: {e}");
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
            onclick: move |_| on_close.call(()),
            div {
                style: "{DIALOG_STYLE}",
                onclick: |evt: Event<MouseData>| evt.stop_propagation(),
                div { style: "{DIALOG_TITLE_STYLE}", "Flag Entity for Review" }

                div {
                    style: "{DIALOG_BODY_STYLE}",
                    "Flag "
                    strong { "{entity_name}" }
                    " for human review."
                }

                // Severity
                label {
                    style: "font-size: 13px; color: #aaa; display: block; margin-bottom: 4px;",
                    "Severity"
                }
                select {
                    style: "{SELECT_STYLE}",
                    value: "{severity.read().label()}",
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

                // Reason
                label {
                    style: "font-size: 13px; color: #aaa; display: block; margin-bottom: 4px;",
                    "Reason"
                }
                textarea {
                    style: "{INPUT_STYLE} min-height: 80px; resize: vertical;",
                    placeholder: "Why should this entity be reviewed?",
                    value: "{reason}",
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
                                    let client = authenticated_client(&cfg);
                                    let base = cfg.server_url.trim_end_matches('/');
                                    let encoded: String =
                                        form_urlencoded::byte_serialize(id.as_bytes()).collect();
                                    let url = format!("{base}/api/v1/knowledge/entities/{encoded}/flag");

                                    let body = serde_json::json!({
                                        "reason": reason_text,
                                        "severity": sev.label().to_lowercase(),
                                    });

                                    match client.post(&url).json(&body).send().await {
                                        Ok(resp) if resp.status().is_success() => {
                                            tracing::info!("flagged entity {id}");
                                            on_flagged.call(());
                                        }
                                        Ok(resp) => {
                                            tracing::warn!("flag failed: {}", resp.status());
                                            is_submitting.set(false);
                                        }
                                        Err(e) => {
                                            tracing::warn!("flag error: {e}");
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
            onclick: move |_| on_close.call(()),
            div {
                style: "{DIALOG_STYLE}",
                onclick: |evt: Event<MouseData>| evt.stop_propagation(),
                div { style: "{DIALOG_TITLE_STYLE}", "Delete Entity" }

                div {
                    style: "{IMPACT_STYLE}",
                    div { style: "font-weight: 600; margin-bottom: 8px;", "Impact" }
                    div { "This will permanently delete " strong { "{entity_name}" } " and:" }
                    ul { style: "margin: 8px 0 0 16px; padding: 0;",
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
                                    let client = authenticated_client(&cfg);
                                    let base = cfg.server_url.trim_end_matches('/');
                                    let encoded: String =
                                        form_urlencoded::byte_serialize(id.as_bytes()).collect();
                                    let url = format!("{base}/api/v1/knowledge/entities/{encoded}");

                                    match client.delete(&url).send().await {
                                        Ok(resp) if resp.status().is_success() => {
                                            tracing::info!("deleted entity {id}");
                                            on_deleted.call(());
                                        }
                                        Ok(resp) => {
                                            tracing::warn!("delete failed: {}", resp.status());
                                            is_submitting.set(false);
                                        }
                                        Err(e) => {
                                            tracing::warn!("delete error: {e}");
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
