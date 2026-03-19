//! Tests for pure scoring functions (no engine needed).
use super::super::*;

// --- Pure scoring function tests (no engine needed) ---

#[test]
fn pagerank_boost_zero_importance_unchanged() {
    let base = 0.6; // inferred tier
    let result = score_epistemic_tier_with_importance(base, 0.0);
    assert!(
        (result - base).abs() < f64::EPSILON,
        "zero importance should not change score, got {result}"
    );
}

#[test]
fn pagerank_boost_max_importance() {
    let base = 0.6;
    let result = score_epistemic_tier_with_importance(base, 1.0);
    // boost = 1.5, so 0.6 * 1.5 = 0.9
    assert!(
        (result - 0.9).abs() < f64::EPSILON,
        "max importance should give 1.5x boost, got {result}"
    );
}

#[test]
fn pagerank_boost_clamped_to_one() {
    let base = 1.0; // verified tier
    let result = score_epistemic_tier_with_importance(base, 1.0);
    // 1.0 * 1.5 = 1.5, clamped to 1.0
    assert!(
        (result - 1.0).abs() < f64::EPSILON,
        "should clamp to 1.0, got {result}"
    );
}

#[test]
fn pagerank_boost_hub_higher_than_peripheral() {
    let base = 0.6;
    let hub = score_epistemic_tier_with_importance(base, 0.9);
    let peripheral = score_epistemic_tier_with_importance(base, 0.1);
    assert!(
        hub > peripheral,
        "hub ({hub}) should score higher than peripheral ({peripheral})"
    );
}

#[test]
fn pagerank_boost_range() {
    // Importance [0, 1] → boost [1.0, 1.5]
    for i in 0..=10 {
        let importance = f64::from(i) / 10.0;
        let result = score_epistemic_tier_with_importance(0.5, importance);
        let expected_boost = 1.0 + importance * 0.5;
        let expected = (0.5 * expected_boost).min(1.0);
        assert!(
            (result - expected).abs() < 1e-10,
            "importance={importance}: expected {expected}, got {result}"
        );
    }
}

#[test]
fn cluster_floor_same_cluster_no_path() {
    // No direct path (base_hop_score = 0.0), but same cluster → floor 0.3
    let result = score_relationship_proximity_with_cluster(0.0, true);
    assert!(
        (result - 0.3).abs() < f64::EPSILON,
        "same-cluster with no path should get 0.3 floor, got {result}"
    );
}

#[test]
fn cluster_floor_same_cluster_with_path() {
    // Direct neighbor (base = 1.0), same cluster → stays 1.0
    let result = score_relationship_proximity_with_cluster(1.0, true);
    assert!(
        (result - 1.0).abs() < f64::EPSILON,
        "same-cluster direct neighbor should stay 1.0, got {result}"
    );
}

#[test]
fn cluster_floor_different_cluster() {
    // No path, different cluster → stays 0.0
    let result = score_relationship_proximity_with_cluster(0.0, false);
    assert!(
        (result).abs() < f64::EPSILON,
        "different-cluster with no path should stay 0.0, got {result}"
    );
}

#[test]
fn cluster_floor_partial_path() {
    // 2-hop (0.5), same cluster → stays 0.5 (above floor)
    let result = score_relationship_proximity_with_cluster(0.5, true);
    assert!(
        (result - 0.5).abs() < f64::EPSILON,
        "same-cluster 2-hop should stay 0.5, got {result}"
    );
}

#[test]
fn supersession_bonus_zero_chain() {
    let result = score_access_with_evolution(0.5, 0);
    assert!(
        (result - 0.5).abs() < f64::EPSILON,
        "zero chain should not change score, got {result}"
    );
}

#[test]
fn supersession_bonus_chain_four() {
    let result = score_access_with_evolution(0.5, 4);
    // bonus = 4 * 0.05 = 0.2
    assert!(
        (result - 0.7).abs() < f64::EPSILON,
        "chain_length=4 should add 0.2, got {result}"
    );
}

#[test]
fn supersession_bonus_capped() {
    // chain_length=10, bonus would be 0.5 but capped at 0.2
    let result = score_access_with_evolution(0.5, 10);
    assert!(
        (result - 0.7).abs() < f64::EPSILON,
        "bonus should be capped at 0.2, got {result}"
    );
}

#[test]
fn supersession_bonus_higher_chain_scores_higher() {
    let base = 0.3;
    let short = score_access_with_evolution(base, 0);
    let long = score_access_with_evolution(base, 4);
    assert!(
        long > short,
        "chain_length=4 ({long}) should score higher than chain_length=0 ({short})"
    );
}

#[test]
fn supersession_bonus_clamped_to_one() {
    let result = score_access_with_evolution(0.95, 4);
    assert!(
        (result - 1.0).abs() < f64::EPSILON,
        "should clamp to 1.0, got {result}"
    );
}

#[test]
fn backward_compat_empty_context() {
    // With empty GraphContext, enhanced scores should equal base scores
    let ctx = GraphContext::default();
    assert!(ctx.is_empty(), "default graph context should be empty");

    let base_tier = 0.6;
    let enhanced_tier =
        score_epistemic_tier_with_importance(base_tier, ctx.importance("any_entity"));
    assert!(
        (enhanced_tier - base_tier).abs() < f64::EPSILON,
        "empty context tier should match base"
    );

    let base_prox = 0.0;
    let enhanced_prox =
        score_relationship_proximity_with_cluster(base_prox, ctx.same_cluster("any_entity"));
    assert!(
        (enhanced_prox - base_prox).abs() < f64::EPSILON,
        "empty context proximity should match base"
    );

    let base_access = 0.5;
    let enhanced_access = score_access_with_evolution(base_access, ctx.chain_length("any_fact"));
    assert!(
        (enhanced_access - base_access).abs() < f64::EPSILON,
        "empty context access should match base"
    );
}

#[test]
fn graph_context_same_cluster_populated() {
    let mut ctx = GraphContext::default();
    ctx.clusters.insert("alice".to_owned(), 1);
    ctx.clusters.insert("bob".to_owned(), 1);
    ctx.clusters.insert("charlie".to_owned(), 2);
    ctx.context_clusters.insert(1);

    assert!(ctx.same_cluster("alice"), "alice is in context cluster 1");
    assert!(ctx.same_cluster("bob"), "bob is in context cluster 1");
    assert!(
        !ctx.same_cluster("charlie"),
        "charlie is in cluster 2, not context cluster"
    );
    assert!(
        !ctx.same_cluster("unknown"),
        "unknown entity is not in any cluster"
    );
}

#[test]
fn graph_dirty_flag_lifecycle() {
    let flag = GraphDirtyFlag::new();
    assert!(!flag.is_dirty(), "new flag should start clean");
    assert!(!flag.take_dirty(), "taking clean flag should return false");

    flag.mark_dirty();
    assert!(flag.is_dirty(), "flag should be dirty after mark_dirty");
    assert!(
        flag.take_dirty(),
        "take_dirty should return true when dirty"
    );

    // After take, should be clean
    assert!(!flag.is_dirty(), "flag should be clean after take_dirty");
    assert!(
        !flag.take_dirty(),
        "subsequent take_dirty should return false when already clean"
    );
}

#[test]
fn graph_dirty_flag_multiple_marks() {
    let flag = GraphDirtyFlag::new();
    flag.mark_dirty();
    flag.mark_dirty();
    flag.mark_dirty();
    // Single take should clear
    assert!(
        flag.take_dirty(),
        "take_dirty should return true after multiple mark_dirty calls"
    );
    assert!(
        !flag.is_dirty(),
        "flag should be clean after single take_dirty"
    );
}
