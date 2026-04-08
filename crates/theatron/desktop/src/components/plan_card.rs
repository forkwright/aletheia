//! Plan card component for execution view.

use dioxus::prelude::*;

use crate::state::execution::{ExecutionPlan, StepStatus};

const CARD_STYLE: &str = "\
    flex: 1; \
    min-width: 260px; \
    max-width: 420px; \
    background: #0f0f1a; \
    border: 1px solid #2a2a3a; \
    border-radius: 6px; \
    padding: 12px 14px;\
";

const CARD_HEADER: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: 8px;\
";

const PLAN_TITLE: &str = "\
    font-size: 13px; \
    font-weight: 600; \
    color: #e0e0e0;\
";

const AGENT_BADGE: &str = "\
    font-size: 11px; \
    color: #888; \
    display: flex; \
    align-items: center; \
    gap: 4px;\
";

const PROGRESS_TEXT: &str = "\
    font-size: 11px; \
    color: #888; \
    margin-bottom: 6px;\
";

const STEP_ROW: &str = "\
    display: flex; \
    align-items: flex-start; \
    gap: 6px; \
    padding: 3px 0; \
    font-size: 12px;\
";

const STEP_ICON_STYLE: &str = "\
    flex-shrink: 0; \
    width: 16px; \
    text-align: center; \
    font-size: 11px;\
";

const STEP_DESC: &str = "color: #c0c0e0;";

const DETAIL_BOX: &str = "\
    margin-top: 4px; \
    padding: 6px 8px; \
    background: #151525; \
    border: 1px solid #2a2a3a; \
    border-radius: 4px; \
    font-size: 11px;\
";

const TIME_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: 12px; \
    margin-top: 8px; \
    font-size: 11px; \
    color: #666;\
";

const EXPAND_BTN: &str = "\
    background: transparent; \
    border: none; \
    color: #4a9aff; \
    font-size: 11px; \
    cursor: pointer; \
    padding: 0;\
";

/// Plan card for an execution plan within a wave.
///
/// Shows title, agent, step list with status indicators, and an expandable detail view.
#[component]
pub(crate) fn PlanCard(plan: ExecutionPlan) -> Element {
    let mut expanded = use_signal(|| false);
    let progress = plan.progress_pct();
    let completed = plan.completed_steps();
    let total = plan.steps.len();
    let has_failure = plan.has_failure();

    let progress_label = if has_failure {
        format!("Step {completed} of {total} (has failures)")
    } else {
        format!("Step {completed} of {total}")
    };

    rsx! {
        div {
            style: "{CARD_STYLE}",

            // Header: title + agent
            div {
                style: "{CARD_HEADER}",
                span { style: "{PLAN_TITLE}", "{plan.title}" }
                if !plan.agent_name.is_empty() {
                    span {
                        style: "{AGENT_BADGE}",
                        span {
                            style: "width: 6px; height: 6px; border-radius: 50%; background: {agent_status_color(&plan.agent_status)};",
                        }
                        "{plan.agent_name}"
                    }
                }
            }

            // Progress
            div {
                style: "{PROGRESS_TEXT}",
                "{progress_label} — {progress}%"
            }

            // Step list
            for step in &plan.steps {
                div {
                    key: "{step.id}",
                    style: "{STEP_ROW}",
                    span {
                        style: "{STEP_ICON_STYLE} color: {step_color(step.status)};",
                        "{step_icon(step.status)}"
                    }
                    div {
                        style: "flex: 1;",
                        span { style: "{STEP_DESC}", "{step.description}" }

                        // Expanded detail
                        if *expanded.read() {
                            if let Some(ref output) = step.output {
                                div {
                                    style: "{DETAIL_BOX} color: #aaa;",
                                    "{output}"
                                }
                            }
                            if let Some(ref error) = step.error {
                                div {
                                    style: "{DETAIL_BOX} color: #ef4444;",
                                    "{error}"
                                }
                            }
                            if let Some(dur) = step.duration_secs {
                                span {
                                    style: "font-size: 10px; color: #555; margin-left: 4px;",
                                    "({dur:.1}s)"
                                }
                            }
                        }
                    }
                }
            }

            // Expand/collapse toggle
            if !plan.steps.is_empty() {
                button {
                    style: "{EXPAND_BTN}",
                    onclick: move |_| {
                        let current = *expanded.read();
                        expanded.set(!current);
                    },
                    if *expanded.read() { "Hide details" } else { "Show details" }
                }
            }

            // Elapsed / remaining time
            if plan.elapsed_secs.is_some() || plan.estimated_remaining_secs.is_some() {
                div {
                    style: "{TIME_ROW}",
                    if let Some(elapsed) = plan.elapsed_secs {
                        span { "{format_duration(elapsed)} elapsed" }
                    }
                    if let Some(remaining) = plan.estimated_remaining_secs {
                        span { "~{format_duration(remaining)} remaining" }
                    }
                }
            }
        }
    }
}

#[must_use]
pub(crate) fn step_icon(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "( )",
        StepStatus::Running => "(>)",
        StepStatus::Complete => "(v)",
        StepStatus::Failed => "(x)",
        StepStatus::Skipped => "(-)",
    }
}

#[must_use]
pub(crate) fn step_color(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "#666",
        StepStatus::Running => "#4a9aff",
        StepStatus::Complete => "#22c55e",
        StepStatus::Failed => "#ef4444",
        StepStatus::Skipped => "#888",
    }
}

fn agent_status_color(status: &str) -> &'static str {
    match status {
        "active" | "running" => "#22c55e",
        "idle" | "waiting" => "#f59e0b",
        "error" | "failed" => "#ef4444",
        _ => "#666",
    }
}

fn format_duration(secs: f64) -> String {
    if secs < 60.0 {
        format!("{secs:.0}s")
    } else if secs < 3600.0 {
        let mins = secs / 60.0;
        format!("{mins:.1}m")
    } else {
        let hours = secs / 3600.0;
        format!("{hours:.1}h")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_icons_are_distinct() {
        let icons: Vec<_> = [
            StepStatus::Pending,
            StepStatus::Running,
            StepStatus::Complete,
            StepStatus::Failed,
            StepStatus::Skipped,
        ]
        .iter()
        .map(|s| step_icon(*s))
        .collect();
        let unique: std::collections::HashSet<_> = icons.iter().collect();
        assert_eq!(unique.len(), icons.len(), "all step icons must be distinct");
    }

    #[test]
    fn step_colors_are_distinct() {
        let colors: Vec<_> = [
            StepStatus::Pending,
            StepStatus::Running,
            StepStatus::Complete,
            StepStatus::Failed,
            StepStatus::Skipped,
        ]
        .iter()
        .map(|s| step_color(*s))
        .collect();
        let unique: std::collections::HashSet<_> = colors.iter().collect();
        assert_eq!(
            unique.len(),
            colors.len(),
            "all step colors must be distinct"
        );
    }

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(45.0), "45s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(150.0), "2.5m");
    }

    #[test]
    fn format_duration_hours() {
        assert_eq!(format_duration(7200.0), "2.0h");
    }
}
