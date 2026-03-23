//! Knowledge graph state: nodes, edges, force simulation, viewport, community filters, drift.

use std::collections::HashSet;

use serde::Deserialize;

// === API response types ===

/// A node in the knowledge graph from the API.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct GraphNode {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) label: String,
    #[serde(default)]
    pub(crate) entity_type: String,
    #[serde(default)]
    pub(crate) confidence: f64,
    #[serde(default)]
    pub(crate) pagerank: f64,
    #[serde(default)]
    pub(crate) community_id: u32,
    #[serde(default)]
    pub(crate) agent_id: Option<String>,
    #[serde(default)]
    pub(crate) created_at: Option<String>,
    #[serde(default)]
    pub(crate) relationship_count: u32,
}

impl Default for GraphNode {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            entity_type: String::new(),
            confidence: 0.0,
            pagerank: 0.0,
            community_id: 0,
            agent_id: None,
            created_at: None,
            relationship_count: 0,
        }
    }
}

/// An edge in the knowledge graph from the API.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct GraphEdge {
    #[serde(default)]
    pub(crate) source: String,
    #[serde(default)]
    pub(crate) target: String,
    #[serde(default)]
    pub(crate) relationship: String,
    #[serde(default)]
    pub(crate) confidence: f64,
}

/// Complete graph payload from `/api/memory/graph`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub(crate) struct GraphData {
    #[serde(default)]
    pub(crate) nodes: Vec<GraphNode>,
    #[serde(default)]
    pub(crate) edges: Vec<GraphEdge>,
}

// === Drift analysis types ===

/// Full drift analysis from `/api/memory/graph/drift`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub(crate) struct DriftAnalysis {
    #[serde(default)]
    pub(crate) orphan_entities: Vec<OrphanEntity>,
    #[serde(default)]
    pub(crate) low_connectivity: Vec<LowConnectivityWarning>,
    #[serde(default)]
    pub(crate) health_score: HealthScore,
    #[serde(default)]
    pub(crate) recommendations: Vec<Recommendation>,
}

/// Entity with few or no relationships.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct OrphanEntity {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) entity_type: String,
    #[serde(default)]
    pub(crate) created_at: Option<String>,
    #[serde(default)]
    pub(crate) relationship_count: u32,
    #[serde(default)]
    pub(crate) suggested_action: String,
}

/// Entity or community with degrading connectivity.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub(crate) struct LowConnectivityWarning {
    #[serde(default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) current_count: u32,
    #[serde(default)]
    pub(crate) previous_count: u32,
    #[serde(default)]
    pub(crate) trend: String,
}

/// Overall graph health metrics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub(crate) struct HealthScore {
    #[serde(default)]
    pub(crate) overall: u32,
    #[serde(default)]
    pub(crate) connectivity: u32,
    #[serde(default)]
    pub(crate) community_distribution: u32,
    #[serde(default)]
    pub(crate) orphan_ratio: u32,
}

/// AI-generated recommendation for graph health improvement.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(crate) struct Recommendation {
    #[serde(default)]
    pub(crate) message: String,
    #[serde(default)]
    pub(crate) category: String,
}

// === Force simulation types ===

/// Position and velocity of a node in the force simulation.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct NodePosition {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) vx: f64,
    pub(crate) vy: f64,
    pub(crate) pinned: bool,
}

/// Parameters controlling the force simulation behavior.
pub(crate) struct SimulationParams {
    pub(crate) repulsion: f64,
    pub(crate) attraction: f64,
    pub(crate) rest_length: f64,
    pub(crate) gravity: f64,
    pub(crate) damping: f64,
    pub(crate) energy_threshold: f64,
    pub(crate) max_iterations: u32,
}

impl Default for SimulationParams {
    fn default() -> Self {
        Self {
            repulsion: 5000.0,
            attraction: 0.01,
            rest_length: 100.0,
            gravity: 0.01,
            damping: 0.9,
            energy_threshold: 0.01,
            max_iterations: 300,
        }
    }
}

/// Resolve edge source/target IDs to node indices for simulation.
#[must_use]
pub(crate) fn resolve_edge_indices(
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> Vec<(usize, usize)> {
    let id_to_idx: std::collections::HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.as_str(), i))
        .collect();

    edges
        .iter()
        .filter_map(|e| {
            let src = *id_to_idx.get(e.source.as_str())?;
            let tgt = *id_to_idx.get(e.target.as_str())?;
            Some((src, tgt))
        })
        .collect()
}

