//! Checkpoint approval card with approve, skip, and override actions.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::checkpoints::{
    Checkpoint, CheckpointAction, CheckpointActionRequest, CheckpointStatus,
};
use crate::state::connection::ConnectionConfig;

const CARD_BASE: &str = "\
    border-radius: 8px; \
    border: 1px solid; \
    padding: 16px 20px; \
    margin-bottom: 12px;\
";

const HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: 8px;\
";

const TITLE_STYLE: &str = "\
    font-size: 15px; \
    font-weight: 600; \
    color: #e0e0e0;\
";

const BADGE_BASE: &str = "\
    display: inline-block; \
    font-size: 11px; \
    font-weight: 600; \
    padding: 2px 8px; \
    border-radius: 10px; \
    text-transform: uppercase; \
    letter-spacing: 0.4px;\
";

const DESCRIPTION_STYLE: &str = "\
    color: #aaa; \
    font-size: 13px; \
    margin-bottom: 10px;\
";

const SECTION_LABEL: &str = "\
    font-size: 11px; \
    font-weight: 600; \
    color: #666; \
    text-transform: uppercase; \
    letter-spacing: 0.5px; \
    margin: 10px 0 4px;\
";

const CONTEXT_STYLE: &str = "\
    font-size: 13px; \
    color: #c0c0e0; \
    background: #0f0f1a; \
    border: 1px solid #2a2a3a; \
    border-radius: 4px; \
    padding: 8px 10px; \
    margin-bottom: 4px;\
";

const REQ_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 3px 0; \
    font-size: 13px;\
";

const ARTIFACT_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: 6px; \
    padding: 2px 0; \
    font-size: 12px;\
";

const DECISION_BOX: &str = "\
    margin-top: 10px; \
    padding: 8px 10px; \
    background: #0f0f1a; \
    border-radius: 4px; \
    border: 1px solid #2a2a3a;\
";

const BTN_ROW: &str = "\
    display: flex; \
    gap: 8px; \
    margin-top: 14px;\
";

const APPROVE_BTN: &str = "\
    background: #166534; \
    color: #dcfce7; \
    border: 1px solid #22c55e; \
    border-radius: 6px; \
    padding: 6px 16px; \
    font-size: 13px; \
    font-weight: 600; \
    cursor: pointer;\
";

const SKIP_BTN: &str = "\
    background: #78350f; \
    color: #fef3c7; \
    border: 1px solid #f59e0b; \
    border-radius: 6px; \
    padding: 6px 16px; \
    font-size: 13px; \
    font-weight: 600; \
    cursor: pointer;\
";

const OVERRIDE_BTN: &str = "\
    background: #7f1d1d; \
    color: #fee2e2; \
    border: 1px solid #ef4444; \
    border-radius: 6px; \
    padding: 6px 16px; \
    font-size: 13px; \
    font-weight: 600; \
    cursor: pointer;\
";

const SUBMIT_BTN_ACTIVE: &str = "\
    background: #4a4aff; \
    color: white; \
    border: none; \
    border-radius: 6px; \
    padding: 6px 16px; \
    font-size: 13px; \
    font-weight: 600; \
    cursor: pointer;\
";

const SUBMIT_BTN_DISABLED: &str = "\
    background: #333; \
    color: #666; \
    border: none; \
    border-radius: 6px; \
    padding: 6px 16px; \
    font-size: 13px; \
    font-weight: 600; \
    cursor: not-allowed;\
";

const CANCEL_BTN: &str = "\
    background: transparent; \
    color: #888; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 6px 12px; \
    font-size: 13px; \
    cursor: pointer;\
";

const NOTES_TEXTAREA: &str = "\
    width: 100%; \
    background: #0f0f1a; \
    border: 1px solid #333; \
    border-radius: 4px; \
    padding: 8px 10px; \
    color: #e0e0e0; \
    font-size: 13px; \
    font-family: inherit; \
    resize: vertical; \
    min-height: 70px; \
    box-sizing: border-box;\
";

const ERROR_STYLE: &str = "color: #ef4444; font-size: 12px; margin-top: 8px;";

