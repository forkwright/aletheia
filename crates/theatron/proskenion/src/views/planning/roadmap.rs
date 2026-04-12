//! Roadmap timeline: horizontal phase visualization with dependency lines.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::components::timeline::{
    Timeline, TimelineBlock, TimelineDependencyLine, phase_positions,
};
use crate::state::connection::ConnectionConfig;
use crate::state::planning::{
    Phase, Roadmap, RoadmapStore, phase_border_color, phase_status_color,
};

#[derive(Debug, Clone)]
enum FetchState {
    Loading,
    Loaded(RoadmapStore),
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

const DETAIL_PANEL: &str = "\
    background: var(--bg-surface); \
    border: 1px solid var(--border); \
    border-radius: var(--radius-md); \
    padding: var(--space-4) var(--space-4); \
    margin-top: var(--space-4);\
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

const PIXELS_PER_DAY: f64 = 4.0;

/// Roadmap timeline view for a planning project.
///
/// Fetches from `GET /api/planning/projects/{project_id}/roadmap`.
/// Renders phases as timeline blocks with dependency arrows.
#[component]
pub(crate) fn RoadmapView(project_id: String) -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut fetch_state = use_signal(|| FetchState::Loading);
    let mut fetch_trigger = use_signal(|| 0u32);
    let mut zoom = use_signal(|| 1.0f64);
    let mut selected_phase = use_signal(|| None::<usize>);

    let project_id_effect = project_id.clone();

    use_effect(move || {
        let _ = *fetch_trigger.read();
        let cfg = config.read().clone();
        let pid = project_id_effect.clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!(
                "{}/api/planning/projects/{pid}/roadmap",
                cfg.server_url.trim_end_matches('/')
            );

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.json::<Roadmap>().await {
                    Ok(roadmap) => {
                        fetch_state.set(FetchState::Loaded(RoadmapStore {
                            roadmap: Some(roadmap),
                        }));
                    }
                    Err(e) => {
                        fetch_state.set(FetchState::Error(format!("parse error: {e}")));
                    }
                },
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

    rsx! {
        div {
            style: "{CONTAINER_STYLE}",

            div {
                style: "{HEADER_ROW}",
                h3 { style: "margin: 0; font-size: var(--text-md); color: var(--text-primary);", "Roadmap" }
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
                        style: "display: flex; align-items: center; justify-content: center; flex: 1; color: var(--text-secondary);",
                        "Loading roadmap..."
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
                        div { style: "font-size: var(--text-md);", "Roadmap not available" }
                        div { style: "font-size: var(--text-sm); max-width: 360px; text-align: center;",
                            "The roadmap API is not available on this pylon instance."
                        }
                    }
                },
                FetchState::Loaded(store) => {
                    let phases = store.phases();
                    let deps = store.dependencies();
                    let active = store.active_phase();
                    let active_id = active.map(|p| p.id.as_str()).unwrap_or("");

                    if phases.is_empty() {
                        rsx! {
                            div {
                                style: "{PLACEHOLDER_STYLE}",
                                div { style: "font-size: var(--text-md);", "No phases defined" }
                                div { style: "font-size: var(--text-sm);",
                                    "Roadmap phases will appear here when configured."
                                }
                            }
                        }
                    } else {
                        let blocks = build_timeline_blocks(phases, active_id);
                        let dep_lines = build_dependency_lines(phases, deps);
                        let current_zoom = *zoom.read();
                        let selected = *selected_phase.read();

                        // Selected phase detail.
                        let selected_detail: Option<&Phase> = selected.and_then(|idx| phases.get(idx));

                        rsx! {
                            div {
                                style: "flex: 1; overflow: hidden; display: flex; flex-direction: column;",

                                Timeline {
                                    blocks,
                                    dependencies: dep_lines,
                                    zoom: current_zoom,
                                    on_zoom_change: move |z: f64| zoom.set(z),
                                    on_block_click: move |idx: usize| {
                                        let current = *selected_phase.peek();
                                        if current == Some(idx) {
                                            selected_phase.set(None);
                                        } else {
                                            selected_phase.set(Some(idx));
                                        }
                                    },
                                }

                                if let Some(phase) = selected_detail {
                                    div {
                                        style: "{DETAIL_PANEL}",
                                        div {
                                            style: "display: flex; align-items: center; justify-content: space-between; margin-bottom: var(--space-2);",
                                            span {
                                                style: "font-size: var(--text-base); font-weight: var(--weight-semibold); color: var(--text-primary);",
                                                "{phase.name}"
                                            }
                                            span {
                                                style: "font-size: var(--text-xs); color: var(--text-secondary);",
                                                "{phase.start_date} — {phase.end_date}"
                                            }
                                        }
                                        div {
                                            style: "display: flex; align-items: center; gap: var(--space-3); font-size: var(--text-xs); color: var(--text-secondary); margin-bottom: var(--space-2);",
                                            span { "Progress: {phase.progress}%" }
                                            span { "Status: {phase_status_label(phase.status)}" }
                                        }
                                        if !phase.requirements.is_empty() {
                                            div {
                                                style: "font-size: var(--text-xs); color: var(--text-muted); margin-top: var(--space-2);",
                                                "Requirements: {phase.requirements.join(\", \")}"
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

/// Convert phases into timeline blocks with calculated positions.
fn build_timeline_blocks(phases: &[Phase], active_id: &str) -> Vec<TimelineBlock> {
    let date_ranges: Vec<(String, String)> = phases
        .iter()
        .map(|p| (p.start_date.clone(), p.end_date.clone()))
        .collect();

    let positions = phase_positions(&date_ranges, PIXELS_PER_DAY);

    phases
        .iter()
        .zip(positions.iter())
        .map(|(phase, (x, w))| {
            let is_active = phase.id == active_id;
            TimelineBlock {
                id: phase.id.clone(),
                label: phase.name.clone(),
                x: *x,
                width: *w,
                color: phase_status_color(phase.status),
                border_color: phase_border_color(phase.status),
                progress: phase.progress,
                active: is_active,
                detail: format!("{} — {}", phase.start_date, phase.end_date),
            }
        })
        .collect()
}

/// Map phase dependency edges to timeline block index pairs.
fn build_dependency_lines(
    phases: &[Phase],
    deps: &[crate::state::planning::PhaseDependency],
) -> Vec<TimelineDependencyLine> {
    deps.iter()
        .filter_map(|dep| {
            let from_idx = phases.iter().position(|p| p.id == dep.from_phase_id)?;
            let to_idx = phases.iter().position(|p| p.id == dep.to_phase_id)?;
            Some(TimelineDependencyLine { from_idx, to_idx })
        })
        .collect()
}

fn phase_status_label(status: crate::state::planning::PhaseStatus) -> &'static str {
    use crate::state::planning::PhaseStatus;
    match status {
        PhaseStatus::Planned => "Planned",
        PhaseStatus::Active => "Active",
        PhaseStatus::Completed => "Completed",
    }
}