/// Initialize positions for nodes in a circular layout.
#[must_use]
pub(crate) fn initial_positions(count: usize) -> Vec<NodePosition> {
    let mut positions = Vec::with_capacity(count);
    for i in 0..count {
        let angle = 2.0 * std::f64::consts::PI * (i as f64) / (count.max(1) as f64);
        let radius = 200.0 + (i as f64 * 3.0);
        positions.push(NodePosition {
            x: angle.cos() * radius,
            y: angle.sin() * radius,
            vx: 0.0,
            vy: 0.0,
            pinned: false,
        });
    }
    positions
}

/// Run one step of the force simulation. Returns the total kinetic energy.
pub(crate) fn simulation_step(
    positions: &mut [NodePosition],
    edge_indices: &[(usize, usize)],
    params: &SimulationParams,
) -> f64 {
    let n = positions.len();

    // WHY: O(n^2) repulsion is acceptable for graphs under ~1000 nodes.
    // Barnes-Hut would be needed for larger graphs.
    for i in 0..n {
        for j in (i + 1)..n {
            let dx = positions[j].x - positions[i].x;
            let dy = positions[j].y - positions[i].y;
            let dist_sq = (dx * dx + dy * dy).max(1.0);
            let dist = dist_sq.sqrt();
            let force = params.repulsion / dist_sq;
            let fx = force * dx / dist;
            let fy = force * dy / dist;
            if !positions[i].pinned {
                positions[i].vx -= fx;
                positions[i].vy -= fy;
            }
            if !positions[j].pinned {
                positions[j].vx += fx;
                positions[j].vy += fy;
            }
        }
    }

    for &(src, tgt) in edge_indices {
        if src >= n || tgt >= n {
            continue;
        }
        let dx = positions[tgt].x - positions[src].x;
        let dy = positions[tgt].y - positions[src].y;
        let dist = (dx * dx + dy * dy).sqrt().max(1.0);
        let force = params.attraction * (dist - params.rest_length);
        let fx = force * dx / dist;
        let fy = force * dy / dist;
        if !positions[src].pinned {
            positions[src].vx += fx;
            positions[src].vy += fy;
        }
        if !positions[tgt].pinned {
            positions[tgt].vx -= fx;
            positions[tgt].vy -= fy;
        }
    }

    let mut energy = 0.0;
    for pos in positions.iter_mut() {
        if !pos.pinned {
            pos.vx -= params.gravity * pos.x;
            pos.vy -= params.gravity * pos.y;
            pos.vx *= params.damping;
            pos.vy *= params.damping;
            pos.x += pos.vx;
            pos.y += pos.vy;
            energy += pos.vx * pos.vx + pos.vy * pos.vy;
        }
    }

    energy
}

// === Viewport ===

/// Viewport state for zoom and pan.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GraphViewport {
    pub(crate) zoom: f64,
    pub(crate) pan_x: f64,
    pub(crate) pan_y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl Default for GraphViewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            width: 800.0,
            height: 600.0,
        }
    }
}

impl GraphViewport {
    /// Convert world coordinates to screen coordinates.
    #[must_use]
    pub(crate) fn world_to_screen(&self, wx: f64, wy: f64) -> (f64, f64) {
        let sx = (wx - self.pan_x) * self.zoom + self.width / 2.0;
        let sy = (wy - self.pan_y) * self.zoom + self.height / 2.0;
        (sx, sy)
    }

    /// Check if world coordinates are visible in the viewport with margin.
    #[must_use]
    pub(crate) fn is_visible(&self, wx: f64, wy: f64, margin: f64) -> bool {
        let (sx, sy) = self.world_to_screen(wx, wy);
        sx >= -margin && sx <= self.width + margin && sy >= -margin && sy <= self.height + margin
    }

    /// Compute viewport that fits all given positions.
    #[must_use]
    pub(crate) fn fit_to_positions(positions: &[NodePosition], width: f64, height: f64) -> Self {
        if positions.is_empty() {
            return Self {
                width,
                height,
                ..Default::default()
            };
        }

        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;

        for pos in positions {
            min_x = min_x.min(pos.x);
            max_x = max_x.max(pos.x);
            min_y = min_y.min(pos.y);
            max_y = max_y.max(pos.y);
        }

        let world_w = (max_x - min_x).max(1.0) + 100.0;
        let world_h = (max_y - min_y).max(1.0) + 100.0;
        let zoom = (width / world_w).min(height / world_h).clamp(0.1, 5.0);

        Self {
            zoom,
            pan_x: (min_x + max_x) / 2.0,
            pan_y: (min_y + max_y) / 2.0,
            width,
            height,
        }
    }
}

