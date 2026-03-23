//! Checkpoint list view: approval gates for a planning project.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::components::checkpoint_card::CheckpointCard;
use crate::state::checkpoints::{Checkpoint, CheckpointStore};
use crate::state::connection::ConnectionConfig;

#[derive(Debug, Clone)]
enum FetchState {
    Loading,
    Loaded(CheckpointStore),
    NotAvailable,
    Error(String),
}

const CONTAINER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    height: 100%; \
    padding: 16px;\
";

const HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: 16px;\
";

const PENDING_BANNER: &str = "\
    background: #1e1e5a; \
    border: 1px solid #4a4aff; \
    border-radius: 6px; \
    padding: 8px 12px; \
    font-size: 13px; \
    color: #8080ff; \
    margin-bottom: 12px;\
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

const PLACEHOLDER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    gap: 10px; \
    color: #555;\
";

/// Checkpoint approval list for a planning project.
///
/// Fetches from `GET /api/planning/projects/{project_id}/checkpoints`.
/// Pending gates appear at the top of the list.
///
/// # TODO
/// Wire SSE checkpoint events (when added to `theatron_core::events::StreamEvent`)
/// for real-time notification when new checkpoints arrive.
#[component]
pub(crate) fn CheckpointsView(project_id: String) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut fetch_state = use_signal(|| FetchState::Loading);
    // WHY: incrementing this signal causes the fetch effect to re-run.
    let mut fetch_trigger = use_signal(|| 0u32);

    let project_id_effect = project_id.clone();

    // Re-runs on mount and whenever fetch_trigger changes.
    use_effect(move || {
        let _ = *fetch_trigger.read();
        let cfg = config.read().clone();
        let pid = project_id_effect.clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/planning/projects/{pid}/checkpoints",
                cfg.server_url.trim_end_matches('/')
            );

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<Vec<Checkpoint>>().await {
                        Ok(checkpoints) => {
                            fetch_state.set(FetchState::Loaded(CheckpointStore { checkpoints }));
                        }
                        Err(e) => {
                            fetch_state.set(FetchState::Error(format!("parse error: {e}")));
                        }
                    }
                }
                // WHY: 404 means checkpoint endpoint not yet on this pylon version.
                Ok(resp) if resp.status().as_u16() == 404 => {
                    fetch_state.set(FetchState::NotAvailable);
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
    });

    let project_id_card = project_id.clone();

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            div {
                style: "{HEADER_ROW}",
                h3 { style: "margin: 0; font-size: 16px; color: #e0e0e0;", "Checkpoints" }
                button {
                    style: "{REFRESH_BTN}",
                    onclick: move |_| {
                        let next = *fetch_trigger.peek() + 1;
                        fetch_trigger.set(next);
                    },
                    "Refresh"
                }
            }

            match &*fetch_state.read() {
                FetchState::Loading => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #888;",
                        "Loading checkpoints..."
                    }
                },
                FetchState::Error(err) => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: #ef4444;",
                        "Error: {err}"
                    }
                },
                FetchState::NotAvailable => rsx! {
                    div {
                        style: "{PLACEHOLDER_STYLE}",
                        div { style: "font-size: 16px;", "Checkpoints not available" }
                        div { style: "font-size: 13px; max-width: 360px; text-align: center;",
                            "The checkpoint API is not available on this pylon instance."
                        }
                    }
                },
                FetchState::Loaded(store) => {
                    let pending = store.pending_count();
                    let sorted_owned: Vec<Checkpoint> =
                        store.sorted().into_iter().cloned().collect();
                    let pid = project_id_card.clone();

                    rsx! {
                        div {
                            style: "flex: 1; overflow-y: auto;",
                            if pending > 0 {
                                {
                                    let noun = if pending == 1 { "checkpoint" } else { "checkpoints" };
                                    rsx! {
                                        div { style: "{PENDING_BANNER}",
                                            "[!] {pending} {noun} awaiting approval"
                                        }
                                    }
                                }
                            }
                            if sorted_owned.is_empty() {
                                div {
                                    style: "{PLACEHOLDER_STYLE}",
                                    div { style: "font-size: 16px;", "No checkpoints" }
                                    div { style: "font-size: 13px;",
                                        "Checkpoints will appear as the project progresses."
                                    }
                                }
                            } else {
                                for checkpoint in sorted_owned {
                                    {
                                        let key = checkpoint.id.clone();
                                        let project_id_inner = pid.clone();
                                        rsx! {
                                            CheckpointCard {
                                                key: "{key}",
                                                checkpoint,
                                                project_id: project_id_inner,
                                                on_action_complete: move |_| {
                                                    let next = *fetch_trigger.peek() + 1;
                                                    fetch_trigger.set(next);
                                                },
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
}
