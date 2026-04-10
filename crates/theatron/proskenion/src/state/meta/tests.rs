use super::*;

#[test]
fn detect_anomaly_requires_minimum_data() {
    assert!(detect_anomaly("a", "m", &[]).is_none());
    assert!(detect_anomaly("a", "m", &[1.0, 2.0]).is_none());
}

#[test]
fn detect_anomaly_flags_spike() {
    // WHY: 10 values near 5.0, then a spike to 20.0 should trigger.
    let mut values = vec![5.0; 10];
    values.push(20.0);
    let anomaly = detect_anomaly("steward", "error rate", &values);
    assert!(anomaly.is_some(), "spike should be detected");
    let a = anomaly.expect("checked above");
    assert_eq!(a.agent_name, "steward");
    assert_eq!(a.direction, TrendDirection::Up);
    assert!(a.deviation_pct > 100.0);
}

#[test]
fn detect_anomaly_ignores_normal_variation() {
    let values = vec![5.0, 5.1, 4.9, 5.0, 5.2, 4.8, 5.0];
    assert!(
        detect_anomaly("a", "m", &values).is_none(),
        "normal variation should not trigger"
    );
}

#[test]
fn detect_anomaly_constant_series() {
    let values = vec![3.0, 3.0, 3.0, 3.0];
    assert!(
        detect_anomaly("a", "m", &values).is_none(),
        "zero variance should not trigger"
    );
}

#[test]
fn compute_health_score_perfect() {
    let score = compute_health_score(1.0, 0.0, 0.0);
    assert!((score - 1.0).abs() < f64::EPSILON, "perfect health = 1.0");
}

#[test]
fn compute_health_score_worst() {
    let score = compute_health_score(0.0, 1.0, 1.0);
    assert!(score.abs() < f64::EPSILON, "worst health = 0.0");
}

#[test]
fn compute_health_score_mixed() {
    let score = compute_health_score(0.8, 0.2, 0.1);
    // 0.8*0.4 + 0.8*0.3 + 0.9*0.3 = 0.32 + 0.24 + 0.27 = 0.83
    assert!(
        (score - 0.83).abs() < 0.01,
        "mixed score should be ~0.83, got {score}"
    );
}

#[test]
fn compute_health_score_clamps_inputs() {
    let score = compute_health_score(1.5, -0.1, 2.0);
    // Clamped: 1.0*0.4 + 1.0*0.3 + (1-1)*0.3 = 0.7
    assert!(
        (score - 0.7).abs() < f64::EPSILON,
        "out-of-range inputs should be clamped"
    );
}

#[test]
fn compute_acceleration_up() {
    let values = vec![10.0, 15.0, 25.0];
    // recent_growth=10, prior_growth=5, diff=5 > 1.0 → Up
    assert_eq!(compute_acceleration(&values), TrendDirection::Up);
}

#[test]
fn compute_acceleration_down() {
    let values = vec![10.0, 25.0, 30.0];
    // recent_growth=5, prior_growth=15, diff=-10 < -1.0 → Down
    assert_eq!(compute_acceleration(&values), TrendDirection::Down);
}

#[test]
fn compute_acceleration_flat() {
    let values = vec![10.0, 15.0, 20.0];
    // recent_growth=5, prior_growth=5, diff=0 → Flat
    assert_eq!(compute_acceleration(&values), TrendDirection::Flat);
}

#[test]
fn compute_acceleration_insufficient_data() {
    assert_eq!(compute_acceleration(&[1.0, 2.0]), TrendDirection::Flat);
}

#[test]
fn build_heatmap_dimensions() {
    let cells = build_heatmap(&[]);
    assert_eq!(cells.len(), 168, "7 days * 24 hours = 168 cells");
}

#[test]
fn build_heatmap_counts() {
    let timestamps = vec![(0, 9), (0, 9), (0, 10), (6, 23)];
    let cells = build_heatmap(&timestamps);
    let mon_9 = cells.iter().find(|c| c.day == 0 && c.hour == 9).expect("cell exists");
    assert_eq!(mon_9.count, 2);
    let sun_23 = cells.iter().find(|c| c.day == 6 && c.hour == 23).expect("cell exists");
    assert_eq!(sun_23.count, 1);
}

