//! Service health panel: aggregate status and per-check rows from `/api/health`.

use dioxus::prelude::*;
use skeue::EmptyState;

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

const PROVIDER_LIST_STYLE: &str = "\
    margin: 0 0 var(--space-2) 18px; \
    display: grid; \
    gap: var(--space-1);\
";

const PROVIDER_ROW_STYLE: &str = "\
    display: grid; \
    grid-template-columns: minmax(92px, 1fr) auto minmax(96px, 2fr) auto; \
    gap: var(--space-2); \
    align-items: center; \
    font-size: var(--text-xs); \
    color: var(--text-secondary);\
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

const PROVIDER_REASON_STYLE: &str = "\
    color: var(--text-muted); \
    font-size: var(--text-xs); \
    white-space: nowrap; \
    overflow: hidden; \
    text-overflow: ellipsis;\
";

const STATUS_BADGE_BASE: &str = "\
    font-size: var(--text-xs); \
    font-weight: var(--weight-bold); \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-md); \
    text-transform: uppercase; \
    letter-spacing: 0.5px;\
";

#[derive(Debug, Clone)]
struct ProviderHealthRow {
    name: String,
    status: String,
    reason: Option<String>,
    optional: bool,
}

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
                    EmptyState { title: "No checks loaded".to_string() }
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
                {render_provider_details(check)}
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

fn render_provider_details(check: &HealthCheckInfo) -> Element {
    let providers = provider_health_rows(check);
    if providers.is_empty() {
        return rsx! {};
    }

    rsx! {
        div {
            style: "{PROVIDER_LIST_STYLE}",
            for provider in providers {
                {render_provider_row(provider)}
            }
        }
    }
}

fn render_provider_row(provider: ProviderHealthRow) -> Element {
    let ProviderHealthRow {
        name,
        status: provider_status,
        reason,
        optional,
    } = provider;
    let status = HealthStatus::from_status(&provider_status);
    let dot_style = format!("{DOT_BASE} background: {};", status.dot_color());
    let reason = reason.unwrap_or_else(|| "ok".to_owned());
    let role = if optional { "optional" } else { "required" };

    rsx! {
        div {
            style: "{PROVIDER_ROW_STYLE}",
            span { style: "{NAME_STYLE}", "{name}" }
            span { style: "display: inline-flex; align-items: center; gap: var(--space-1); color: var(--text-primary);",
                span { style: "{dot_style}" }
                span { "{provider_status}" }
            }
            span { style: "{PROVIDER_REASON_STYLE}", "{reason}" }
            span { style: "color: var(--text-muted); text-transform: uppercase; font-size: var(--text-xs);", "{role}" }
        }
    }
}

fn provider_health_rows(check: &HealthCheckInfo) -> Vec<ProviderHealthRow> {
    let Some(providers) = check
        .details
        .as_ref()
        .and_then(|details| details.get("providers"))
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };

    providers
        .iter()
        .filter_map(|provider| {
            let name = provider.get("name")?.as_str()?.to_owned();
            let status = provider.get("status")?.as_str()?.to_owned();
            let reason = provider
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned);
            let optional = provider
                .get("optional")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or_default();
            Some(ProviderHealthRow {
                name,
                status,
                reason,
                optional,
            })
        })
        .collect()
}

fn badge_text_color(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Degraded => "var(--text-primary)",
        _ => "var(--bg-surface)",
    }
}
