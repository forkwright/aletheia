//! Inline planning card component with step progress visualization.

use dioxus::prelude::*;

use crate::state::tools::{PlanCardState, PlanStatus, StepStatus};

const CARD_STYLE: &str = "\
    background: #1a1a30; \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-3); \
    margin-top: var(--space-1); \
    font-size: var(--text-sm);\
";

const CARD_COMPLETE_STYLE: &str = "\
    background: #1a2a1a; \
    border: 1px solid #2a4a2a; \
    border-radius: var(--radius-md); \
    padding: var(--space-3); \
    margin-top: var(--space-1); \
    font-size: var(--text-sm);\
";

const TITLE_STYLE: &str = "\
    font-weight: var(--weight-semibold); \
    color: #c0c0e0; \
    font-size: var(--text-base); \
    margin-bottom: var(--space-2);\
";

const STEP_LIST_STYLE: &str = "\
    list-style: none; \
    padding: 0; \
    margin: 0;\
";

const STEP_ITEM_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-1) 0; \
    color: var(--text-secondary); \
    font-size: var(--text-sm);\
";

const PROGRESS_BAR_OUTER: &str = "\
    height: 6px; \
    background: var(--border); \
    border-radius: var(--radius-sm); \
    margin-top: var(--space-3); \
    overflow: hidden;\
";

const PROGRESS_BAR_INNER: &str = "\
    height: 100%; \
    background: var(--accent); \
    border-radius: var(--radius-sm); \
    transition: width var(--transition-measured);\
";

const PROGRESS_BAR_COMPLETE: &str = "\
    height: 100%; \
    background: var(--status-success); \
    border-radius: var(--radius-sm);\
";

const STATUS_LABEL_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--text-muted); \
    margin-top: var(--space-2); \
    text-align: right;\
";

const STEP_ICON_PENDING: &str = "\u{25CB}"; // ○
const STEP_ICON_COMPLETE: &str = "\u{2713}"; // ✓
const STEP_ICON_FAILED: &str = "\u{2717}"; // ✗
const STEP_ICON_IN_PROGRESS: &str = "\u{25CF}"; // ●

/// Inline planning card with step list and progress bar.
#[component]
pub(crate) fn PlanningCard(plan: PlanCardState) -> Element {
    let completed = plan.completed_count();
    let total = plan.total_steps();
    let is_finished = plan.is_finished();

    let card_style = if is_finished {
        CARD_COMPLETE_STYLE
    } else {
        CARD_STYLE
    };
    let progress_pct = if total > 0 {
        // SAFETY(numeric): step counts are small; truncation is acceptable.
        #[expect(
            clippy::cast_precision_loss,
            reason = "step counts are small enough that f64 is exact"
        )]
        #[expect(clippy::as_conversions, reason = "small step counts for progress display")]
        let pct = (completed as f64 / total as f64) * 100.0;
        pct
    } else {
        0.0
    };
    let bar_inner = if is_finished {
        PROGRESS_BAR_COMPLETE
    } else {
        PROGRESS_BAR_INNER
    };

    let status_text = match &plan.status {
        PlanStatus::Proposed => "Proposed".to_string(),
        PlanStatus::InProgress => format!("{completed}/{total} steps complete"),
        PlanStatus::Complete { status } => format!("Complete: {status}"),
    };

    rsx! {
        div {
            style: "{card_style}",
            div { style: "{TITLE_STYLE}", "Plan" }
            ul {
                style: "{STEP_LIST_STYLE}",
                for step in &plan.steps {
                    li {
                        key: "{step.id}",
                        style: "{STEP_ITEM_STYLE}",
                        {render_step_icon(step.status)}
                        span {
                            style: step_label_style(step.status),
                            "{step.label}"
                        }
                    }
                }
            }
            div {
                style: "{PROGRESS_BAR_OUTER}",
                div {
                    style: "{bar_inner} width: {progress_pct:.0}%;",
                }
            }
            div { style: "{STATUS_LABEL_STYLE}", "{status_text}" }
        }
    }
}

/// Render the icon for a step status.
fn render_step_icon(status: StepStatus) -> Element {
    let (icon, color) = match status {
        StepStatus::Pending => (STEP_ICON_PENDING, "var(--text-muted)"),
        StepStatus::InProgress => (STEP_ICON_IN_PROGRESS, "var(--accent)"),
        StepStatus::Complete => (STEP_ICON_COMPLETE, "var(--status-success)"),
        StepStatus::Failed => (STEP_ICON_FAILED, "var(--status-error)"),
    };
    let style = format!("color: {color}; font-size: var(--text-base);");
    rsx! { span { style: "{style}", "{icon}" } }
}

/// Inline style for a step label based on its status.
fn step_label_style(status: StepStatus) -> String {
    match status {
        StepStatus::Pending => "color: var(--text-muted);".to_string(),
        StepStatus::InProgress => "color: #c0c0e0; font-weight: var(--weight-medium);".to_string(),
        StepStatus::Complete => "color: var(--text-secondary); text-decoration: line-through;".to_string(),
        StepStatus::Failed => "color: #f87171;".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use crate::state::tools::PlanStepState;

    use super::*;

    #[test]
    fn step_label_style_varies_by_status() {
        let pending = step_label_style(StepStatus::Pending);
        let active = step_label_style(StepStatus::InProgress);
        let done = step_label_style(StepStatus::Complete);
        let fail = step_label_style(StepStatus::Failed);
        assert_ne!(
            pending, active,
            "pending and in-progress styles must differ"
        );
        assert_ne!(done, fail, "complete and failed styles must differ");
    }

    #[test]
    fn complete_step_has_line_through() {
        let style = step_label_style(StepStatus::Complete);
        assert!(
            style.contains("line-through"),
            "completed steps should have strikethrough"
        );
    }

    #[test]
    fn plan_progress_text_for_in_progress() {
        let plan = PlanCardState {
            plan_id: "p1".into(),
            steps: vec![
                PlanStepState {
                    id: 0,
                    label: "a".to_string(),
                    status: StepStatus::Complete,
                    result: None,
                },
                PlanStepState {
                    id: 1,
                    label: "b".to_string(),
                    status: StepStatus::InProgress,
                    result: None,
                },
            ],
            status: PlanStatus::InProgress,
        };
        assert_eq!(plan.completed_count(), 1);
        assert_eq!(plan.total_steps(), 2);
    }
}
