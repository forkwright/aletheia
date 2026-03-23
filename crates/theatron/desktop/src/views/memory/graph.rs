//! Knowledge graph visualization: 2D force-directed layout with community coloring.

use dioxus::prelude::*;

use crate::api::client::authenticated_client;
use crate::components::graph::GraphCanvas;
use crate::state::connection::ConnectionConfig;
use crate::state::fetch::FetchState;
use crate::state::graph::{
    CommunityFilter, GraphData, GraphViewport, SimulationParams, community_color,
    initial_positions, resolve_edge_indices, simulation_step,
};

const GRAPH_CONTAINER: &str = "\
    display: flex; \
    flex-direction: column; \
    flex: 1; \
    gap: 8px; \
    min-height: 0;\
";

const TOOLBAR_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    gap: 8px; \
    padding: 8px 12px; \
    background: #1a1a2e; \
    border: 1px solid #333; \
    border-radius: 8px; \
    flex-wrap: wrap;\
";

const TOOL_BTN: &str = "\
    background: #2a2a4a; \
    color: #e0e0e0; \
    border: 1px solid #444; \
    border-radius: 4px; \
    padding: 4px 10px; \
    font-size: 12px; \
    cursor: pointer;\
";

const TOOL_BTN_ACTIVE: &str = "\
    background: #3a3a5a; \
    color: #e0e0e0; \
    border: 1px solid #9A7B4F; \
    border-radius: 4px; \
    padding: 4px 10px; \
    font-size: 12px; \
    cursor: pointer;\
";

const STAT_LABEL: &str = "\
    font-size: 11px; \
    color: #666;\
";

const STATUS_STYLE: &str = "\
    display: flex; \
    align-items: center; \
    justify-content: center; \
    flex: 1; \
    color: #888; \
    font-size: 14px;\
";

const COMMUNITY_CHIP: &str = "\
    display: inline-flex; \
    align-items: center; \
    gap: 4px; \
    padding: 2px 8px; \
    border-radius: 12px; \
    font-size: 11px; \
    cursor: pointer; \
    border: 1px solid #444;\
";

/// Knowledge graph view with force-directed layout and controls.
#[component]
pub(crate) fn GraphView() -> Element {
    let config: Signal<ConnectionConfig> = use_context();
    let mut fetch_state = use_signal(|| FetchState::<GraphData>::Loading);
    let mut graph_data = use_signal(GraphData::default);
    let mut positions = use_signal(Vec::new);
    let mut edge_indices = use_signal(Vec::new);
    let mut viewport = use_signal(GraphViewport::default);
    let mut filter = use_signal(CommunityFilter::default);
    let mut simulating = use_signal(|| false);
    let mut show_drift = use_signal(|| false);
    let drift_node_ids: Vec<String> = Vec::new();

    let mut do_fetch = move || {
        let cfg = config.read().clone();
        fetch_state.set(FetchState::Loading);

        spawn(async move {
            let client = authenticated_client(&cfg);
            let url = format!("{}/api/memory/graph", cfg.server_url.trim_end_matches('/'));

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => match resp.json::<GraphData>().await {
                    Ok(data) => {
                        let ei = resolve_edge_indices(&data.nodes, &data.edges);
                        let pos = initial_positions(data.nodes.len());
                        edge_indices.set(ei);
                        positions.set(pos);
                        graph_data.set(data);
                        fetch_state.set(FetchState::Loaded(GraphData::default()));
                        simulating.set(true);
                    }
                    Err(e) => {
                        fetch_state.set(FetchState::Error(format!("parse error: {e}")));
                    }
                },
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

    // WHY: Run force simulation in a background coroutine with frame budget.
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
                    // Auto-fit viewport after stabilization.
                    let final_pos = positions.read();
                    let vp = GraphViewport::fit_to_positions(&final_pos, 800.0, 600.0);
                    viewport.set(vp);
                    break;
                }

                tokio::time::sleep(std::time::Duration::from_millis(16)).await;
            }
            simulating.set(false);
        });
    });

    use_effect(move || {
        do_fetch();
    });

    let current_data = graph_data.read();
    let communities: Vec<u32> = {
        let mut cs: Vec<u32> = current_data
            .nodes
            .iter()
            .map(|n| n.community_id)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        cs.sort();
        cs
    };
    let node_count = current_data.nodes.len();
    let edge_count = current_data.edges.len();

    rsx! {
        div {
            style: "{GRAPH_CONTAINER}",

            // Controls toolbar
            div {
                style: "{TOOLBAR_STYLE}",

                button {
                    style: "{TOOL_BTN}",
                    onclick: move |_| {
                        let pos = positions.read();
                        let vp = GraphViewport::fit_to_positions(&pos, 800.0, 600.0);
                        viewport.set(vp);
                    },
                    "Fit"
                }

                button {
                    style: "{TOOL_BTN}",
                    onclick: move |_| {
                        let data = graph_data.read();
                        positions.set(initial_positions(data.nodes.len()));
                        simulating.set(true);
                    },
                    "Reset Layout"
                }

                button {
                    style: if filter.read().color_by_agent { TOOL_BTN_ACTIVE } else { TOOL_BTN },
                    onclick: move |_| {
                        let mut f = filter.write();
                        f.color_by_agent = !f.color_by_agent;
                    },
                    "Agent Overlay"
                }

                button {
                    style: if *show_drift.read() { TOOL_BTN_ACTIVE } else { TOOL_BTN },
                    onclick: move |_| {
                        let current = *show_drift.read();
                        show_drift.set(!current);
                    },
                    "Show Drift"
                }

                button {
                    style: "{TOOL_BTN}",
                    onclick: move |_| do_fetch(),
                    "Refresh"
                }

                span { style: "{STAT_LABEL}", "{node_count} nodes" }
                span { style: "{STAT_LABEL}", "{edge_count} edges" }

                if *simulating.read() {
                    span { style: "font-size: 11px; color: #f59e0b;", "Simulating..." }
                }
            }

            // Community filter chips
            if !communities.is_empty() {
                div {
                    style: "display: flex; gap: 4px; flex-wrap: wrap; padding: 0 4px;",
                    for cid in &communities {
                        {
                            let community_id = *cid;
                            let color = community_color(community_id);
                            let visible = filter.read().is_visible(community_id);
                            let opacity = if visible { "1.0" } else { "0.4" };
                            rsx! {
                                span {
                                    key: "c-{community_id}",
                                    style: "{COMMUNITY_CHIP} opacity: {opacity};",
                                    onclick: move |_| {
                                        filter.write().toggle(community_id);
                                    },
                                    span {
                                        style: "width: 8px; height: 8px; border-radius: 50%; background: {color}; display: inline-block;",
                                    }
                                    "C{community_id}"
                                }
                            }
                        }
                    }
                }
            }

            // Graph canvas or loading state
            match &*fetch_state.read() {
                FetchState::Loading => rsx! {
                    div { style: "{STATUS_STYLE}", "Loading graph..." }
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
                        drift_node_ids: drift_node_ids.clone(),
                        show_drift: *show_drift.read(),
                        on_node_click: move |id: String| {
                            // NOTE: Navigation to entity detail would go here.
                            // Requires memory explorer route with entity ID parameter.
                            tracing::debug!(entity_id = %id, "graph node clicked");
                        },
                        on_viewport_change: move |vp: GraphViewport| {
                            viewport.set(vp);
                        },
                    }
                },
            }
        }
    }
}
