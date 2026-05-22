//! Tests for counterfactual reasoning queries.
#![expect(clippy::expect_used, reason = "test assertions")]

use eidos::knowledge::CausalRelationType;

use crate::Db;
use crate::counterfactual::Counterfactual;
use crate::runtime::db::ScriptMutability;

/// Set up a `causal_edges` relation and populate it with test data.
fn setup_causal_graph(db: &Db) {
    db.run(
        r":create causal_edges {
            cause: String, effect: String =>
            ordering: String,
            relationship_type: String,
            confidence: Float,
            created_at: String
        }",
        std::collections::BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating causal_edges should succeed");

    // Graph: a -> b -> c,  a -> d,  d -> e
    // a causes b (enabled)
    // b causes c (caused)
    // a causes d (correlated)
    // d causes e (prevented)
    let edges = [
        ("a", "b", "enabled", 0.8),
        ("b", "c", "caused", 0.7),
        ("a", "d", "correlated", 0.5),
        ("d", "e", "prevented", 0.9),
    ];

    for (cause, effect, rel_type, confidence) in edges {
        let script = format!(
            r"?[cause, effect, ordering, relationship_type, confidence, created_at] <- [['{cause}', '{effect}', 'before', '{rel_type}', {confidence}, '2026-01-01T00:00:00Z']]
            :put causal_edges {{cause, effect => ordering, relationship_type, confidence, created_at}}"
        );
        db.run(
            &script,
            std::collections::BTreeMap::new(),
            ScriptMutability::Mutable,
        )
        .unwrap_or_else(|_| panic!("inserting edge {cause}->{effect} should succeed"));
    }
}

#[test]
fn dependency_analysis_finds_transitive_causes() {
    let db = Db::open_mem().expect("open_mem should succeed");
    setup_causal_graph(&db);

    // c depends on b and a (transitively)
    let deps =
        Counterfactual::dependency_analysis(&db, "c").expect("dependency analysis should succeed");

    assert_eq!(
        deps.len(),
        2,
        "c should have 2 dependency edges: b->c and a->b"
    );

    let causes: Vec<_> = deps.iter().map(|e| e.cause.as_str()).collect();
    assert!(causes.contains(&"b"), "c should directly depend on b");
    assert!(causes.contains(&"a"), "c should transitively depend on a");

    let b_to_c = deps
        .iter()
        .find(|e| e.cause == "b" && e.effect == "c")
        .expect("b->c edge");
    assert_eq!(b_to_c.relationship_type, CausalRelationType::Caused);
    assert!((b_to_c.confidence - 0.7).abs() < f64::EPSILON);
}

#[test]
fn impact_analysis_finds_transitive_effects() {
    let db = Db::open_mem().expect("open_mem should succeed");
    setup_causal_graph(&db);

    // a impacts b, c, d, e (transitively)
    let impacts =
        Counterfactual::impact_analysis(&db, "a").expect("impact analysis should succeed");

    assert_eq!(impacts.len(), 4, "a should have 4 impact edges");

    let effects: Vec<_> = impacts.iter().map(|e| e.effect.as_str()).collect();
    assert!(effects.contains(&"b"), "a should directly impact b");
    assert!(effects.contains(&"d"), "a should directly impact d");
    assert!(effects.contains(&"c"), "a should transitively impact c");
    assert!(effects.contains(&"e"), "a should transitively impact e");

    let a_to_b = impacts
        .iter()
        .find(|e| e.cause == "a" && e.effect == "b")
        .expect("a->b edge");
    assert_eq!(a_to_b.relationship_type, CausalRelationType::Enabled);
}

#[test]
fn minimal_provenance_returns_justifying_subgraph() {
    let db = Db::open_mem().expect("open_mem should succeed");
    setup_causal_graph(&db);

    // Provenance for c should include a->b and b->c
    let prov =
        Counterfactual::minimal_provenance(&db, "c").expect("minimal provenance should succeed");

    assert_eq!(prov.len(), 2, "provenance for c should have 2 edges");

    let causes: Vec<_> = prov.iter().map(|e| e.cause.as_str()).collect();
    assert!(causes.contains(&"a"), "provenance should include a");
    assert!(causes.contains(&"b"), "provenance should include b");
}

#[test]
fn dependency_analysis_for_root_node_returns_empty() {
    let db = Db::open_mem().expect("open_mem should succeed");
    setup_causal_graph(&db);

    let deps =
        Counterfactual::dependency_analysis(&db, "a").expect("dependency analysis should succeed");
    assert!(
        deps.is_empty(),
        "a has no causes, so dependency should be empty"
    );
}

#[test]
fn impact_analysis_for_leaf_node_returns_empty() {
    let db = Db::open_mem().expect("open_mem should succeed");
    setup_causal_graph(&db);

    let impacts =
        Counterfactual::impact_analysis(&db, "c").expect("impact analysis should succeed");
    assert!(
        impacts.is_empty(),
        "c has no effects, so impact should be empty"
    );
}

#[test]
fn minimal_provenance_for_root_node_returns_empty() {
    let db = Db::open_mem().expect("open_mem should succeed");
    setup_causal_graph(&db);

    let prov =
        Counterfactual::minimal_provenance(&db, "a").expect("minimal provenance should succeed");
    assert!(prov.is_empty(), "a is a root cause with no incoming edges");
}

#[test]
fn dependency_analysis_for_unknown_node_returns_empty() {
    let db = Db::open_mem().expect("open_mem should succeed");
    setup_causal_graph(&db);

    let deps = Counterfactual::dependency_analysis(&db, "unknown")
        .expect("dependency analysis should succeed");
    assert!(deps.is_empty(), "unknown node should have no dependencies");
}

#[test]
fn unknown_causal_relation_errors() {
    let db = Db::open_mem().expect("open_mem should succeed");
    db.run(
        r":create causal_edges {
            cause: String, effect: String =>
            ordering: String,
            relationship_type: String,
            confidence: Float,
            created_at: String
        }",
        std::collections::BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("creating causal_edges should succeed");
    db.run(
        r"?[cause, effect, ordering, relationship_type, confidence, created_at] <- [['a', 'b', 'before', 'mystery', 0.7, '2026-01-01T00:00:00Z']]
        :put causal_edges {cause, effect => ordering, relationship_type, confidence, created_at}",
        std::collections::BTreeMap::new(),
        ScriptMutability::Mutable,
    )
    .expect("inserting edge should succeed");

    let err = Counterfactual::impact_analysis(&db, "a")
        .expect_err("unknown relation type should be rejected");
    assert!(
        matches!(err, crate::error::Error::UnknownCausalRelation { .. }),
        "expected UnknownCausalRelation, got {err:?}"
    );
}
