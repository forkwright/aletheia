//! Inline fact curation dialogs: forget, restore, adjust confidence, change
//! sensitivity. All served by the existing `knowledge/mutation.rs` routes.

use dioxus::prelude::*;

use crate::state::connection::ConnectionConfig;
use crate::views::memory::memory_client;
use crate::state::memory::FactSensitivity;

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
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-lg); \
    padding: var(--space-6); \
    min-width: 380px; \
    max-width: 560px; \
    color: var(--text-primary);\
";

const TITLE_STYLE: &str = "\
    font-size: var(--text-lg); \
    font-weight: var(--weight-bold); \
    margin-bottom: var(--space-4);\
";

const BODY_STYLE: &str = "\
    font-size: var(--text-base); \
    line-height: var(--leading-normal); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-4);\
";

const QUOTE_STYLE: &str = "\
    font-size: var(--text-sm); \
    color: var(--text-primary); \
    background: var(--bg-surface-dim); \
    border-left: 3px solid var(--accent); \
    border-radius: 0 var(--radius-md) var(--radius-md) 0; \
    padding: var(--space-2) var(--space-3); \
    margin-bottom: var(--space-4);\
";

const ACTIONS_STYLE: &str = "\
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

const SELECT_STYLE: &str = "\
    width: 100%; \
    background: var(--bg-surface-dim); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-3); \
    color: var(--text-primary); \
    font-size: var(--text-base); \
    margin-bottom: var(--space-3); \
    cursor: pointer;\
";

const RANGE_STYLE: &str = "width: 100%; margin-bottom: var(--space-3);";

/// Truncate a fact statement for use in a dialog header.
fn preview(content: &str) -> String {
    const MAX: usize = 140;
    if content.chars().count() > MAX {
        let truncated: String = content.chars().take(MAX).collect();
        format!("{truncated}…")
    } else {
        content.to_string()
    }
}

/// Confirm-and-forget dialog (soft-delete; restorable).
#[component]
pub(crate) fn ForgetFactDialog(
    fact_id: String,
    content: String,
    on_close: EventHandler<()>,
    on_done: EventHandler<()>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut is_submitting = use_signal(|| false);
    let quote = preview(&content);

    rsx! {
        div {
            style: "{OVERLAY_STYLE}",
            onclick: move |_| on_close.call(()),
            div {
                style: "{DIALOG_STYLE}",
                onclick: |evt: Event<MouseData>| evt.stop_propagation(),
                div { style: "{TITLE_STYLE}", "Forget this fact?" }
                div { style: "{QUOTE_STYLE}", "{quote}" }
                div {
                    style: "{BODY_STYLE}",
                    "The agent will stop recalling this. It is soft-deleted and can be restored later."
                }
                div {
                    style: "{ACTIONS_STYLE}",
                    button {
                        style: "{BTN_CANCEL_STYLE}",
                        onclick: move |_| on_close.call(()),
                        "Cancel"
                    }
                    button {
                        style: "{BTN_DANGER_STYLE}",
                        disabled: *is_submitting.read(),
                        onclick: {
                            let id = fact_id.clone();
                            move |_| {
                                is_submitting.set(true);
                                let cfg = config.read().clone();
                                let id = id.clone();
                                spawn(async move {
                                    let Some(client) = memory_client(&cfg) else {
                                        is_submitting.set(false);
                                        return;
                                    };
                                    match client.knowledge_forget(&id).await {
                                        Ok(()) => {
                                            tracing::info!(fact_id = %id, "fact forgotten");
                                            on_done.call(());
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, "forget failed");
                                            is_submitting.set(false);
                                        }
                                    }
                                });
                            }
                        },
                        if *is_submitting.read() { "Forgetting..." } else { "Forget" }
                    }
                }
            }
        }
    }
}

/// Restore a previously forgotten fact.
#[component]
pub(crate) fn RestoreFactDialog(
    fact_id: String,
    content: String,
    on_close: EventHandler<()>,
    on_done: EventHandler<()>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut is_submitting = use_signal(|| false);
    let quote = preview(&content);

    rsx! {
        div {
            style: "{OVERLAY_STYLE}",
            onclick: move |_| on_close.call(()),
            div {
                style: "{DIALOG_STYLE}",
                onclick: |evt: Event<MouseData>| evt.stop_propagation(),
                div { style: "{TITLE_STYLE}", "Restore this fact?" }
                div { style: "{QUOTE_STYLE}", "{quote}" }
                div {
                    style: "{BODY_STYLE}",
                    "The agent will recall this fact again."
                }
                div {
                    style: "{ACTIONS_STYLE}",
                    button {
                        style: "{BTN_CANCEL_STYLE}",
                        onclick: move |_| on_close.call(()),
                        "Cancel"
                    }
                    button {
                        style: "{BTN_PRIMARY_STYLE}",
                        disabled: *is_submitting.read(),
                        onclick: {
                            let id = fact_id.clone();
                            move |_| {
                                is_submitting.set(true);
                                let cfg = config.read().clone();
                                let id = id.clone();
                                spawn(async move {
                                    let Some(client) = memory_client(&cfg) else {
                                        is_submitting.set(false);
                                        return;
                                    };
                                    match client.knowledge_restore(&id).await {
                                        Ok(()) => {
                                            tracing::info!(fact_id = %id, "fact restored");
                                            on_done.call(());
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, "restore failed");
                                            is_submitting.set(false);
                                        }
                                    }
                                });
                            }
                        },
                        if *is_submitting.read() { "Restoring..." } else { "Restore" }
                    }
                }
            }
        }
    }
}

