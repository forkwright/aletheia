//! Gap analysis panel: requirements not yet verified, filterable by priority.

use dioxus::prelude::*;

use crate::state::verification::{
    RequirementPriority, RequirementVerification, VerificationStatus,
};

const PANEL_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    gap: 8px;\
";

const FILTER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    margin-bottom: 8px;\
";

const FILTER_BTN_ACTIVE: &str = "\
    background: #1e1e5a; \
    color: #8080ff; \
    border: 1px solid #4a4aff; \
    border-radius: 6px; \
    padding: 3px 10px; \
    font-size: 12px; \
    cursor: pointer;\
";

const FILTER_BTN_INACTIVE: &str = "\
    background: transparent; \
    color: #888; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 3px 10px; \
    font-size: 12px; \
    cursor: pointer;\
";

const GAP_CARD: &str = "\
    background: #1a1a2e; \
    border: 1px solid #2a2a3a; \
    border-radius: 6px; \
    padding: 12px 14px;\
";

const GAP_TITLE_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    margin-bottom: 6px;\
";

const GAP_TITLE: &str = "\
    font-size: 14px; \
    font-weight: 600; \
    color: #e0e0e0;\
";

const GAP_LABEL: &str = "\
    font-size: 11px; \
    font-weight: 600; \
    padding: 1px 6px; \
    border-radius: 4px; \
    text-transform: uppercase;\
";

const CRITERIA_STYLE: &str = "\
    font-size: 13px; \
    color: #aaa; \
    margin-bottom: 4px;\
";

const ACTION_STYLE: &str = "\
    font-size: 12px; \
    color: #4a9aff; \
    background: #0f1a2a; \
    border-radius: 3px; \
    padding: 3px 6px; \
    margin-top: 4px;\
";

const EMPTY_STYLE: &str = "\
    color: #555; \
    font-size: 13px; \
    padding: 16px 0;\
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
                span { style: "font-size: 12px; color: #666;", "Filter:" }
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
                                        style: "font-size: 11px; color: {status_color};",
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
        RequirementPriority::P0 => "background: #3a0f0f; color: #ef4444;",
        RequirementPriority::P1 => "background: #2a1f05; color: #f59e0b;",
        RequirementPriority::P2 => "background: #0f1f2a; color: #4a9aff;",
        RequirementPriority::P3 => "background: #1a1a2e; color: #666;",
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
        VerificationStatus::Verified => "#22c55e",
        VerificationStatus::PartiallyVerified => "#f59e0b",
        VerificationStatus::Unverified => "#888",
        VerificationStatus::Failed => "#ef4444",
    }
}
