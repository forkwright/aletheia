//! Checkpoint list view: approval gates for a planning project.

use dioxus::prelude::*;

use crate::components::checkpoint_card::CheckpointCard;
use crate::state::checkpoints::{Checkpoint, CheckpointStore};

#[derive(Debug, Clone)]
#[expect(dead_code, reason = "checkpoint routes are pending B23 backend work")]
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
    padding: var(--space-4);\
";

const HEADER_ROW: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: space-between; \
    margin-bottom: var(--space-4);\
";

const PENDING_BANNER: &str = "\
    background: var(--bg-surface-dim); \
    border: 1px solid var(--accent); \
    border-radius: var(--radius-md); \
    padding: var(--space-2) var(--space-3); \
    font-size: var(--text-sm); \
    color: var(--accent); \
    margin-bottom: var(--space-3);\
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

const PLACEHOLDER_STYLE: &str = "\
    display: flex; \
    flex-direction: column; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    gap: var(--space-3); \
    color: var(--text-muted);\
";

/// Checkpoint approval list for a planning project.
///
/// Shows checkpoints when the pylon checkpoint API exists.
/// Pending gates appear at the top of the list once checkpoint routes land.
#[component]
pub(crate) fn CheckpointsView(project_id: String) -> Element {
    let mut fetch_state = use_signal(|| FetchState::NotAvailable);
    // WHY: incrementing this signal causes the fetch effect to re-run.
    let mut fetch_trigger = use_signal(|| 0u32);

    let project_id_card = project_id.clone();

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",
            div {
                style: "{HEADER_ROW}",
                h3 { style: "margin: 0; font-size: var(--text-md); color: var(--text-primary);", "Checkpoints" }
                button {
                    style: "{REFRESH_BTN}",
                    onclick: move |_| {
                        fetch_state.set(FetchState::NotAvailable);
                    },
                    "Refresh"
                }
            }

            match &*fetch_state.read() {
                FetchState::Loading => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--text-secondary);",
                        "Loading checkpoints..."
                    }
                },
                FetchState::Error(err) => rsx! {
                    div {
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--status-error);",
                        "Error: {err}"
                    }
                },
                FetchState::NotAvailable => rsx! {
                    div {
                        style: "{PLACEHOLDER_STYLE}",
                        div { style: "font-size: var(--text-md);", "Checkpoints not available" }
                        div { style: "font-size: var(--text-sm); max-width: 360px; text-align: center;",
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
                                    div { style: "font-size: var(--text-md);", "No checkpoints" }
                                    div { style: "font-size: var(--text-sm);",
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