/// Adjust a fact's confidence with a slider, then PUT it.
#[component]
pub(crate) fn AdjustConfidenceDialog(
    fact_id: String,
    content: String,
    initial: f64,
    on_close: EventHandler<()>,
    on_done: EventHandler<()>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut value = use_signal(|| initial.clamp(0.0, 1.0));
    let mut is_submitting = use_signal(|| false);
    let quote = preview(&content);
    let current = *value.read();
    #[expect(
        clippy::cast_sign_loss,
        reason = "value is clamped to 0.0–1.0, always non-negative"
    )]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "percentage 0–100 fits in the display"
    )]
    #[expect(clippy::as_conversions, reason = "percentage display only")]
    let pct = (current * 100.0) as u32;

    rsx! {
        div {
            style: "{OVERLAY_STYLE}",
            onclick: move |_| on_close.call(()),
            div {
                style: "{DIALOG_STYLE}",
                onclick: |evt: Event<MouseData>| evt.stop_propagation(),
                div { style: "{TITLE_STYLE}", "Adjust confidence" }
                div { style: "{QUOTE_STYLE}", "{quote}" }
                div {
                    style: "font-size: var(--text-base); font-weight: var(--weight-semibold); \
                            margin-bottom: var(--space-2); text-align: center;",
                    "{pct}%"
                }
                input {
                    style: "{RANGE_STYLE}",
                    r#type: "range",
                    min: "0",
                    max: "100",
                    value: "{pct}",
                    oninput: move |evt: Event<FormData>| {
                        if let Ok(p) = evt.value().parse::<f64>() {
                            value.set((p / 100.0).clamp(0.0, 1.0));
                        }
                    },
                }
                div {
                    style: "{ACTIONS_STYLE}",
                    button {
                        style: "{BTN_CANCEL_STYLE}",
                        onclick: move |_| on_close.call(()),
                        "Cancel"
                    }
                    button {
                        style: "{BTN_PRIMARY_STYLE}",
                        disabled: *is_submitting.read(),
                        onclick: {
                            let id = fact_id.clone();
                            move |_| {
                                is_submitting.set(true);
                                let cfg = config.read().clone();
                                let id = id.clone();
                                let conf = *value.read();
                                spawn(async move {
                                    let Some(client) = memory_client(&cfg) else {
                                        is_submitting.set(false);
                                        return;
                                    };
                                    match client.knowledge_update_confidence(&id, conf).await {
                                        Ok(()) => {
                                            tracing::info!(fact_id = %id, confidence = conf, "confidence updated");
                                            on_done.call(());
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, "confidence update failed");
                                            is_submitting.set(false);
                                        }
                                    }
                                });
                            }
                        },
                        if *is_submitting.read() { "Saving..." } else { "Save" }
                    }
                }
            }
        }
    }
}

/// Change a fact's data-sovereignty sensitivity, then PUT it.
#[component]
pub(crate) fn ChangeSensitivityDialog(
    fact_id: String,
    content: String,
    initial: FactSensitivity,
    on_close: EventHandler<()>,
    on_done: EventHandler<()>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut selected = use_signal(|| initial);
    let mut is_submitting = use_signal(|| false);
    let quote = preview(&content);
    let current = *selected.read();

    rsx! {
        div {
            style: "{OVERLAY_STYLE}",
            onclick: move |_| on_close.call(()),
            div {
                style: "{DIALOG_STYLE}",
                onclick: |evt: Event<MouseData>| evt.stop_propagation(),
                div { style: "{TITLE_STYLE}", "Change sensitivity" }
                div { style: "{QUOTE_STYLE}", "{quote}" }
                div {
                    style: "{BODY_STYLE}",
                    "Sensitivity gates which providers may receive this fact during recall."
                }
                select {
                    style: "{SELECT_STYLE}",
                    value: "{current.wire()}",
                    onchange: move |evt: Event<FormData>| {
                        selected.set(FactSensitivity::from_raw(&evt.value()));
                    },
                    for s in FactSensitivity::ALL {
                        option {
                            value: "{s.wire()}",
                            selected: *s == current,
                            "{s.label()}"
                        }
                    }
                }
                div {
                    style: "{ACTIONS_STYLE}",
                    button {
                        style: "{BTN_CANCEL_STYLE}",
                        onclick: move |_| on_close.call(()),
                        "Cancel"
                    }
                    button {
                        style: "{BTN_PRIMARY_STYLE}",
                        disabled: *is_submitting.read(),
                        onclick: {
                            let id = fact_id.clone();
                            move |_| {
                                is_submitting.set(true);
                                let cfg = config.read().clone();
                                let id = id.clone();
                                let sens = *selected.read();
                                spawn(async move {
                                    let Some(client) = memory_client(&cfg) else {
                                        is_submitting.set(false);
                                        return;
                                    };
                                    match client.knowledge_update_sensitivity(&id, &sens.wire()).await {
                                        Ok(()) => {
                                            tracing::info!(fact_id = %id, sensitivity = sens.wire(), "sensitivity updated");
                                            on_done.call(());
                                        }
                                        Err(e) => {
                                            tracing::warn!(error = %e, "sensitivity update failed");
                                            is_submitting.set(false);
                                        }
                                    }
                                });
                            }
                        },
                        if *is_submitting.read() { "Saving..." } else { "Save" }
                    }
                }
            }
        }
    }
}
