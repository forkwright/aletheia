//! Execution view: wave-based progress and parallel plan visualization.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::components::plan_card::PlanCard;
use crate::components::wave_band::WaveBand;
use crate::state::connection::ConnectionConfig;
use crate::state::execution::{ExecutionState, ExecutionStore};

/// Fetch state for execution data.
#[derive(Debug, Clone)]
enum ExecutionFetchState {
    Loading,
    Loaded(ExecutionState),
    NotAvailable,
    Error(String),
}

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    padding: 16px; \
    gap: 12px; \
    overflow-y: auto;\
";

const HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between;\
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

const PROGRESS_BAR_TRACK: &str = "\
    height: 6px; \
    background: #2a2a3a; \
    border-radius: 3px; \
    overflow: hidden;\
";

const PROGRESS_SUMMARY: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    font-size: 13px; \
    color: #aaa; \
    margin-bottom: 4px;\
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

/// Polling interval for execution state updates (10 seconds).
const POLL_INTERVAL_MS: u64 = 10_000;

/// Execution view with wave-based progress and plan visualization.
#[component]
pub(crate) fn ExecutionView(project_id: String) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut fetch_state = use_signal(|| ExecutionFetchState::Loading);
    let mut fetch_trigger = use_signal(|| 0u32);

    // Fetch execution state on mount and when trigger changes.
    let project_id_effect = project_id.clone();
    use_effect(move || {
        let _trigger = *fetch_trigger.read();
        let cfg = config.read().clone();
        let pid = project_id_effect.clone();
        fetch_state.set(ExecutionFetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/planning/projects/{pid}/execution",
                cfg.server_url.trim_end_matches('/')
            );

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<ExecutionState>().await {
                        Ok(state) => fetch_state.set(ExecutionFetchState::Loaded(state)),
                        Err(e) => {
                            fetch_state
                                .set(ExecutionFetchState::Error(format!("parse error: {e}")));
                        }
                    }
                }
                // WHY: 404 means execution endpoint not available on this pylon version.
                Ok(resp) if resp.status().as_u16() == 404 => {
                    fetch_state.set(ExecutionFetchState::NotAvailable);
                }
                Ok(resp) => {
                    let status = resp.status();
                    fetch_state.set(ExecutionFetchState::Error(format!(
                        "server returned {status}"
                    )));
                }
                Err(e) => {
                    fetch_state.set(ExecutionFetchState::Error(format!("connection error: {e}")));
                }
            }
        });
    });

    // Polling fallback: re-fetch every 10 seconds.
    use_effect(move || {
        spawn(async move {
            loop {
                tokio::time::sleep(tokio::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
                fetch_trigger.set(fetch_trigger() + 1);
            }
        });
    });

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            div {
                style: "{HEADER_ROW}",
                h3 { style: "font-size: 16px; margin: 0; color: #e0e0e0;", "Execution" }
                button {
                    style: "{REFRESH_BTN}",
                    onclick: move |_| fetch_trigger.set(fetch_trigger() + 1),
                    "Refresh"
                }
            }

            match &*fetch_state.read() {
                ExecutionFetchState::Loading => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #888;",
                        "Loading execution state..."
                    }
                },
                ExecutionFetchState::Error(err) => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #ef4444;",
                        "Error: {err}"
                    }
                },
                ExecutionFetchState::NotAvailable => rsx! {
                    div {
                        style: "{PLACEHOLDER_STYLE}",
                        div { style: "font-size: 48px;", "[E]" }
                        div { style: "font-size: 16px;", "Execution view not available" }
                        div { style: "font-size: 13px; max-width: 400px; text-align: center;",
                            "The execution API is not available on this pylon instance."
                        }
                    }
                },
                ExecutionFetchState::Loaded(state) => {
                    if state.waves.is_empty() {
                        rsx! {
                            div {
                                style: "{PLACEHOLDER_STYLE}",
                                div { style: "font-size: 16px;", "No execution in progress" }
                                div { style: "font-size: 13px;",
                                    "Waves will appear here when plan execution begins."
                                }
                            }
                        }
                    } else {
                        let store = ExecutionStore { state: Some(state.clone()) };
                        let overall_pct = store.overall_progress_pct();
                        let active_wave = store.active_wave_number();
                        let wave_count = store.wave_count();

                        let progress_text = match active_wave {
                            Some(n) => format!("Wave {n} of {wave_count} — {overall_pct}% complete"),
                            None => format!("{wave_count} waves — {overall_pct}% complete"),
                        };

                        rsx! {
                            // Overall progress
                            div {
                                style: "{PROGRESS_SUMMARY}",
                                span { "{progress_text}" }
                            }
                            div {
                                style: "{PROGRESS_BAR_TRACK}",
                                div {
                                    style: "height: 100%; background: #4a9aff; width: {overall_pct}%; border-radius: 3px; transition: width 0.3s;",
                                }
                            }

                            // Wave bands
                            for wave in &state.waves {
                                WaveBand {
                                    key: "{wave.wave_number}",
                                    wave: wave.clone(),
                                    for plan in &wave.plans {
                                        PlanCard {
                                            key: "{plan.id}",
                                            plan: plan.clone(),
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
}
