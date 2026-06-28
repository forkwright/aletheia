//! Provider/model readiness panel backed by `/api/v1/providers`.

use dioxus::prelude::*;
use skene::api::types::ProviderInfo;
use skeue::EmptyState;

use crate::state::ops::{HealthStatus, ProviderInventoryStore};

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

const SUMMARY_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-3); \
    margin-bottom: var(--space-3);\
";

const PROVIDER_ROW: &str = "\
    display: grid; \
    gap: var(--space-1); \
    padding: var(--space-2) 0; \
    border-bottom: 1px solid var(--border-separator); \
    font-size: var(--text-xs);\
";

const PROVIDER_HEAD: &str = "\
    display: flex; \
    align-items: center; \
    gap: var(--space-2); \
    min-width: 0;\
";

const PROVIDER_NAME: &str = "\
    color: var(--text-primary); \
    font-weight: var(--weight-bold); \
    min-width: 0; \
    overflow-wrap: anywhere;\
";

const META_STYLE: &str = "\
    color: var(--text-muted); \
    overflow-wrap: anywhere;\
";

const MODELS_STYLE: &str = "\
    color: var(--text-secondary); \
    overflow-wrap: anywhere; \
    line-height: 1.4;\
";

const ERROR_STYLE: &str = "\
    color: var(--status-error); \
    font-size: var(--text-sm); \
    margin-bottom: var(--space-3);\
";

const STATUS_BADGE_BASE: &str = "\
    font-size: var(--text-xs); \
    font-weight: var(--weight-bold); \
    padding: var(--space-1) var(--space-2); \
    border-radius: var(--radius-md); \
    text-transform: uppercase; \
    letter-spacing: 0; \
    flex-shrink: 0;\
";

#[component]
pub(crate) fn ProviderReadinessPanel(store: Signal<ProviderInventoryStore>) -> Element {
    let data = store.read();
    let ready = data.ready_count();
    let total = data.providers.len();

    rsx! {
        div {
            style: "{PANEL_STYLE}",

            div { style: "{SECTION_TITLE}", "Provider Routes" }

            div {
                style: "{SUMMARY_ROW}",
                span {
                    style: format!(
                        "{STATUS_BADGE_BASE} background: {}; color: {};",
                        summary_color(ready, total),
                        summary_text_color(ready, total),
                    ),
                    "{ready}/{total} ready"
                }
                if total == 0 && data.error.is_none() {
                    EmptyState { title: "No providers registered".to_string() }
                }
            }

            if let Some(ref err) = data.error {
                div {
                    style: "{ERROR_STYLE}",
                    "{err}"
                }
            }

            for provider in &data.providers {
                {render_provider_row(provider)}
            }
        }
    }
}

fn render_provider_row(provider: &ProviderInfo) -> Element {
    let status = HealthStatus::from_status(&provider.health);
    let badge_style = format!(
        "{STATUS_BADGE_BASE} background: {}; color: {};",
        status.dot_color(),
        badge_text_color(status),
    );
    let meta = format!(
        "{} / {} / auth {} / {}",
        provider.kind, provider.deployment_target, provider.auth_source, provider.base_url
    );
    let advertised_models = model_list(&provider.supported_models, "no advertised models");
    let configured_models = model_list(&provider.configured_models, "no configured models");

    rsx! {
        div {
            style: "{PROVIDER_ROW}",
            div {
                style: "{PROVIDER_HEAD}",
                span { style: "{PROVIDER_NAME}", "{provider.name}" }
                span { style: "{badge_style}", "{provider.health}" }
            }
            div { style: "{META_STYLE}", "{meta}" }
            div { style: "{MODELS_STYLE}", "advertised: {advertised_models}" }
            div { style: "{MODELS_STYLE}", "configured: {configured_models}" }
            if let Some(ref reason) = provider.health_reason {
                div { style: "{META_STYLE}", "{reason}" }
            }
        }
    }
}

fn model_list(models: &[String], empty: &str) -> String {
    if models.is_empty() {
        empty.to_string()
    } else {
        models.join(", ")
    }
}

fn summary_color(ready: usize, total: usize) -> &'static str {
    if total == 0 {
        "var(--text-muted)"
    } else if ready == total {
        "var(--status-success)"
    } else if ready == 0 {
        "var(--status-error)"
    } else {
        "var(--status-warning)"
    }
}

fn summary_text_color(ready: usize, total: usize) -> &'static str {
    if total > 0 && ready != 0 && ready != total {
        "var(--text-primary)"
    } else {
        "var(--bg-surface)"
    }
}

fn badge_text_color(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Degraded => "var(--text-primary)",
        _ => "var(--bg-surface)",
    }
}
