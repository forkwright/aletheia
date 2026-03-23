//! Temporal graph view: timeline scrubber over knowledge graph snapshots.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::components::graph::GraphCanvas;
use crate::components::timeline_scrubber::TimelineScrubber;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::graph::{
    CommunityFilter, GraphData, GraphTimelineState, GraphViewport, SimulationParams, TimelineStep,
    initial_positions, resolve_edge_indices, simulation_step,
};

const TIMELINE_CONTAINER: &str = "\
    display: flex; \
    flex-direction: column; \
    flex: 1; \
    gap: 8px; \
    min-height: 0;\
";

const CONTROLS_ROW: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 8px 12px; \
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px;\
";

const CTRL_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 4px; \
    padding: 4px 10px; \
    font-size: 12px; \
    cursor: pointer;\
";

const CTRL_BTN_ACTIVE: &str = "\
    background: #3a3a5a; \
    color: #e0e0e0; \
    border: 1px solid #9A7B4F; \
    border-radius: 4px; \
    padding: 4px 10px; \
    font-size: 12px; \
    cursor: pointer;\
";

const STATUS_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    color: #888; \
    font-size: 14px;\
";

const STEP_SELECT: &str = "\
    background: #12110f; \
    border: 1px solid #333; \
    border-radius: 4px; \
    padding: 4px 8px; \
    color: #e0e0e0; \
    font-size: 12px; \
    cursor: pointer;\
";

