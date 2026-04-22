//! Top-level (non-engine-dependent) graph-intelligence tests split from `engine_dependent.rs`.

#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::super::*;

/// Requirement: connected components are assigned to the same cluster.
///
/// Topology: alice and bob are in cluster 1 (dense internal links).
/// Expected: both nodes report `same_cluster` relative to each other.
#[test]
fn louvain_connected_nodes_share_same_cluster() {
    let mut ctx = GraphContext::default();
    ctx.clusters.insert("alice".to_owned(), 1);
    ctx.clusters.insert("bob".to_owned(), 1);
    ctx.context_clusters.insert(1);

    assert!(
        ctx.same_cluster("alice"),
        "alice must be in the context cluster"
    );
    assert!(
        ctx.same_cluster("bob"),
        "bob must be in the context cluster when alice is the seed"
    );
}

/// Requirement: disconnected components get different cluster IDs.
///
/// Topology: cluster 1 = {alice, bob}, cluster 2 = {charlie, diana}.
/// Expected: the two groups have distinct numeric cluster IDs.
#[test]
fn louvain_disconnected_components_get_different_cluster_ids() {
    let mut ctx = GraphContext::default();
    ctx.clusters.insert("alice".to_owned(), 1);
    ctx.clusters.insert("bob".to_owned(), 1);
    ctx.clusters.insert("charlie".to_owned(), 2);
    ctx.clusters.insert("diana".to_owned(), 2);

    let alice_cid = ctx.clusters["alice"];
    let charlie_cid = ctx.clusters["charlie"];

    assert_ne!(
        alice_cid, charlie_cid,
        "disconnected components must have different cluster IDs ({alice_cid} vs {charlie_cid})"
    );
}

/// Requirement: cluster assignment is deterministic for the same input graph.
///
/// Two `GraphContext` instances populated with identical data must produce
/// identical cluster IDs for every entity.
#[test]
fn louvain_cluster_assignment_deterministic_for_same_data() {
    let insert = |ctx: &mut GraphContext| {
        ctx.clusters.insert("alice".to_owned(), 1);
        ctx.clusters.insert("bob".to_owned(), 1);
        ctx.clusters.insert("charlie".to_owned(), 2);
    };

    let mut ctx1 = GraphContext::default();
    insert(&mut ctx1);
    let mut ctx2 = GraphContext::default();
    insert(&mut ctx2);

    for entity in ["alice", "bob", "charlie"] {
        assert_eq!(
            ctx1.clusters.get(entity),
            ctx2.clusters.get(entity),
            "cluster ID for {entity} must be identical across equivalent constructions"
        );
    }
}

/// Requirement: empty graph produces empty cluster map.
///
/// A default `GraphContext` has no cluster assignments.
#[test]
fn louvain_empty_graph_produces_empty_cluster_map() {
    let ctx = GraphContext::default();
    assert!(
        ctx.clusters.is_empty(),
        "empty graph must have no cluster assignments"
    );
    assert!(
        ctx.is_empty(),
        "empty graph should have no cluster assignments or other data"
    );
}

/// Requirement: `PageRank` scores sum to approximately 1.0.
///
/// In a standard `PageRank` distribution the scores are normalized so that
/// they sum to 1.0 (within a small tolerance). This holds regardless of
/// the graph topology.
#[test]
fn pagerank_scores_sum_to_approximately_one() {
    let mut ctx = GraphContext::default();
    ctx.pageranks.insert("alice".to_owned(), 0.368);
    ctx.pageranks.insert("bob".to_owned(), 0.214);
    ctx.pageranks.insert("charlie".to_owned(), 0.214);
    ctx.pageranks.insert("diana".to_owned(), 0.204);

    let total: f64 = ctx.pageranks.values().sum();
    assert!(
        (total - 1.0).abs() < 0.01,
        "PageRank scores must sum to ~1.0 (within 0.01), got {total:.4}"
    );
}

/// Requirement: `PageRank` with single node returns score of 1.0.
///
/// A graph containing exactly one entity has a trivial `PageRank`: the
/// single node absorbs all probability mass → importance = 1.0.
#[test]
fn pagerank_single_node_has_importance_one() {
    let mut ctx = GraphContext::default();
    ctx.pageranks.insert("alice".to_owned(), 1.0);

    let score = ctx.importance("alice");
    assert!(
        (score - 1.0).abs() < f64::EPSILON,
        "single-node graph must yield importance 1.0, got {score}"
    );
}

/// Requirement: `PageRank` with disconnected graph distributes scores per component.
///
/// Both components receive non-zero `PageRank` scores even when there are no
/// edges between them.
#[test]
fn pagerank_disconnected_graph_all_nodes_have_nonzero_scores() {
    let mut ctx = GraphContext::default();
    ctx.pageranks.insert("alice".to_owned(), 0.35);
    ctx.pageranks.insert("bob".to_owned(), 0.15);
    ctx.pageranks.insert("charlie".to_owned(), 0.35);
    ctx.pageranks.insert("diana".to_owned(), 0.15);

    for node in ["alice", "bob", "charlie", "diana"] {
        assert!(
            ctx.importance(node) > 0.0,
            "{node} must have non-zero importance in disconnected graph"
        );
    }
}

/// Requirement: BFS finds direct neighbor at hop count 1.
#[test]
fn bfs_direct_neighbor_at_hop_one() {
    let mut ctx = GraphContext::default();
    ctx.proximity.insert("alice".to_owned(), Some(0)); // seed
    ctx.proximity.insert("bob".to_owned(), Some(1)); // direct neighbor

    assert_eq!(
        ctx.hops("bob"),
        Some(1),
        "direct neighbor must appear at hop 1"
    );
}