// === Community coloring ===

const COMMUNITY_PALETTE: &[&str] = &[
    "#7a7aff", "#22c55e", "#f59e0b", "#ef4444", "#8b5cf6", "#06b6d4", "#ec4899", "#84cc16",
    "#f97316", "#14b8a6", "#a855f7", "#eab308",
];

/// Color for a community ID from a rotating palette.
#[must_use]
pub(crate) fn community_color(community_id: u32) -> &'static str {
    let idx = community_id as usize % COMMUNITY_PALETTE.len();
    // SAFETY: idx is always < COMMUNITY_PALETTE.len() due to modulo.
    COMMUNITY_PALETTE[idx]
}

/// Node radius based on PageRank score.
#[must_use]
pub(crate) fn node_radius(pagerank: f64) -> f64 {
    (4.0 + pagerank * 160.0).clamp(4.0, 20.0)
}

// === Community filter ===

/// Tracks which communities are visible and the color mode.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CommunityFilter {
    pub(crate) hidden: HashSet<u32>,
    pub(crate) color_by_agent: bool,
}

impl Default for CommunityFilter {
    fn default() -> Self {
        Self {
            hidden: HashSet::new(),
            color_by_agent: false,
        }
    }
}

impl CommunityFilter {
    #[must_use]
    pub(crate) fn is_visible(&self, community_id: u32) -> bool {
        !self.hidden.contains(&community_id)
    }

    pub(crate) fn toggle(&mut self, community_id: u32) {
        if self.hidden.contains(&community_id) {
            self.hidden.remove(&community_id);
        } else {
            self.hidden.insert(community_id);
        }
    }
}

// === Timeline state ===

/// Time step for graph timeline playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TimelineStep {
    Day,
    #[default]
    Week,
    Month,
}

impl TimelineStep {
    pub(crate) const ALL: &[Self] = &[Self::Day, Self::Week, Self::Month];

    #[must_use]
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Day => "Day",
            Self::Week => "Week",
            Self::Month => "Month",
        }
    }

    /// Advance a `YYYY-MM-DD` date string by one step.
    #[must_use]
    pub(crate) fn advance(self, date: &str) -> String {
        let parts: Vec<&str> = date.split('-').collect();
        if parts.len() != 3 {
            return date.to_string();
        }
        let year: i32 = parts[0].parse().unwrap_or(2026);
        let month: u32 = parts[1].parse().unwrap_or(1);
        let day: u32 = parts[2].parse().unwrap_or(1);

        let (y, m, d) = match self {
            Self::Day => advance_days(year, month, day, 1),
            Self::Week => advance_days(year, month, day, 7),
            Self::Month => {
                let nm = if month == 12 { 1 } else { month + 1 };
                let ny = if month == 12 { year + 1 } else { year };
                (ny, nm, day.min(days_in_month(ny, nm)))
            }
        };

        format!("{y:04}-{m:02}-{d:02}")
    }
}

fn advance_days(year: i32, month: u32, day: u32, count: u32) -> (i32, u32, u32) {
    let mut y = year;
    let mut m = month;
    let mut d = day + count;

    while d > days_in_month(y, m) {
        d -= days_in_month(y, m);
        m += 1;
        if m > 12 {
            m = 1;
            y += 1;
        }
    }

    (y, m, d)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Playback and range state for the graph timeline.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct GraphTimelineState {
    pub(crate) since: Option<String>,
    pub(crate) until: Option<String>,
    pub(crate) playing: bool,
    pub(crate) step: TimelineStep,
    pub(crate) min_date: Option<String>,
    pub(crate) max_date: Option<String>,
}

// === Visible node selection (frustum culling + level of detail) ===

/// Select which node indices to render based on viewport visibility and zoom-based LOD.
/// Caps at `max_rendered` nodes, preferring higher PageRank.
#[must_use]
pub(crate) fn visible_node_indices(
    nodes: &[GraphNode],
    positions: &[NodePosition],
    viewport: &GraphViewport,
    filter: &CommunityFilter,
    max_rendered: usize,
) -> Vec<usize> {
    let margin = 50.0;
    let mut candidates: Vec<(usize, f64)> = nodes
        .iter()
        .enumerate()
        .filter(|(i, node)| {
            filter.is_visible(node.community_id)
                && *i < positions.len()
                && viewport.is_visible(positions[*i].x, positions[*i].y, margin)
        })
        .map(|(i, node)| (i, node.pagerank))
        .collect();

    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    candidates.truncate(max_rendered);
    candidates.into_iter().map(|(i, _)| i).collect()
}

/// Health score color based on value.
#[must_use]
pub(crate) fn health_color(score: u32) -> &'static str {
    match score {
        80..=100 => "#22c55e",
        60..=79 => "#f59e0b",
        _ => "#ef4444",
    }
}

