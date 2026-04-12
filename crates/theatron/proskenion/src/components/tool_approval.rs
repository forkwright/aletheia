//! Inline tool approval component with risk level indicators.

use dioxus::prelude::*;

use crate::state::tools::{RiskLevel, ToolApprovalState};

const APPROVAL_BASE_STYLE: &str = "\
    padding: var(--space-3); \
    border-radius: var(--radius-md); \
    margin-top: var(--space-1); \
    font-size: var(--text-sm);\
";

const TOOL_NAME_STYLE: &str = "\
    font-weight: var(--weight-semibold); \
    color: var(--text-primary); \
    font-size: var(--text-base);\
";

const REASON_STYLE: &str = "\
    color: var(--text-secondary); \
    margin-top: var(--space-1); \
    font-size: var(--text-sm);\
";

const INPUT_PREVIEW_STYLE: &str = "\
    background: var(--code-bg); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-sm); \
    padding: var(--space-2) var(--space-2); \
    margin-top: var(--space-2); \
    font-family: var(--font-mono); \
    font-size: var(--text-xs); \
    color: var(--code-fg); \
    max-height: 120px; \
    overflow-y: auto; \
    white-space: pre-wrap;\
";

const RISK_BADGE_BASE: &str = "\
    display: inline-block; \
    font-size: var(--text-xs); \
    font-weight: var(--weight-semibold); \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-lg); \
    margin-left: var(--space-2); \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const BUTTON_ROW_STYLE: &str = "\
    display: flex; \
    gap: var(--space-2); \
    margin-top: var(--space-3);\
";

const APPROVE_BTN_STYLE: &str = "\
    background: var(--status-success); \
    color: var(--text-inverse); \
    border: none; \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    font-weight: var(--weight-semibold); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const DENY_BTN_STYLE: &str = "\
    background: var(--status-error); \
    color: var(--text-inverse); \
    border: none; \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-4); \
    font-size: var(--text-sm); \
    font-weight: var(--weight-semibold); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const RESOLVED_STYLE: &str = "\
    color: var(--text-muted); \
    font-style: italic; \
    padding: var(--space-2); \
    font-size: var(--text-sm);\
";

/// Inline tool approval dialog.
///
/// Shows the tool name, risk level badge, reason, and input preview.
/// Approve/deny buttons send the decision via the `on_approve` and
/// `on_deny` callbacks.
#[component]
pub(crate) fn ToolApproval(
    approval: ToolApprovalState,
    on_approve: EventHandler<()>,
    on_deny: EventHandler<()>,
) -> Element {
    if approval.resolved {
        return rsx! {
            div { style: "{RESOLVED_STYLE}", "Tool approval resolved" }
        };
    }

    let container_style = approval_container_style(approval.risk);
    let badge_style = risk_badge_style(approval.risk);
    let risk_label = approval.risk.label();
    let input_text = serde_json::to_string_pretty(&approval.input)
        .unwrap_or_else(|_| approval.input.to_string());

    rsx! {
        div {
            style: "{container_style}",
            div {
                style: "display: flex; align-items: center;",
                span { style: "{TOOL_NAME_STYLE}", "{approval.tool_name}" }
                span { style: "{badge_style}", "{risk_label}" }
            }
            div { style: "{REASON_STYLE}", "{approval.reason}" }
            div {
                style: "{INPUT_PREVIEW_STYLE}",
                "{input_text}"
            }
            div {
                style: "{BUTTON_ROW_STYLE}",
                button {
                    style: "{APPROVE_BTN_STYLE}",
                    onclick: move |_| on_approve.call(()),
                    "Approve"
                }
                button {
                    style: "{DENY_BTN_STYLE}",
                    onclick: move |_| on_deny.call(()),
                    "Deny"
                }
            }
        }
    }
}

/// Build the container style with risk-level-appropriate border and background.
fn approval_container_style(risk: RiskLevel) -> String {
    let (border_color, background) = match risk {
        RiskLevel::Low => ("var(--status-success)", "var(--status-success-bg)"),
        RiskLevel::Medium => ("var(--status-warning)", "var(--status-warning-bg)"),
        RiskLevel::High => ("var(--status-error)", "var(--status-error-bg)"),
        RiskLevel::Critical => ("var(--aima)", "var(--aima-bg)"),
    };
    format!("{APPROVAL_BASE_STYLE} border: 2px solid {border_color}; background: {background};")
}

/// Build the risk badge style with risk-appropriate colors.
fn risk_badge_style(risk: RiskLevel) -> String {
    let (bg, color) = match risk {
        RiskLevel::Low => ("var(--status-success-bg)", "var(--status-success)"),
        RiskLevel::Medium => ("var(--status-warning-bg)", "var(--status-warning)"),
        RiskLevel::High => ("var(--status-error-bg)", "var(--status-error)"),
        RiskLevel::Critical => ("var(--aima-bg)", "var(--aima)"),
    };
    format!("{RISK_BADGE_BASE} background: {bg}; color: {color};")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_container_style_varies_by_risk() {
        let low = approval_container_style(RiskLevel::Low);
        let high = approval_container_style(RiskLevel::High);
        let critical = approval_container_style(RiskLevel::Critical);
        assert_ne!(low, high, "low and high styles must differ");
        assert_ne!(high, critical, "high and critical styles must differ");
    }

    #[test]
    fn risk_badge_style_varies_by_risk() {
        let medium = risk_badge_style(RiskLevel::Medium);
        let critical = risk_badge_style(RiskLevel::Critical);
        assert_ne!(medium, critical, "badge styles must differ by risk");
    }

    #[test]
    fn approval_container_critical_has_distinct_background() {
        let critical = approval_container_style(RiskLevel::Critical);
        // WHY: critical risk gets a visibly red-tinted background.
        assert!(
            critical.contains("--aima-bg"),
            "critical should use aima-bg token"
        );
    }
}
