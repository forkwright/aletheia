//! SVG-based knowledge graph canvas with zoom, pan, and node interaction.

use dioxus::prelude::*;

use crate::state::graph::{
    CommunityFilter, GraphEdge, GraphNode, GraphViewport, NodePosition, community_color,
    label_threshold, node_radius, visible_node_indices,
};

const CANVAS_CONTAINER: &str = "\
    position: relative; \
    overflow: hidden; \
    flex: 1; \
    background: #0f0f1a; \
    border: 1px solid #2a2a3a; \
    border-radius: 8px; \
    cursor: grab; \
    min-height: 300px;\
";

const TOOLTIP_STYLE: &str = "\
    position: absolute; \
    background: #1a1a2e; \
    border: 1px solid #444; \
    border-radius: 6px; \
    padding: 8px 12px; \
    pointer-events: none; \
    z-index: 10; \
    min-width: 140px;\
";

const TOOLTIP_TITLE: &str = "\
    font-size: 13px; \
    font-weight: bold; \
    color: #e0e0e0; \
    margin-bottom: 4px;\
";

const TOOLTIP_ROW: &str = "\
    font-size: 11px; \
    color: #888; \
    line-height: 1.5;\
";

/// Maximum number of rendered nodes for performance.
const MAX_RENDERED_NODES: usize = 500;

