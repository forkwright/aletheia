//! Planning view: project list and project detail with checkpoints and verification.

pub(crate) mod checkpoints;
pub(crate) mod gap_analysis;
pub(crate) mod project_detail;
pub(crate) mod verification;

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::state::connection::ConnectionConfig;

#[derive(Debug, Clone, serde::Deserialize)]
struct Plan {
    #[serde(default)]
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct PlanStep {
    #[serde(default)]
    description: String,
    #[serde(default)]
    status: String,
}

/// Planning has an extra variant for 404 (endpoint not available), so it
/// keeps its own state enum rather than using the shared `FetchState<T>`.
#[derive(Debug, Clone)]
enum PlanFetchState {
    Loading,
    Loaded(Vec<Plan>),
    NoPlanningAvailable,
    Error(String),
}

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    gap: 16px;\
";

const PLAN_CARD: &str = "\
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    padding: 16px 20px;\
";

const PLAN_TITLE: &str = "\
    font-size: 16px; \
    font-weight: bold; \
    color: #e0e0e0; \
    margin-bottom: 8px;\
";

const PLAN_STATUS: &str = "\
    display: inline-block; \
    padding: 2px 8px; \
    border-radius: 4px; \
    font-size: 11px; \
    margin-bottom: 12px;\
";

const STEP_STYLE: &str = "\
    display: flex; \
    align-items: flex-start; \
    gap: 8px; \
    padding: 6px 0; \
    border-bottom: 1px solid #222; \
    font-size: 13px;\
";

const STEP_MARKER: &str = "\
    flex-shrink: 0; \
    width: 18px; \
    text-align: center; \
    font-size: 12px;\
";

const PLACEHOLDER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    gap: 12px; \
    color: #555;\
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

#[component]
pub(crate) fn Planning() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut fetch_state = use_signal(|| PlanFetchState::Loading);

    let mut do_refresh = move || {
        let cfg = config.read().clone();
        fetch_state.set(PlanFetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!("{}/api/v1/plans", cfg.server_url.trim_end_matches('/'));

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.json::<Vec<Plan>>().await {
                    Ok(plans) => fetch_state.set(PlanFetchState::Loaded(plans)),
                    Err(e) => {
                        fetch_state.set(PlanFetchState::Error(format!("parse error: {e}")));
                    }
                },
                // WHY: 404 means planning endpoint not available on this pylon version.
                Ok(resp) if resp.status().as_u16() == 404 => {
                    fetch_state.set(PlanFetchState::NoPlanningAvailable);
                }
                Ok(resp) => {
                    let status = resp.status();
                    fetch_state.set(PlanFetchState::Error(format!("server returned {status}")));
                }
                Err(e) => {
                    fetch_state.set(PlanFetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    };

    use_effect(move || {
        do_refresh();
    });

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            div {
                style: "display: flex; align-items: center; justify-content: space-between;",
                h2 { style: "font-size: 20px; margin: 0;", "Planning" }
                button {
                    style: "{REFRESH_BTN}",
                    onclick: move |_| do_refresh(),
                    "Refresh"
                }
            }

            match &*fetch_state.read() {
                PlanFetchState::Loading => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #888;",
                        "Loading plans..."
                    }
                },
                PlanFetchState::Error(err) => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #ef4444;",
                        "Error: {err}"
                    }
                },
                PlanFetchState::NoPlanningAvailable => rsx! {
                    div {
                        style: "{PLACEHOLDER_STYLE}",
                        div { style: "font-size: 48px;", "[P]" }
                        div { style: "font-size: 16px;", "No planning service available" }
                        div { style: "font-size: 13px; max-width: 400px; text-align: center;",
                            "The planning API is not available on this pylon instance. "
                            "Plans will appear here when connected to a pylon with dianoia integration."
                        }
                    }
                },
                PlanFetchState::Loaded(plans) => {
                    if plans.is_empty() {
                        rsx! {
                            div {
                                style: "{PLACEHOLDER_STYLE}",
                                div { style: "font-size: 16px;", "No active plans" }
                                div { style: "font-size: 13px;",
                                    "Plans will appear here when agents create them."
                                }
                            }
                        }
                    } else {
                        rsx! {
                            div {
                                style: "flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 12px;",
                                for plan in plans {
                                    {render_plan(plan)}
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn render_plan(plan: &Plan) -> Element {
    let status_style = match plan.status.as_str() {
        "active" | "in_progress" => "background: #1a2a3a; color: #4a9aff;",
        "complete" | "completed" => "background: #1a3a1a; color: #22c55e;",
        "failed" => "background: #3a1a1a; color: #ef4444;",
        _ => "background: #2a2a3a; color: #888;",
    };

    rsx! {
        div {
            style: "{PLAN_CARD}",
            div { style: "{PLAN_TITLE}", "{plan.title}" }
            span { style: "{PLAN_STATUS} {status_style}", "{plan.status}" }
            if !plan.id.is_empty() {
                span { style: "font-size: 11px; color: #555; margin-left: 8px;",
                    "{plan.id}"
                }
            }
            for (i , step) in plan.steps.iter().enumerate() {
                div {
                    key: "{i}",
                    style: "{STEP_STYLE}",
                    span {
                        style: "{STEP_MARKER} color: {step_color(&step.status)};",
                        "{step_icon(&step.status)}"
                    }
                    span { style: "color: #e0e0e0;", "{step.description}" }
                }
            }
        }
    }
}

fn step_icon(status: &str) -> &'static str {
    match status {
        "complete" | "completed" | "done" => "[v]",
        "active" | "in_progress" | "running" => "[>]",
        "failed" | "error" => "[x]",
        "skipped" => "[-]",
        _ => "[ ]",
    }
}

fn step_color(status: &str) -> &'static str {
    match status {
        "complete" | "completed" | "done" => "#22c55e",
        "active" | "in_progress" | "running" => "#4a9aff",
        "failed" | "error" => "#ef4444",
        "skipped" => "#666",
        _ => "#888",
    }
}
