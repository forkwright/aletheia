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