/// Knowledge graph SVG canvas with zoom, pan, frustum culling, and tooltips.
#[component]
pub(crate) fn GraphCanvas(
    nodes: Vec<GraphNode>,
    positions: Vec<NodePosition>,
    edges: Vec<GraphEdge>,
    edge_indices: Vec<(usize, usize)>,
    viewport: GraphViewport,
    filter: CommunityFilter,
    drift_node_ids: Vec<String>,
    show_drift: bool,
    on_node_click: EventHandler<String>,
    on_viewport_change: EventHandler<GraphViewport>,
) -> Element {
    let mut hovered_idx = use_signal(|| Option::<usize>::None);
    let mut dragging = use_signal(|| false);
    let mut drag_start = use_signal(|| (0.0_f64, 0.0_f64));
    let mut drag_pan_start = use_signal(|| (0.0_f64, 0.0_f64));

    let visible = visible_node_indices(&nodes, &positions, &viewport, &filter, MAX_RENDERED_NODES);
    let lod_threshold = label_threshold(viewport.zoom);
    let vp = viewport.clone();
    let vp_wheel = viewport.clone();
    let vp_down = viewport.clone();

    rsx! {
        div {
            style: "{CANVAS_CONTAINER}",

            onwheel: move |evt| {
                let delta = evt.delta();
                let dy = delta.strip_units().y;
                let factor = if dy > 0.0 { 0.9 } else { 1.1 };
                let new_zoom = (vp_wheel.zoom * factor).clamp(0.1, 5.0);
                on_viewport_change.call(GraphViewport {
                    zoom: new_zoom,
                    ..vp_wheel.clone()
                });
            },

            onmousedown: move |evt| {
                dragging.set(true);
                let coords = evt.page_coordinates();
                drag_start.set((coords.x, coords.y));
                drag_pan_start.set((vp_down.pan_x, vp_down.pan_y));
            },

            onmousemove: move |evt| {
                if *dragging.read() {
                    let coords = evt.page_coordinates();
                    let (sx, sy) = *drag_start.read();
                    let (px, py) = *drag_pan_start.read();
                    let dx = (coords.x - sx) / vp.zoom.max(0.01);
                    let dy = (coords.y - sy) / vp.zoom.max(0.01);
                    on_viewport_change.call(GraphViewport {
                        pan_x: px - dx,
                        pan_y: py - dy,
                        ..vp.clone()
                    });
                }
            },

            onmouseup: move |_| {
                dragging.set(false);
            },

            onmouseleave: move |_| {
                dragging.set(false);
            },

            svg {
                width: "100%",
                height: "100%",

                // Edges (render behind nodes)
                for (ei , &(src, tgt)) in edge_indices.iter().enumerate() {
                    if src < positions.len() && tgt < positions.len() {
                        {
                            let (x1, y1) = viewport.world_to_screen(positions[src].x, positions[src].y);
                            let (x2, y2) = viewport.world_to_screen(positions[tgt].x, positions[tgt].y);
                            let conf = edges.get(ei).map(|e| e.confidence).unwrap_or(0.5);
                            let thickness = (conf * 3.0).clamp(0.5, 3.0);
                            let src_community = nodes.get(src).map(|n| n.community_id).unwrap_or(0);
                            let edge_color = community_color(src_community);
                            rsx! {
                                line {
                                    key: "e-{ei}",
                                    x1: "{x1}",
                                    y1: "{y1}",
                                    x2: "{x2}",
                                    y2: "{y2}",
                                    stroke: "{edge_color}",
                                    stroke_width: "{thickness}",
                                    opacity: "0.3",
                                }
                            }
                        }
                    }
                }

                // Nodes
                for &ni in &visible {
                    if ni < nodes.len() && ni < positions.len() {
                        {
                            let node = &nodes[ni];
                            let pos = &positions[ni];
                            let (sx, sy) = viewport.world_to_screen(pos.x, pos.y);
                            let r = node_radius(node.pagerank) * viewport.zoom.clamp(0.3, 3.0);
                            let color = if filter.color_by_agent {
                                agent_color(node.agent_id.as_deref())
                            } else {
                                community_color(node.community_id)
                            };
                            let is_drift = show_drift && drift_node_ids.contains(&node.id);
                            let stroke = if is_drift { "#B8923B" } else { "#1a1a2e" };
                            let sw = if is_drift { "2.5" } else { "1" };
                            let node_id = node.id.clone();
                            rsx! {
                                circle {
                                    key: "n-{ni}",
                                    cx: "{sx}",
                                    cy: "{sy}",
                                    r: "{r}",
                                    fill: "{color}",
                                    stroke: "{stroke}",
                                    stroke_width: "{sw}",
                                    onmouseenter: move |_| hovered_idx.set(Some(ni)),
                                    onmouseleave: move |_| hovered_idx.set(None),
                                    onclick: move |evt| {
                                        evt.stop_propagation();
                                        on_node_click.call(node_id.clone());
                                    },
                                }
                            }
                        }
                    }
                }
            }

            // HTML labels for high-PageRank nodes
            for &ni in &visible {
                if ni < nodes.len() && ni < positions.len() && nodes[ni].pagerank > lod_threshold {
                    {
                        let node = &nodes[ni];
                        let pos = &positions[ni];
                        let (sx, sy) = viewport.world_to_screen(pos.x, pos.y);
                        let r = node_radius(node.pagerank) * viewport.zoom.clamp(0.3, 3.0);
                        let label_y = sy - r - 6.0;
                        rsx! {
                            div {
                                key: "lbl-{ni}",
                                style: "position: absolute; left: {sx}px; top: {label_y}px; transform: translateX(-50%); font-size: 11px; color: #e0e0e0; pointer-events: none; white-space: nowrap; text-shadow: 0 1px 3px #000;",
                                "{node.label}"
                            }
                        }
                    }
                }
            }

            // Tooltip on hover
            if let Some(hi) = *hovered_idx.read() {
                if hi < nodes.len() && hi < positions.len() {
                    {
                        let node = &nodes[hi];
                        let pos = &positions[hi];
                        let (sx, sy) = viewport.world_to_screen(pos.x, pos.y);
                        let tip_y = sy - 80.0;
                        rsx! {
                            div {
                                style: "{TOOLTIP_STYLE} left: {sx}px; top: {tip_y}px; transform: translateX(-50%);",
                                div { style: "{TOOLTIP_TITLE}", "{node.label}" }
                                div { style: "{TOOLTIP_ROW}", "Type: {node.entity_type}" }
                                div { style: "{TOOLTIP_ROW}", "Confidence: {format_pct(node.confidence)}" }
                                div { style: "{TOOLTIP_ROW}", "Relationships: {node.relationship_count}" }
                                if let Some(ref agent) = node.agent_id {
                                    div { style: "{TOOLTIP_ROW}", "Agent: {agent}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn format_pct(value: f64) -> String {
    format!("{:.0}%", value * 100.0)
}

/// Color by agent ID, cycling through a distinct palette.
fn agent_color(agent_id: Option<&str>) -> &'static str {
    const AGENT_PALETTE: &[&str] = &[
        "#06b6d4", "#f59e0b", "#8b5cf6", "#ec4899", "#22c55e", "#ef4444", "#7a7aff", "#84cc16",
    ];
    match agent_id {
        Some(id) => {
            let hash: usize = id.bytes().map(|b| b as usize).sum();
            let idx = hash % AGENT_PALETTE.len();
            // SAFETY: idx always < len due to modulo.
            AGENT_PALETTE[idx]
        }
        None => "#555",
    }
}