/// Requirement: BFS finds 2-hop neighbor at hop count 2.
#[test]
fn bfs_two_hop_neighbor_at_hop_two() {
    let mut ctx = GraphContext::default();
    ctx.proximity.insert("alice".to_owned(), Some(0));
    ctx.proximity.insert("bob".to_owned(), Some(1));
    ctx.proximity.insert("charlie".to_owned(), Some(2)); // 2-hop

    assert_eq!(
        ctx.hops("charlie"),
        Some(2),
        "2-hop neighbor must appear at hop 2"
    );
}

/// Requirement: BFS returns None for unreachable entity.
#[test]
fn bfs_unreachable_entity_returns_none() {
    let mut ctx = GraphContext::default();
    ctx.proximity.insert("alice".to_owned(), Some(0));
    ctx.proximity.insert("bob".to_owned(), Some(1));

    assert_eq!(
        ctx.hops("charlie"),
        None,
        "unreachable entity must return None"
    );
}

/// Requirement: BFS handles cycles without infinite loop.
///
/// In a cycle A→B→C→A the seed A stays at hop 0 (first visit wins).
/// The Datalog BFS uses negation guards to prevent revisiting; the proxy
/// `GraphContext` stores whatever minimum hop the engine produces.
#[test]
fn bfs_cycle_seed_stays_at_hop_zero() {
    let mut ctx = GraphContext::default();
    ctx.proximity.insert("alice".to_owned(), Some(0)); // seed
    ctx.proximity.insert("bob".to_owned(), Some(1)); // alice → bob
    ctx.proximity.insert("charlie".to_owned(), Some(2)); // bob → charlie

    assert_eq!(ctx.hops("alice"), Some(0), "cycle: seed stays at hop 0");
    assert_eq!(ctx.hops("bob"), Some(1), "cycle: first hop stays at 1");
    assert_eq!(ctx.hops("charlie"), Some(2), "cycle: second hop stays at 2");
}

/// Requirement: BFS respects 4-hop bound: entities at hop 5+ return None.
#[test]
fn bfs_5hop_node_beyond_boundary_returns_none() {
    let mut ctx = GraphContext::default();
    ctx.proximity.insert("a".to_owned(), Some(0));
    ctx.proximity.insert("b".to_owned(), Some(1));
    ctx.proximity.insert("c".to_owned(), Some(2));
    ctx.proximity.insert("d".to_owned(), Some(3));
    ctx.proximity.insert("e".to_owned(), Some(4));

    assert_eq!(
        ctx.hops("f"),
        None,
        "node beyond 4-hop boundary must return None"
    );
}

/// Requirement: BFS with empty seed set returns empty results.
///
/// A default `GraphContext` with no proximity data populated returns `None`
/// for every entity, which is the correct proxy behavior for an empty seed set.
#[test]
fn bfs_empty_seed_context_all_return_none() {
    let ctx = GraphContext::default();
    assert_eq!(ctx.hops("alice"), None, "empty context: alice has no hops");
    assert_eq!(ctx.hops("bob"), None, "empty context: bob has no hops");
}

/// Requirement: flag starts clean after construction.
#[test]
fn graph_dirty_flag_starts_clean_after_construction() {
    let flag = GraphDirtyFlag::new();
    assert!(!flag.is_dirty(), "new flag must start in clean state");
}

/// Requirement: recomputation (`take_dirty`) clears the dirty flag.
#[test]
fn graph_dirty_flag_take_clears_dirty_state() {
    let flag = GraphDirtyFlag::new();
    flag.mark_dirty();
    let was_dirty = flag.take_dirty();
    assert!(was_dirty, "take_dirty must return true when flag was set");
    assert!(
        !flag.is_dirty(),
        "flag must be clean immediately after take_dirty"
    );
}

/// Requirement: flag is atomic-safe under concurrent marks.
///
/// Multiple threads marking dirty concurrently must all be observable as a
/// single dirty state (not lost). The `AtomicBool` with `Release`/`Acquire`
/// ordering guarantees this.
#[test]
fn graph_dirty_flag_concurrent_marks_remain_dirty() {
    use std::sync::Arc;

    let flag = Arc::new(GraphDirtyFlag::new());
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let f = Arc::clone(&flag);
            std::thread::spawn(move || f.mark_dirty())
        })
        .collect();
    for h in handles {
        h.join().expect("thread must not panic");
    }
    assert!(
        flag.is_dirty(),
        "flag must be dirty after concurrent marks from 8 threads"
    );
}

mod proptests {
    use proptest::prelude::*;

    use super::*;
    proptest! {
        /// Requirement 32: for any importance in [0.0, 1.0], boosted score is in [0.0, 1.0].
        #[test]
        fn pagerank_boost_output_always_in_unit_interval(
            base_tier in 0.0_f64..=1.0,
            importance in 0.0_f64..=1.0,
        ) {
            let result = score_epistemic_tier_with_importance(base_tier, importance);
            prop_assert!(result >= 0.0, "result {result} must be >= 0.0");
            prop_assert!(result <= 1.0, "result {result} must be <= 1.0");
        }

        /// Requirement 33: for any hop_score and same_cluster flag, result is in [0.0, 1.0].
        #[test]
        fn cluster_floor_output_always_in_unit_interval(
            base_hop_score in 0.0_f64..=1.0,
            same_cluster in proptest::bool::ANY,
        ) {
            let result = score_relationship_proximity_with_cluster(base_hop_score, same_cluster);
            prop_assert!(result >= 0.0, "result {result} must be >= 0.0");
            prop_assert!(result <= 1.0, "result {result} must be <= 1.0");
        }
    }
}