#[test]
fn build_heatmap_ignores_out_of_range() {
    let timestamps = vec![(7, 0), (0, 24), (255, 255)];
    let cells = build_heatmap(&timestamps);
    let total: u32 = cells.iter().map(|c| c.count).sum();
    assert_eq!(total, 0, "out-of-range timestamps must be ignored");
}

#[test]
fn cost_per_entity_zero_entities() {
    assert!(cost_per_entity(100.0, 0).abs() < f64::EPSILON);
}

#[test]
fn cost_per_entity_normal() {
    let cpe = cost_per_entity(50.0, 100);
    assert!((cpe - 0.5).abs() < f64::EPSILON);
}

#[test]
fn tokens_per_entity_zero_entities() {
    assert!(tokens_per_entity(1000, 0).abs() < f64::EPSILON);
}

#[test]
fn depth_distribution_classify() {
    assert_eq!(DepthDistribution::classify(0), "short");
    assert_eq!(DepthDistribution::classify(9), "short");
    assert_eq!(DepthDistribution::classify(10), "medium");
    assert_eq!(DepthDistribution::classify(49), "medium");
    assert_eq!(DepthDistribution::classify(50), "long");
    assert_eq!(DepthDistribution::classify(500), "long");
}

#[test]
fn depth_distribution_pct_empty() {
    let d = DepthDistribution::default();
    assert!(d.pct(0).abs() < f64::EPSILON);
}

#[test]
fn compute_average_empty() {
    assert!(compute_average(&[]).abs() < f64::EPSILON);
}

#[test]
fn compute_average_normal() {
    let avg = compute_average(&[10.0, 20.0, 30.0]);
    assert!((avg - 20.0).abs() < f64::EPSILON);
}

#[test]
fn compute_ratio_zero_denominator() {
    assert!(compute_ratio(10, 0).abs() < f64::EPSILON);
}

#[test]
fn compute_ratio_normal() {
    let r = compute_ratio(30, 10);
    assert!((r - 3.0).abs() < f64::EPSILON);
}

#[test]
fn generate_recommendations_healthy() {
    let recs = generate_recommendations(0.05, 0.1, 0.8, 5.0);
    assert!(recs.is_empty(), "healthy metrics should produce no recommendations");
}

#[test]
fn generate_recommendations_unhealthy() {
    let recs = generate_recommendations(0.25, 0.3, 0.3, 0.5);
    assert_eq!(recs.len(), 4, "all thresholds exceeded");
}

#[test]
fn heatmap_color_zero() {
    assert_eq!(heatmap_color(0, 10), "#1a1a2e");
    assert_eq!(heatmap_color(0, 0), "#1a1a2e");
}

#[test]
fn heatmap_color_high() {
    assert_eq!(heatmap_color(10, 10), "#22c55e");
}

#[test]
fn anomaly_message_format() {
    let a = Anomaly {
        agent_name: "steward".to_string(),
        metric_name: "error rate".to_string(),
        current_value: 12.0,
        baseline_mean: 5.5,
        deviation_pct: 118.0,
        direction: TrendDirection::Up,
    };
    let msg = a.message();
    assert!(msg.contains("steward"));
    assert!(msg.contains("error rate"));
    assert!(msg.contains("increased"));
    assert!(msg.contains("118%"));
}

#[test]
fn radar_axes_bounded() {
    let card = AgentScorecard {
        agent_id: "test".to_string(),
        agent_name: "Test".to_string(),
        avg_tokens_per_response: 500.0,
        tool_calls_per_session: 10.0,
        tool_success_rate: 0.95,
        distillation_frequency: 2.0,
        avg_context_before_distill: 80.0,
        messages_per_session: 25.0,
        sessions_per_day: 3.0,
        errors_per_session: 0.5,
    };
    let axes = card.radar_axes();
    for (i, &v) in axes.iter().enumerate() {
        assert!(
            (0.0..=1.0).contains(&v),
            "axis {i} = {v} out of bounds"
        );
    }
}
