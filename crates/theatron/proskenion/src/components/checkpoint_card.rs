//! Read-only checkpoint gate card: status, context, requirements, and artifacts.

use dioxus::prelude::*;

use crate::components::badge::status_badge_style as badge_style;
use crate::state::checkpoints::{Checkpoint, CheckpointAction, CheckpointStatus};

// WHY(#7224): approve/skip/override used to POST to pylon via
// `project_checkpoint_action_url`, but that route has never had a pylon
// handler and dianoia has no persisted, mutable checkpoint entity to back
// one -- see the same-numbered note on `skene::api::routes::planning`. The
// action buttons were deleted; this card is now a read-only checkpoint
// display. `CheckpointAction` itself is kept: `CheckpointDecision::action`
// still deserializes a checkpoint's historical decision for display.

const CARD_BASE: &str = "\
    border-radius: var(--radius-md); \
    border: 1px solid; \
    padding: var(--space-4) var(--space-5); \
    margin-bottom: var(--space-3);\
";

const HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: var(--space-2);\
";

const TITLE_STYLE: &str = "\
    font-size: var(--text-md); \
    font-weight: var(--weight-semibold); \
    color: var(--text-primary);\
";

const DESCRIPTION_STYLE: &str = "\
    color: var(--text-secondary); \
    font-size: var(--text-sm); \
    margin-bottom: var(--space-3);\
";

const SECTION_LABEL: &str = "\
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    color: var(--text-muted); \
    text-transform: uppercase; \
    letter-spacing: 0.5px; \
    margin: var(--space-3) 0 var(--space-1);\
";

const CONTEXT_STYLE: &str = "\
    font-size: var(--text-sm); \
    color: var(--text-primary); \
    background: var(--bg-surface-dim); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-sm); \
    padding: var(--space-2) var(--space-3); \
    margin-bottom: var(--space-1);\
";

const REQ_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-1) 0; \
    font-size: var(--text-sm);\
";

const ARTIFACT_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-1) 0; \
    font-size: var(--text-xs);\
";

const DECISION_BOX: &str = "\
    margin-top: var(--space-3); \
    padding: var(--space-2) var(--space-3); \
    background: var(--bg-surface-dim); \
    border-radius: var(--radius-sm); \
    border: 1px solid var(--border);\
";

/// Read-only checkpoint card: gate context, requirements, artifacts, and
/// the recorded decision (if any). No approve/skip/override actions --
/// pylon has never implemented the checkpoint-action endpoint.
#[component]
pub(crate) fn CheckpointCard(checkpoint: Checkpoint) -> Element {
    let card_style = card_container_style(checkpoint.status);
    let badge_style = checkpoint_badge_style(checkpoint.status);
    let badge_label = status_label(checkpoint.status);

    rsx! {
        div {
            style: "{card_style}",
            role: "group",
            "aria-label": "{checkpoint.title}",

            div {
                style: "{HEADER_ROW}",
                span { style: "{TITLE_STYLE}", "{checkpoint.title}" }
                span { style: "{badge_style}", "{badge_label}" }
            }

            if !checkpoint.description.is_empty() {
                div { style: "{DESCRIPTION_STYLE}", "{checkpoint.description}" }
            }

            if !checkpoint.context.is_empty() {
                div { style: "{SECTION_LABEL}", "Context" }
                div { style: "{CONTEXT_STYLE}", "{checkpoint.context}" }
            }

            if !checkpoint.requirements.is_empty() {
                div { style: "{SECTION_LABEL}", "Requirements" }
                for req in &checkpoint.requirements {
                    div {
                        key: "{req.id}",
                        style: "{REQ_ROW}",
                        span {
                            style: if req.met { "color: var(--status-success); width: 18px;" } else { "color: var(--status-error); width: 18px;" },
                            if req.met { "[v]" } else { "[x]" }
                        }
                        span { style: "color: var(--text-primary);", "{req.title}" }
                    }
                }
            }

            if !checkpoint.artifacts.is_empty() {
                div { style: "{SECTION_LABEL}", "Artifacts" }
                for (i, artifact) in checkpoint.artifacts.iter().enumerate() {
                    div {
                        key: "{i}",
                        style: "{ARTIFACT_ROW}",
                        span { style: "color: var(--text-muted);", "{artifact.label}:" }
                        span { style: "color: var(--text-primary); font-family: var(--font-mono);", "{artifact.value}" }
                    }
                }
            }

            if let Some(ref decision) = checkpoint.decision {
                div {
                    style: "{DECISION_BOX}",
                    div { style: "font-size: var(--text-xs); color: var(--text-secondary);",
                        "{action_label(decision.action)} by {decision.actor} at {decision.timestamp}"
                    }
                    if !decision.notes.is_empty() {
                        div { style: "font-size: var(--text-xs); color: var(--text-secondary); margin-top: var(--space-1); font-style: italic;",
                            "\"{decision.notes}\""
                        }
                    }
                }
            }
        }
    }
}

fn card_container_style(status: CheckpointStatus) -> String {
    let (bg, border) = match status {
        CheckpointStatus::Pending => ("var(--bg-surface)", "var(--accent)"),
        CheckpointStatus::Approved => ("var(--status-success-bg)", "var(--status-success)"),
        CheckpointStatus::Skipped => ("var(--status-warning-bg)", "var(--status-warning)"),
        CheckpointStatus::Overridden => ("var(--status-error-bg)", "var(--status-error)"),
    };
    format!("{CARD_BASE} background: {bg}; border-color: {border};")
}

fn checkpoint_badge_style(status: CheckpointStatus) -> String {
    let (bg, color) = match status {
        CheckpointStatus::Pending => ("var(--status-info-bg)", "var(--status-info)"),
        CheckpointStatus::Approved => ("var(--status-success-bg)", "var(--status-success)"),
        CheckpointStatus::Skipped => ("var(--status-warning-bg)", "var(--status-warning)"),
        CheckpointStatus::Overridden => ("var(--status-error-bg)", "var(--status-error)"),
    };
    badge_style(bg, color)
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
        let pending = checkpoint_badge_style(CheckpointStatus::Pending);
        let approved = checkpoint_badge_style(CheckpointStatus::Approved);
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
}