/// PageRank threshold for showing labels, scaled by zoom level.
#[must_use]
pub(crate) fn label_threshold(zoom: f64) -> f64 {
    (0.02 / zoom.max(0.1)).clamp(0.001, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulation_converges_for_connected_pair() {
        let edge_indices = vec![(0, 1)];
        let params = SimulationParams::default();
        let mut positions = vec![
            NodePosition {
                x: -100.0,
                y: 0.0,
                ..Default::default()
            },
            NodePosition {
                x: 100.0,
                y: 0.0,
                ..Default::default()
            },
        ];

        let mut last_energy = f64::MAX;
        for _ in 0..200 {
            let energy = simulation_step(&mut positions, &edge_indices, &params);
            // NOTE: energy may briefly increase during initial oscillation.
            last_energy = energy;
        }
        assert!(
            last_energy < 1.0,
            "simulation should converge, energy={last_energy}"
        );
    }

    #[test]
    fn repulsion_pushes_overlapping_nodes_apart() {
        let params = SimulationParams::default();
        let mut positions = vec![
            NodePosition {
                x: 0.0,
                y: 0.0,
                ..Default::default()
            },
            NodePosition {
                x: 1.0,
                y: 0.0,
                ..Default::default()
            },
        ];

        simulation_step(&mut positions, &[], &params);

        let dist_after = (positions[1].x - positions[0].x).abs();
        assert!(
            dist_after > 1.0,
            "overlapping nodes should repel, dist={dist_after}"
        );
    }

    #[test]
    fn attraction_pulls_distant_connected_nodes() {
        let mut params = SimulationParams::default();
        params.repulsion = 0.0;
        params.gravity = 0.0;
        let edge_indices = vec![(0, 1)];
        let mut positions = vec![
            NodePosition {
                x: -500.0,
                y: 0.0,
                ..Default::default()
            },
            NodePosition {
                x: 500.0,
                y: 0.0,
                ..Default::default()
            },
        ];

        simulation_step(&mut positions, &edge_indices, &params);

        let dist_after = (positions[1].x - positions[0].x).abs();
        assert!(
            dist_after < 1000.0,
            "connected nodes should attract, dist={dist_after}"
        );
    }

    #[test]
    fn pinned_nodes_remain_stationary() {
        let params = SimulationParams::default();
        let mut positions = vec![
            NodePosition {
                x: 50.0,
                y: 50.0,
                pinned: true,
                ..Default::default()
            },
            NodePosition {
                x: 200.0,
                y: 0.0,
                ..Default::default()
            },
        ];

        simulation_step(&mut positions, &[], &params);

        assert!(
            (positions[0].x - 50.0).abs() < f64::EPSILON,
            "pinned x should not change"
        );
        assert!(
            (positions[0].y - 50.0).abs() < f64::EPSILON,
            "pinned y should not change"
        );
    }

    #[test]
    fn viewport_world_to_screen_maps_origin_to_center() {
        let vp = GraphViewport::default();
        let (sx, sy) = vp.world_to_screen(0.0, 0.0);
        assert!((sx - 400.0).abs() < f64::EPSILON, "origin maps to center x");
        assert!((sy - 300.0).abs() < f64::EPSILON, "origin maps to center y");
    }

    #[test]
    fn viewport_zoom_scales_offset() {
        let vp = GraphViewport {
            zoom: 2.0,
            ..Default::default()
        };
        let (sx, _) = vp.world_to_screen(100.0, 0.0);
        assert!(
            (sx - 600.0).abs() < f64::EPSILON,
            "zoom 2x: 100*2 + 400 = 600, got {sx}"
        );
    }

    #[test]
    fn viewport_visibility_includes_on_screen() {
        let vp = GraphViewport::default();
        assert!(vp.is_visible(0.0, 0.0, 0.0), "origin should be visible");
        assert!(
            !vp.is_visible(10000.0, 0.0, 0.0),
            "far point should not be visible"
        );
    }

    #[test]
    fn fit_to_positions_centers_on_data() {
        let positions = vec![
            NodePosition {
                x: -100.0,
                y: -50.0,
                ..Default::default()
            },
            NodePosition {
                x: 100.0,
                y: 50.0,
                ..Default::default()
            },
        ];
        let vp = GraphViewport::fit_to_positions(&positions, 800.0, 600.0);
        assert!(
            vp.pan_x.abs() < f64::EPSILON,
            "center x should be 0, got {}",
            vp.pan_x
        );
        assert!(
            vp.pan_y.abs() < f64::EPSILON,
            "center y should be 0, got {}",
            vp.pan_y
        );
        assert!(vp.zoom > 0.0, "zoom should be positive");
    }

    #[test]
    fn community_color_wraps_around_palette() {
        let c0 = community_color(0);
        let c12 = community_color(12);
        assert_eq!(c0, c12, "community 12 wraps to same slot as 0");
    }

    #[test]
    fn node_radius_clamps_to_range() {
        assert!(
            (node_radius(0.0) - 4.0).abs() < f64::EPSILON,
            "min radius is 4"
        );
        assert!(
            (node_radius(1.0) - 20.0).abs() < f64::EPSILON,
            "max radius is 20"
        );
        assert!(
            node_radius(0.05) > 4.0 && node_radius(0.05) < 20.0,
            "mid-range pagerank gives mid-range radius"
        );
    }

    #[test]
    fn community_filter_toggle_hides_and_restores() {
        let mut filter = CommunityFilter::default();
        assert!(filter.is_visible(1), "all visible by default");
        filter.toggle(1);
        assert!(!filter.is_visible(1), "hidden after first toggle");
        filter.toggle(1);
        assert!(filter.is_visible(1), "visible after second toggle");
    }

    #[test]
    fn timeline_step_advance_day() {
        assert_eq!(TimelineStep::Day.advance("2026-03-15"), "2026-03-16");
        assert_eq!(TimelineStep::Day.advance("2026-03-31"), "2026-04-01");
        assert_eq!(TimelineStep::Day.advance("2026-12-31"), "2027-01-01");
    }

    #[test]
    fn timeline_step_advance_week() {
        assert_eq!(TimelineStep::Week.advance("2026-03-15"), "2026-03-22");
        assert_eq!(TimelineStep::Week.advance("2026-03-29"), "2026-04-05");
    }

    #[test]
    fn timeline_step_advance_month() {
        assert_eq!(TimelineStep::Month.advance("2026-01-31"), "2026-02-28");
        assert_eq!(TimelineStep::Month.advance("2026-12-15"), "2027-01-15");
    }

    #[test]
    fn initial_positions_creates_correct_count() {
        let positions = initial_positions(10);
        assert_eq!(positions.len(), 10, "should create 10 positions");
        let zero = initial_positions(0);
        assert!(zero.is_empty(), "zero nodes, zero positions");
    }

    #[test]
    fn visible_node_indices_respects_community_filter() {
        let nodes = vec![
            GraphNode {
                id: "a".into(),
                community_id: 0,
                pagerank: 0.5,
                ..Default::default()
            },
            GraphNode {
                id: "b".into(),
                community_id: 1,
                pagerank: 0.3,
                ..Default::default()
            },
        ];
        let positions = vec![NodePosition::default(), NodePosition::default()];
        let viewport = GraphViewport::default();
        let mut filter = CommunityFilter::default();
        filter.toggle(1);

        let visible = visible_node_indices(&nodes, &positions, &viewport, &filter, 500);
        assert_eq!(visible.len(), 1, "only community 0 should be visible");
        assert_eq!(visible[0], 0);
    }

    #[test]
    fn health_color_returns_correct_band() {
        assert_eq!(health_color(90), "#22c55e", "high score = green");
        assert_eq!(health_color(70), "#f59e0b", "mid score = amber");
        assert_eq!(health_color(30), "#ef4444", "low score = red");
    }

    #[test]
    fn resolve_edge_indices_maps_ids_to_positions() {
        let nodes = vec![
            GraphNode {
                id: "x".into(),
                ..Default::default()
            },
            GraphNode {
                id: "y".into(),
                ..Default::default()
            },
            GraphNode {
                id: "z".into(),
                ..Default::default()
            },
        ];
        let edges = vec![
            GraphEdge {
                source: "x".into(),
                target: "z".into(),
                ..GraphEdge {
                    source: String::new(),
                    target: String::new(),
                    relationship: String::new(),
                    confidence: 0.0,
                }
            },
            GraphEdge {
                source: "a".into(),
                target: "b".into(),
                relationship: String::new(),
                confidence: 0.0,
            },
        ];

        let resolved = resolve_edge_indices(&nodes, &edges);
        assert_eq!(resolved.len(), 1, "only x->z should resolve");
        assert_eq!(resolved[0], (0, 2));
    }
}
