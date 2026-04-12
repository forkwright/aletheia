//! Gap analysis panel: requirements not yet verified, filterable by priority.

use dioxus::prelude::*;

use crate::state::verification::{
    RequirementPriority, RequirementVerification, VerificationStatus,
};

const PANEL_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: var(--space-2);\
";

const FILTER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    margin-bottom: var(--space-2);\
";

const FILTER_BTN_ACTIVE: &str = "\
    background: var(--bg-surface-dim); \
    color: var(--accent); \
    border: 1px solid var(--accent); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const FILTER_BTN_INACTIVE: &str = "\
    background: transparent; \
    color: var(--text-secondary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const GAP_CARD: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-3) 14px;\
";

const GAP_TITLE_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    margin-bottom: var(--space-2);\
";

const GAP_TITLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-semibold); \
    color: var(--text-primary);\
";

const GAP_LABEL: &str = "\
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    padding: 1px 6px; \
    border-radius: var(--radius-sm); \
    text-transform: uppercase;\
";

const CRITERIA_STYLE: &str = "\
    font-size: var(--text-sm); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-1);\
";

const ACTION_STYLE: &str = "\
    font-size: var(--text-xs); \
    color: var(--accent); \
    background: var(--bg-surface-dim); \
    border-radius: var(--radius-sm); \
    padding: var(--space-1) var(--space-2); \
    margin-top: var(--space-1);\
";

const EMPTY_STYLE: &str = "\
    color: var(--text-muted); \
    font-size: var(--text-sm); \
    padding: var(--space-4) 0;\
";

/// Gap analysis panel listing requirements that are not yet fully verified.
///
/// Shows missing criteria and suggested actions. P0-only filter surfaces blocking gaps.
#[component]
pub(crate) fn GapAnalysisPanel(requirements: Vec<RequirementVerification>) -> Element {
    let mut show_blocking_only = use_signal(|| false);

    let gaps: Vec<&RequirementVerification> = requirements
        .iter()
        .filter(|r| {
            matches!(
                r.status,
                VerificationStatus::Unverified
                    | VerificationStatus::PartiallyVerified
                    | VerificationStatus::Failed
            )
        })
        .collect();

    let displayed: Vec<&RequirementVerification> = if *show_blocking_only.read() {
        gaps.iter()
            .copied()
            .filter(|r| r.priority == RequirementPriority::P0)
            .collect()
    } else {
        gaps.clone()
    };

    let blocking_count = gaps
        .iter()
        .filter(|r| r.priority == RequirementPriority::P0)
        .count();

    rsx! {
        div {
            style: "{PANEL_STYLE}",
            div {
                style: "{FILTER_ROW}",
                span { style: "font-size: var(--text-xs); color: var(--text-muted);", "Filter:" }
                button {
                    style: if !*show_blocking_only.read() { "{FILTER_BTN_ACTIVE}" } else { "{FILTER_BTN_INACTIVE}" },
                    onclick: move |_| show_blocking_only.set(false),
                    "All ({gaps.len()})"
                }
                button {
                    style: if *show_blocking_only.read() { "{FILTER_BTN_ACTIVE}" } else { "{FILTER_BTN_INACTIVE}" },
                    onclick: move |_| show_blocking_only.set(true),
                    "Blocking P0 ({blocking_count})"
                }
            }

            if displayed.is_empty() {
                div { style: "{EMPTY_STYLE}",
                    if *show_blocking_only.read() {
                        "No blocking (P0) gaps."
                    } else {
                        "No gaps found — all requirements verified."
                    }
                }
            } else {
                for req in displayed {
                    {
                        let priority_style = priority_badge_style(req.priority);
                        let priority_label = priority_label(req.priority);
                        let status_color = status_color(req.status);
                        rsx! {
                            div {
                                key: "{req.id}",
                                style: "{GAP_CARD}",
                                div {
                                    style: "{GAP_TITLE_ROW}",
                                    span { style: "{GAP_TITLE}", "{req.title}" }
                                    span { style: "{GAP_LABEL} {priority_style}", "{priority_label}" }
                                    span {
                                        style: "font-size: var(--text-xs); color: {status_color};",
                                        "{status_label(req.status)}"
                                    }
                                }
                                for (i, gap) in req.gaps.iter().enumerate() {
                                    div {
                                        key: "{i}",
                                        div { style: "{CRITERIA_STYLE}", "Missing: {gap.missing_criteria}" }
                                        div { style: "{ACTION_STYLE}", "Suggested: {gap.suggested_action}" }
                                    }
                                }
                                if req.gaps.is_empty() {
                                    div { style: "{CRITERIA_STYLE}",
                                        "No detailed gap information available."
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn priority_badge_style(priority: RequirementPriority) -> &'static str {
    match priority {
        RequirementPriority::P0 => "background: #3a0f0f; color: var(--status-error);",
        RequirementPriority::P1 => "background: #2a1f05; color: var(--status-warning);",
        RequirementPriority::P2 => "background: var(--bg-surface-dim); color: var(--accent);",
        RequirementPriority::P3 => "background: var(--bg-surface); color: var(--text-muted);",
    }
}

fn priority_label(priority: RequirementPriority) -> &'static str {
    match priority {
        RequirementPriority::P0 => "P0",
        RequirementPriority::P1 => "P1",
        RequirementPriority::P2 => "P2",
        RequirementPriority::P3 => "P3",
    }
}

fn status_label(status: VerificationStatus) -> &'static str {
    match status {
        VerificationStatus::Verified => "Verified",
        VerificationStatus::PartiallyVerified => "Partial",
        VerificationStatus::Unverified => "Unverified",
        VerificationStatus::Failed => "Failed",
    }
}

fn status_color(status: VerificationStatus) -> &'static str {
    match status {
        VerificationStatus::Verified => "var(--status-success)",
        VerificationStatus::PartiallyVerified => "var(--status-warning)",
        VerificationStatus::Unverified => "var(--text-secondary)",
        VerificationStatus::Failed => "var(--status-error)",
    }
}