/// Checkpoint approval card with gate context, requirements, artifacts, and actions.
///
/// Approve, Skip, and Override are gated on the API response — no optimistic update.
/// Skip and Override require a notes entry of at least 10 characters before submit.
#[component]
pub(crate) fn CheckpointCard(
    checkpoint: Checkpoint,
    project_id: String,
    on_action_complete: EventHandler<()>,
) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut selected_action: Signal<Option<CheckpointAction>> = use_signal(|| None);
    let mut notes = use_signal(String::new);
    let mut submitting = use_signal(|| false);
    let mut error_msg: Signal<Option<String>> = use_signal(|| None);

    let is_pending = checkpoint.status == CheckpointStatus::Pending;
    let notes_valid = notes.read().len() >= 10;

    // Clones captured by the approve closure.
    let checkpoint_id_approve = checkpoint.id.clone();
    let project_id_approve = project_id.clone();

    // Clones captured by the submit-with-notes closure.
    let checkpoint_id_submit = checkpoint.id.clone();
    let project_id_submit = project_id.clone();

    let do_approve = move |_| {
        let cfg = config.read().clone();
        let cid = checkpoint_id_approve.clone();
        let pid = project_id_approve.clone();
        submitting.set(true);
        error_msg.set(None);
        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/planning/projects/{pid}/checkpoints/{cid}/action",
                cfg.server_url.trim_end_matches('/')
            );
            let req = CheckpointActionRequest {
                action: CheckpointAction::Approve,
                notes: None,
            };
            match client.post(&url).json(&req).send().await {
                Ok(resp) if resp.status().is_success() => {
                    on_action_complete.call(());
                }
                Ok(resp) => {
                    let status = resp.status();
                    error_msg.set(Some(format!("server error: {status}")));
                    submitting.set(false);
                }
                Err(e) => {
                    error_msg.set(Some(format!("connection error: {e}")));
                    submitting.set(false);
                }
            }
        });
    };

    let do_submit_notes = move |_| {
        let Some(action) = *selected_action.read() else {
            return;
        };
        if notes.read().len() < 10 {
            return;
        }
        let cfg = config.read().clone();
        let cid = checkpoint_id_submit.clone();
        let pid = project_id_submit.clone();
        let notes_val = notes.read().clone();
        submitting.set(true);
        error_msg.set(None);
        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/planning/projects/{pid}/checkpoints/{cid}/action",
                cfg.server_url.trim_end_matches('/')
            );
            let req = CheckpointActionRequest {
                action,
                notes: Some(notes_val),
            };
            match client.post(&url).json(&req).send().await {
                Ok(resp) if resp.status().is_success() => {
                    on_action_complete.call(());
                }
                Ok(resp) => {
                    let status = resp.status();
                    error_msg.set(Some(format!("server error: {status}")));
                    submitting.set(false);
                }
                Err(e) => {
                    error_msg.set(Some(format!("connection error: {e}")));
                    submitting.set(false);
                }
            }
        });
    };

    let card_style = card_container_style(checkpoint.status);
    let badge_style = status_badge_style(checkpoint.status);
    let badge_label = status_label(checkpoint.status);

    rsx! {
        div {
            style: "{card_style}",

            // Header: title + status badge
            div {
                style: "{HEADER_ROW}",
                span { style: "{TITLE_STYLE}", "{checkpoint.title}" }
                span { style: "{badge_style}", "{badge_label}" }
            }

            // Description
            if !checkpoint.description.is_empty() {
                div { style: "{DESCRIPTION_STYLE}", "{checkpoint.description}" }
            }

            // Context
            if !checkpoint.context.is_empty() {
                div { style: "{SECTION_LABEL}", "Context" }
                div { style: "{CONTEXT_STYLE}", "{checkpoint.context}" }
            }

            // Requirements
            if !checkpoint.requirements.is_empty() {
                div { style: "{SECTION_LABEL}", "Requirements" }
                for req in &checkpoint.requirements {
                    div {
                        key: "{req.id}",
                        style: "{REQ_ROW}",
                        span {
                            style: if req.met { "color: #22c55e; width: 18px;" } else { "color: #ef4444; width: 18px;" },
                            if req.met { "[v]" } else { "[x]" }
                        }
                        span { style: "color: #c0c0e0;", "{req.title}" }
                    }
                }
            }

            // Artifacts
            if !checkpoint.artifacts.is_empty() {
                div { style: "{SECTION_LABEL}", "Artifacts" }
                for (i, artifact) in checkpoint.artifacts.iter().enumerate() {
                    div {
                        key: "{i}",
                        style: "{ARTIFACT_ROW}",
                        span { style: "color: #666;", "{artifact.label}:" }
                        span { style: "color: #c0c0e0; font-family: monospace;", "{artifact.value}" }
                    }
                }
            }

            // Decision record (resolved gates)
            if let Some(ref decision) = checkpoint.decision {
                div {
                    style: "{DECISION_BOX}",
                    div { style: "font-size: 12px; color: #888;",
                        "{action_label(decision.action)} by {decision.actor} at {decision.timestamp}"
                    }
                    if !decision.notes.is_empty() {
                        div { style: "font-size: 12px; color: #aaa; margin-top: 4px; font-style: italic;",
                            "\"{decision.notes}\""
                        }
                    }
                }
            }

            // Error feedback
            if let Some(ref err) = *error_msg.read() {
                div { style: "{ERROR_STYLE}", "{err}" }
            }

            // Action area (pending gates only)
            if is_pending {
                match *selected_action.read() {
                    None => rsx! {
                        div {
                            style: "{BTN_ROW}",
                            button {
                                style: "{APPROVE_BTN}",
                                disabled: *submitting.read(),
                                onclick: do_approve,
                                "Approve"
                            }
                            button {
                                style: "{SKIP_BTN}",
                                disabled: *submitting.read(),
                                onclick: move |_| selected_action.set(Some(CheckpointAction::Skip)),
                                "Skip"
                            }
                            button {
                                style: "{OVERRIDE_BTN}",
                                disabled: *submitting.read(),
                                onclick: move |_| selected_action.set(Some(CheckpointAction::Override)),
                                "Override"
                            }
                        }
                    },
                    Some(action) => rsx! {
                        div {
                            style: "margin-top: 12px;",
                            div { style: "{SECTION_LABEL}", "{notes_action_label(action)} — reason required (min 10 chars):" }
                            textarea {
                                style: "{NOTES_TEXTAREA}",
                                placeholder: "Explain your decision...",
                                rows: "3",
                                value: "{notes.read()}",
                                oninput: move |evt: Event<FormData>| notes.set(evt.value().clone()),
                            }
                            div {
                                style: "{BTN_ROW}",
                                button {
                                    style: if notes_valid { "{SUBMIT_BTN_ACTIVE}" } else { "{SUBMIT_BTN_DISABLED}" },
                                    disabled: !notes_valid || *submitting.read(),
                                    onclick: do_submit_notes,
                                    if *submitting.read() { "Submitting..." } else { "Submit" }
                                }
                                button {
                                    style: "{CANCEL_BTN}",
                                    disabled: *submitting.read(),
                                    onclick: move |_| {
                                        selected_action.set(None);
                                        notes.set(String::new());
                                    },
                                    "Back"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn card_container_style(status: CheckpointStatus) -> String {
    let (bg, border) = match status {
        CheckpointStatus::Pending => ("#1a1a2e", "#4a4aff"),
        CheckpointStatus::Approved => ("#0f1f0f", "#22c55e"),
        CheckpointStatus::Skipped => ("#1e1a10", "#f59e0b"),
        CheckpointStatus::Overridden => ("#1e0f0f", "#ef4444"),
    };
    format!("{CARD_BASE} background: {bg}; border-color: {border};")
}

fn status_badge_style(status: CheckpointStatus) -> String {
    let (bg, color) = match status {
        CheckpointStatus::Pending => ("#1e1e5a", "#8080ff"),
        CheckpointStatus::Approved => ("#0f2a0f", "#22c55e"),
        CheckpointStatus::Skipped => ("#2a1f05", "#f59e0b"),
        CheckpointStatus::Overridden => ("#2a0f0f", "#ef4444"),
    };
    format!("{BADGE_BASE} background: {bg}; color: {color};")
}

fn status_label(status: CheckpointStatus) -> &'static str {
    match status {
        CheckpointStatus::Pending => "Pending",
        CheckpointStatus::Approved => "Approved",
        CheckpointStatus::Skipped => "Skipped",
        CheckpointStatus::Overridden => "Overridden",
    }
}

fn action_label(action: CheckpointAction) -> &'static str {
    match action {
        CheckpointAction::Approve => "Approved",
        CheckpointAction::Skip => "Skipped",
        CheckpointAction::Override => "Overridden",
    }
}

fn notes_action_label(action: CheckpointAction) -> &'static str {
    match action {
        CheckpointAction::Skip => "Skip",
        CheckpointAction::Override => "Override",
        CheckpointAction::Approve => "Approve",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_container_style_differs_by_status() {
        let pending = card_container_style(CheckpointStatus::Pending);
        let approved = card_container_style(CheckpointStatus::Approved);
        let skipped = card_container_style(CheckpointStatus::Skipped);
        let overridden = card_container_style(CheckpointStatus::Overridden);
        assert_ne!(pending, approved);
        assert_ne!(approved, skipped);
        assert_ne!(skipped, overridden);
    }

    #[test]
    fn status_badge_style_differs_by_status() {
        let pending = status_badge_style(CheckpointStatus::Pending);
        let approved = status_badge_style(CheckpointStatus::Approved);
        assert_ne!(pending, approved);
    }

    #[test]
    fn status_labels_are_distinct() {
        let labels: Vec<_> = [
            CheckpointStatus::Pending,
            CheckpointStatus::Approved,
            CheckpointStatus::Skipped,
            CheckpointStatus::Overridden,
        ]
        .iter()
        .map(|s| status_label(*s))
        .collect();
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len(), "all labels must be distinct");
    }

    #[test]
    fn notes_action_label_skip_and_override_distinct() {
        assert_ne!(
            notes_action_label(CheckpointAction::Skip),
            notes_action_label(CheckpointAction::Override)
        );
    }
}
