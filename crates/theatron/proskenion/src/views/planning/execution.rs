//! Execution view: wave-based progress and parallel plan visualization.

use dioxus::prelude::*;

use crate::components::plan_card::PlanCard;
use crate::components::wave_band::WaveBand;
use crate::state::execution::{ExecutionState, ExecutionStore};

/// Fetch state for execution data.
#[derive(Debug, Clone)]
#[expect(dead_code, reason = "execution route is pending B23 backend work")]
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
    padding: var(--space-4); \
    gap: var(--space-3); \
    overflow-y: auto;\
";

const HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between;\
";

const REFRESH_BTN: &str = "\
    background: var(--border); \
    color: var(--text-primary); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-1) var(--space-3); \
    font-size: var(--text-xs); \
    cursor: pointer; \
    transition: background-color var(--transition-quick), \
                color var(--transition-quick), \
                border-color var(--transition-quick);\
";

const PROGRESS_BAR_TRACK: &str = "\
    height: 6px; \
    background: var(--border); \
    border-radius: var(--radius-sm); \
    overflow: hidden;\
";

const PROGRESS_SUMMARY: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    font-size: var(--text-sm); \
    color: var(--text-secondary); \
    margin-bottom: var(--space-1);\
";

const PLACEHOLDER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    gap: var(--space-3); \
    color: var(--text-muted);\
";

/// Execution view with wave-based progress and plan visualization.
#[component]
pub(crate) fn ExecutionView(project_id: String) -> Element {
    let _ = &project_id;
    let mut fetch_state = use_signal(|| ExecutionFetchState::NotAvailable);

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            div {
                style: "{HEADER_ROW}",
                h3 { style: "font-size: var(--text-md); margin: 0; color: var(--text-primary);", "Execution" }
                button {
                    style: "{REFRESH_BTN}",
                    onclick: move |_| fetch_state.set(ExecutionFetchState::NotAvailable),
                    "Refresh"
                }
            }

            match &*fetch_state.read() {
                ExecutionFetchState::Loading => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--text-secondary);",
                        "Loading execution state..."
                    }
                },
                ExecutionFetchState::Error(err) => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--status-error);",
                        "Error: {err}"
                    }
                },
                ExecutionFetchState::NotAvailable => rsx! {
                    div {
                        style: "{PLACEHOLDER_STYLE}",
                        div { style: "font-size: var(--text-3xl);", "[E]" }
                        div { style: "font-size: var(--text-md);", "Execution view not available" }
                        div { style: "font-size: var(--text-sm); max-width: 400px; text-align: center;",
                            "The execution API is not available on this pylon instance."
                        }
                    }
                },
                ExecutionFetchState::Loaded(state) => {
                    if state.waves.is_empty() {
                        rsx! {
                            div {
                                style: "{PLACEHOLDER_STYLE}",
                                div { style: "font-size: var(--text-md);", "No execution in progress" }
                                div { style: "font-size: var(--text-sm);",
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
                            div {
                                style: "{PROGRESS_SUMMARY}",
                                span { "{progress_text}" }
                            }
                            div {
                                style: "{PROGRESS_BAR_TRACK}",
                                div {
                                    style: "height: 100%; background: var(--accent); width: {overall_pct}%; border-radius: var(--radius-sm); transition: width var(--transition-measured);",
                                }
                            }

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
