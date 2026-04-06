//! Drift detection panel: orphan entities, low-connectivity warnings, health score.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::graph::{DriftAnalysis, health_color};

const DRIFT_CONTAINER: &str = "\
    display: flex; \
    flex-direction: column; \
    flex: 1; \
    gap: 12px; \
    overflow-y: auto;\
";

const SECTION_STYLE: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px;\
";

const SECTION_TITLE: &str = "\
    font-size: 14px; \
    font-weight: bold; \
    color: #aaa; \
    margin-bottom: 12px;\
";

const ROW_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 12px; \
    padding: 8px 0; \
    border-bottom: 1px solid #222; \
    font-size: 13px;\
";

const ENTITY_NAME: &str = "\
    color: #e0e0e0; \
    flex: 1;\
";

const ENTITY_TYPE_BADGE: &str = "\
    font-size: 10px; \
    color: #888; \
    background: #2a2a3a; \
    padding: 2px 6px; \
    border-radius: 4px;\
";

const ACTION_BADGE: &str = "\
    font-size: 10px; \
    padding: 2px 8px; \
    border-radius: 4px;\
";

const TREND_UP: &str = "color: #22c55e; font-size: 11px;";
const TREND_DOWN: &str = "color: #ef4444; font-size: 11px;";
const TREND_FLAT: &str = "color: #888; font-size: 11px;";

const HEALTH_CARD: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    padding: 16px; \
    min-width: 100px;\
";

const HEALTH_VALUE: &str = "\
    font-size: 32px; \
    font-weight: bold;\
";

const HEALTH_LABEL: &str = "\
    font-size: 11px; \
    color: #888; \
    margin-top: 4px;\
";

const RECOMMENDATION_STYLE: &str = "\
    padding: 8px 12px; \
    background: #12110f; \
    border-left: 3px solid #9A7B4F; \
    border-radius: 0 4px 4px 0; \
    font-size: 13px; \
    color: #e0e0e0; \
    line-height: 1.4;\
";

const REC_CATEGORY: &str = "\
    font-size: 10px; \
    color: #666; \
    margin-top: 4px;\
";

const STATUS_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    color: #888; \
    font-size: 14px;\
";

const REFRESH_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 4px 12px; \
    font-size: 12px; \
    cursor: pointer;\
";

