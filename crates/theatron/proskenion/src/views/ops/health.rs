//! Service health panel: aggregate status and per-check rows from `/api/health`.

use dioxus::prelude::*;

use crate::state::ops::{HealthCheckInfo, HealthStatus, ServiceHealthStore};

const PANEL_STYLE: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4); \
    flex: 1; \
    overflow-y: auto; \
    min-width: 280px;\
";

const SECTION_TITLE: &str = "\
    font-size: var(--text-base); \
    font-weight: var(--weight-bold); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-3);\
";

const ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    padding: var(--space-1) 0; \
    border-bottom: 1px solid var(--border-separator); \
    font-size: var(--text-xs);\
";

const DOT_BASE: &str = "\
    width: 8px; \
    height: 8px; \
    border-radius: 50%; \
    flex-shrink: 0;\
";

const NAME_STYLE: &str = "\
    color: var(--text-primary); \
    flex: 1; \
    white-space: nowrap; \
    overflow: hidden; \
    text-overflow: ellipsis;\
";

const MESSAGE_STYLE: &str = "\
    color: var(--text-muted); \
    font-size: var(--text-xs); \
    white-space: nowrap;\
";

const EMPTY_STATE: &str = "\
    color: var(--text-muted); \
    font-size: var(--text-xs); \
    padding: var(--space-1) 0;\
";

const STATUS_BADGE_BASE: &str = "\
    font-size: var(--text-xs); \
    font-weight: var(--weight-bold); \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-md); \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

#[component]
pub(crate) fn ServiceHealthPanel(store: Signal<ServiceHealthStore>) -> Element {
    let data = store.read();

    rsx! {
        div {
            style: "{PANEL_STYLE}",

            div { style: "{SECTION_TITLE}", "Service Health" }

            div {
                style: "display: flex; align-items: center; gap: var(--space-3); margin-bottom: var(--space-3);",
                span {
                    style: format!("{STATUS_BADGE_BASE} background: {}; color: {};",
                        data.status.dot_color(),
                        badge_text_color(data.status)),
                    "{data.status.label()}"
                }
                if data.checks.is_empty() && data.error.is_none() {
                    span { style: "{EMPTY_STATE}", "No checks loaded" }
                }
            }

            if let Some(ref err) = data.error {
                div {
                    style: "color: var(--status-error); font-size: var(--text-sm); margin-bottom: var(--space-3);",
                    "{err}"
                }
            }

            for check in &data.checks {
                {render_check_row(check)}
            }
        }
    }
}

fn render_check_row(check: &HealthCheckInfo) -> Element {
    let status = HealthStatus::from_status(&check.status);
    let dot_style = format!("{DOT_BASE} background: {};", status.dot_color());
    let badge_style = format!(
        "{STATUS_BADGE_BASE} background: {}; color: {};",
        status.dot_color(),
        badge_text_color(status)
    );

    rsx! {
        div {
            style: "{ROW_STYLE}",
            span { style: "{dot_style}" }
            span { style: "{NAME_STYLE}", "{check.name}" }
            span { style: "{badge_style}", "{check.status}" }
            if let Some(ref message) = check.message {
                span { style: "{MESSAGE_STYLE}", "{message}" }
            }
        }
    }
}

fn badge_text_color(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Degraded => "var(--text-primary)",
        _ => "var(--bg-surface)",
    }
}
