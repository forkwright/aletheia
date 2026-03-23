//! Inline tool approval component with risk level indicators.

use dioxus::prelude::*;

use crate::state::tools::{RiskLevel, ToolApprovalState};

const APPROVAL_BASE_STYLE: &str = "\
    padding: 12px; \
    border-radius: 8px; \
    margin-top: 4px; \
    font-size: 13px;\
";

const TOOL_NAME_STYLE: &str = "\
    font-weight: 600; \
    color: #c0c0e0; \
    font-size: 14px;\
";

const REASON_STYLE: &str = "\
    color: #aaa; \
    margin-top: 4px; \
    font-size: 13px;\
";

const INPUT_PREVIEW_STYLE: &str = "\
    background: #0f0f1a; \
    border: 1px solid #333; \
    border-radius: 4px; \
    padding: 6px 8px; \
    margin-top: 8px; \
    font-family: 'JetBrains Mono', 'Fira Code', monospace; \
    font-size: 12px; \
    color: #b0b0d0; \
    max-height: 120px; \
    overflow-y: auto; \
    white-space: pre-wrap;\
";

const RISK_BADGE_BASE: &str = "\
    display: inline-block; \
    font-size: 11px; \
    font-weight: 600; \
    padding: 2px 8px; \
    border-radius: 10px; \
    margin-left: 8px; \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

const BUTTON_ROW_STYLE: &str = "\
    display: flex; \
    gap: 8px; \
    margin-top: 10px;\
";

const APPROVE_BTN_STYLE: &str = "\
    background: #22c55e; \
    color: #0f0f1a; \
    border: none; \
    border-radius: 6px; \
    padding: 6px 16px; \
    font-size: 13px; \
    font-weight: 600; \
    cursor: pointer;\
";

const DENY_BTN_STYLE: &str = "\
    background: #ef4444; \
    color: white; \
    border: none; \
    border-radius: 6px; \
    padding: 6px 16px; \
    font-size: 13px; \
    font-weight: 600; \
    cursor: pointer;\
";

const RESOLVED_STYLE: &str = "\
    color: #666; \
    font-style: italic; \
    padding: 8px; \
    font-size: 13px;\
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
        RiskLevel::Low => ("#2a4a2a", "#1a1a2e"),
        RiskLevel::Medium => ("#8b6914", "#1e1a10"),
        RiskLevel::High => ("#7a2020", "#1e0f0f"),
        RiskLevel::Critical => ("#a02020", "#2a1010"),
    };
    format!("{APPROVAL_BASE_STYLE} border: 2px solid {border_color}; background: {background};")
}

/// Build the risk badge style with risk-appropriate colors.
fn risk_badge_style(risk: RiskLevel) -> String {
    let (bg, color) = match risk {
        RiskLevel::Low => ("#1a3a1a", "#4ade80"),
        RiskLevel::Medium => ("#3a2a0a", "#fbbf24"),
        RiskLevel::High => ("#3a1010", "#f87171"),
        RiskLevel::Critical => ("#5a1515", "#fca5a5"),
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
            critical.contains("#2a1010"),
            "critical should have red-tinted background"
        );
    }
}