/// Graph timeline view with date range scrubber and playback.
#[component]
pub(crate) fn GraphTimelineView() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut fetch_state = use_signal(|| FetchState::<GraphData>::Loading);
    let mut graph_data = use_signal(GraphData::default);
    let mut positions = use_signal(Vec::new);
    let mut edge_indices = use_signal(Vec::new);
    let mut viewport = use_signal(GraphViewport::default);
    let filter = use_signal(CommunityFilter::default);
    let mut timeline = use_signal(GraphTimelineState::default);
    let mut simulating = use_signal(|| false);

    let mut fetch_snapshot = move |since: Option<String>, until: Option<String>| {
        let cfg = config.read().clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let mut url = format!("{}/api/memory/graph", cfg.server_url.trim_end_matches('/'));

            let mut params = Vec::new();
            if let Some(ref s) = since {
                params.push(format!("since={s}"));
            }
            if let Some(ref u) = until {
                params.push(format!("until={u}"));
            }
            if !params.is_empty() {
                url.push('?');
                url.push_str(&params.join("&"));
            }

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    match resp.json::<GraphData>().await {
                        Ok(data) => {
                            let ei = resolve_edge_indices(&data.nodes, &data.edges);
                            let pos = initial_positions(data.nodes.len());

                            // Derive date range from node timestamps.
                            let dates: Vec<&str> = data
                                .nodes
                                .iter()
                                .filter_map(|n| n.created_at.as_deref())
                                .collect();
                            let min_d = dates
                                .iter()
                                .min()
                                .map(|s| s.chars().take(10).collect::<String>());
                            let max_d = dates
                                .iter()
                                .max()
                                .map(|s| s.chars().take(10).collect::<String>());

                            {
                                let mut ts = timeline.write();
                                if ts.min_date.is_none() {
                                    ts.min_date = min_d;
                                }
                                if ts.max_date.is_none() {
                                    ts.max_date = max_d;
                                }
                            }

                            edge_indices.set(ei);
                            positions.set(pos);
                            graph_data.set(data);
                            fetch_state.set(FetchState::Loaded(GraphData::default()));
                            simulating.set(true);
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

    // Force simulation coroutine.
    use_effect(move || {
        if !*simulating.read() {
            return;
        }

        spawn(async move {
            let params = SimulationParams::default();
            for _ in 0..params.max_iterations {
                if !*simulating.read() {
                    break;
                }
                let mut pos = positions.read().clone();
                let ei = edge_indices.read().clone();
                let mut energy = 0.0;
                for _ in 0..10 {
                    energy = simulation_step(&mut pos, &ei, &params);
                    if energy < params.energy_threshold {
                        break;
                    }
                }
                positions.set(pos);
                if energy < params.energy_threshold {
                    simulating.set(false);
                    let final_pos = positions.read();
                    viewport.set(GraphViewport::fit_to_positions(&final_pos, 800.0, 600.0));
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(16)).await;
            }
            simulating.set(false);
        });
    });

    // Playback auto-advance.
    use_effect(move || {
        let ts = timeline.read();
        if !ts.playing {
            return;
        }
        let step = ts.step;
        let until_val = ts.until.clone();
        let max_val = ts.max_date.clone();

        spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            let (still_playing, since) = {
                let ts = timeline.read();
                (ts.playing, ts.since.clone())
            };
            if !still_playing {
                return;
            }

            if let Some(ref current_until) = until_val {
                let next = step.advance(current_until);
                let at_end = match &max_val {
                    Some(max) => next.as_str() > max.as_str(),
                    None => false,
                };
                if at_end {
                    timeline.write().playing = false;
                    return;
                }
                timeline.write().until = Some(next.clone());
                fetch_snapshot(since, Some(next));
            }
        });
    });

    // Initial fetch.
    use_effect(move || {
        fetch_snapshot(None, None);
    });

    let ts = timeline.read();
    let min_date = ts
        .min_date
        .clone()
        .unwrap_or_else(|| "2024-01-01".to_string());
    let max_date = ts
        .max_date
        .clone()
        .unwrap_or_else(|| "2026-12-31".to_string());
    let since_val = ts.since.clone().unwrap_or_else(|| min_date.clone());
    let until_val = ts.until.clone().unwrap_or_else(|| max_date.clone());
    let is_playing = ts.playing;
    let current_step = ts.step;
    drop(ts);

    let current_data = graph_data.read();

    rsx! {
        div {
            style: "{TIMELINE_CONTAINER}",

            // Playback controls
            div {
                style: "{CONTROLS_ROW}",

                button {
                    style: if is_playing { CTRL_BTN_ACTIVE } else { CTRL_BTN },
                    onclick: move |_| {
                        let mut ts = timeline.write();
                        ts.playing = !ts.playing;
                        if ts.playing && ts.since.is_none() {
                            ts.since = ts.min_date.clone();
                            ts.until = ts.min_date.clone();
                        }
                    },
                    if is_playing { "Pause" } else { "Play" }
                }

                select {
                    style: "{STEP_SELECT}",
                    value: "{current_step.label()}",
                    onchange: move |evt: Event<FormData>| {
                        let step = match evt.value().as_str() {
                            "Day" => TimelineStep::Day,
                            "Week" => TimelineStep::Week,
                            "Month" => TimelineStep::Month,
                            _ => TimelineStep::Week,
                        };
                        timeline.write().step = step;
                    },
                    for s in TimelineStep::ALL {
                        option {
                            key: "{s.label()}",
                            value: "{s.label()}",
                            selected: current_step == *s,
                            "{s.label()}"
                        }
                    }
                }

                if *simulating.read() {
                    span { style: "font-size: 11px; color: #f59e0b;", "Simulating..." }
                }

                span {
                    style: "font-size: 11px; color: #666; margin-left: auto;",
                    "{current_data.nodes.len()} nodes, {current_data.edges.len()} edges"
                }
            }

            // Graph canvas
            match &*fetch_state.read() {
                FetchState::Loading => rsx! {
                    div { style: "{STATUS_STYLE}", "Loading snapshot..." }
                },
                FetchState::Error(err) => rsx! {
                    div { style: "{STATUS_STYLE} color: #ef4444;", "Error: {err}" }
                },
                FetchState::Loaded(_) => rsx! {
                    GraphCanvas {
                        nodes: current_data.nodes.clone(),
                        positions: positions.read().clone(),
                        edges: current_data.edges.clone(),
                        edge_indices: edge_indices.read().clone(),
                        viewport: viewport.read().clone(),
                        filter: filter.read().clone(),
                        drift_node_ids: Vec::new(),
                        show_drift: false,
                        on_node_click: move |_id: String| {},
                        on_viewport_change: move |vp: GraphViewport| {
                            viewport.set(vp);
                        },
                    }
                },
            }

            // Timeline scrubber
            TimelineScrubber {
                min_date: min_date.clone(),
                max_date: max_date.clone(),
                since: since_val.clone(),
                until: until_val.clone(),
                on_since_change: move |date: String| {
                    let until = timeline.read().until.clone();
                    timeline.write().since = Some(date.clone());
                    fetch_snapshot(Some(date), until);
                },
                on_until_change: move |date: String| {
                    let since = timeline.read().since.clone();
                    timeline.write().until = Some(date.clone());
                    fetch_snapshot(since, Some(date));
                },
            }
        }
    }
}