/// Drift detection panel with orphan entities, connectivity warnings, and health score.
#[component]
pub(crate) fn DriftView() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut fetch_state = use_signal(|| FetchState::<DriftAnalysis>::Loading);
    let mut drift = use_signal(DriftAnalysis::default);

    let mut do_fetch = move || {
        let cfg = config.read().clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/memory/graph/drift",
                cfg.server_url.trim_end_matches('/')
            );

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<DriftAnalysis>().await {
                        Ok(data) => {
                            drift.set(data);
                            fetch_state.set(FetchState::Loaded(DriftAnalysis::default()));
                        }
                        Err(e) => {
                            fetch_state.set(FetchState::Error(format!("parse error: {e}")));
                        }
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    fetch_state.set(FetchState::Error(format!("server returned {status}")));
                }
                Err(e) => {
                    fetch_state.set(FetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    };

    use_effect(move || {
        do_fetch();
    });

    let current_drift = drift.read();

    rsx! {
        div {
            style: "{DRIFT_CONTAINER}",

            div {
                style: "display: flex; align-items: center; justify-content: space-between;",
                h3 { style: "font-size: 16px; margin: 0; color: #e0e0e0;", "Drift Detection" }
                button {
                    style: "{REFRESH_BTN}",
                    onclick: move |_| do_fetch(),
                    "Refresh"
                }
            }

            match &*fetch_state.read() {
                FetchState::Loading => rsx! {
                    div { style: "{STATUS_STYLE}", "Analyzing graph drift..." }
                },
                FetchState::Error(err) => rsx! {
                    div { style: "{STATUS_STYLE} color: #ef4444;", "Error: {err}" }
                },
                FetchState::Loaded(_) => rsx! {
                    // Health score dashboard
                    div {
                        style: "{SECTION_STYLE}",
                        div { style: "{SECTION_TITLE}", "Health Score" }
                        div {
                            style: "display: flex; gap: 16px; flex-wrap: wrap;",
                            HealthCard {
                                value: current_drift.health_score.overall,
                                label: "Overall",
                            }
                            HealthCard {
                                value: current_drift.health_score.connectivity,
                                label: "Connectivity",
                            }
                            HealthCard {
                                value: current_drift.health_score.community_distribution,
                                label: "Community",
                            }
                            HealthCard {
                                value: current_drift.health_score.orphan_ratio,
                                label: "Orphan Ratio",
                            }
                        }
                    }

                    // Orphan entities
                    div {
                        style: "{SECTION_STYLE}",
                        div { style: "{SECTION_TITLE}",
                            "Orphan Entities ({current_drift.orphan_entities.len()})"
                        }
                        if current_drift.orphan_entities.is_empty() {
                            div { style: "color: #555; font-size: 13px;", "No orphan entities detected" }
                        }
                        for (i , orphan) in current_drift.orphan_entities.iter().enumerate() {
                            div {
                                key: "orphan-{i}",
                                style: "{ROW_STYLE}",
                                span { style: "{ENTITY_NAME}", "{orphan.name}" }
                                span { style: "{ENTITY_TYPE_BADGE}", "{orphan.entity_type}" }
                                span { style: "font-size: 11px; color: #666;",
                                    "{orphan.relationship_count} rel"
                                }
                                if let Some(ref date) = orphan.created_at {
                                    span { style: "font-size: 11px; color: #555;", "{date}" }
                                }
                                span {
                                    style: "{ACTION_BADGE} {action_badge_color(&orphan.suggested_action)}",
                                    "{orphan.suggested_action}"
                                }
                            }
                        }
                    }

                    // Low connectivity warnings
                    div {
                        style: "{SECTION_STYLE}",
                        div { style: "{SECTION_TITLE}",
                            "Low Connectivity ({current_drift.low_connectivity.len()})"
                        }
                        if current_drift.low_connectivity.is_empty() {
                            div { style: "color: #555; font-size: 13px;", "No connectivity warnings" }
                        }
                        for (i , warning) in current_drift.low_connectivity.iter().enumerate() {
                            div {
                                key: "lc-{i}",
                                style: "{ROW_STYLE}",
                                span { style: "{ENTITY_NAME}", "{warning.name}" }
                                span {
                                    style: "font-size: 12px; color: #e0e0e0;",
                                    "{warning.current_count}"
                                }
                                span { style: trend_style(&warning.trend),
                                    "{trend_arrow(&warning.trend)} from {warning.previous_count}"
                                }
                            }
                        }
                    }

                    // Recommendations
                    if !current_drift.recommendations.is_empty() {
                        div {
                            style: "{SECTION_STYLE}",
                            div { style: "{SECTION_TITLE}",
                                "Recommendations ({current_drift.recommendations.len()})"
                            }
                            div {
                                style: "display: flex; flex-direction: column; gap: 8px;",
                                for (i , rec) in current_drift.recommendations.iter().enumerate() {
                                    div {
                                        key: "rec-{i}",
                                        style: "{RECOMMENDATION_STYLE}",
                                        "{rec.message}"
                                        div { style: "{REC_CATEGORY}", "{rec.category}" }
                                    }
                                }
                            }
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn HealthCard(value: u32, label: &'static str) -> Element {
    let color = health_color(value);
    rsx! {
        div {
            style: "{HEALTH_CARD}",
            div { style: "{HEALTH_VALUE} color: {color};", "{value}" }
            div { style: "{HEALTH_LABEL}", "{label}" }
        }
    }
}

fn action_badge_color(action: &str) -> String {
    match action.to_lowercase().as_str() {
        "connect" => "background: #1a2a3a; color: #4a9aff;".to_string(),
        "delete" | "remove" => "background: #2a1a1a; color: #ef4444;".to_string(),
        "archive" => "background: #2a2a3a; color: #888;".to_string(),
        _ => "background: #2a2a1a; color: #f59e0b;".to_string(),
    }
}

fn trend_style(trend: &str) -> &'static str {
    match trend.to_lowercase().as_str() {
        "up" | "increasing" => TREND_UP,
        "down" | "decreasing" => TREND_DOWN,
        _ => TREND_FLAT,
    }
}

fn trend_arrow(trend: &str) -> &'static str {
    match trend.to_lowercase().as_str() {
        "up" | "increasing" => "+",
        "down" | "decreasing" => "-",
        _ => "=",
    }
}
