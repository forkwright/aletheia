#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions use direct indexing"
)]

use crate::id::FactId;
use crate::knowledge::{
    CausalEdge, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
    TemporalOrdering, far_future,
};
use crate::knowledge_store::KnowledgeStore;

/// Helper: create a minimal fact with the given ID and content.
fn make_fact(id: &str, content: &str) -> Fact {
    let now = jiff::Timestamp::now();
    Fact {
        id: FactId::new(id).expect("valid test id"),
        nous_id: "test-nous".to_owned(),
        content: content.to_owned(),
        fact_type: "observation".to_owned(),
        temporal: FactTemporal {
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            source_session_id: Some("test-session".to_owned()),
            stability_hours: 720.0,
        },
        lifecycle: FactLifecycle {
            superseded_by: None,
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        },
        access: FactAccess {
            access_count: 0,
            last_accessed_at: None,
        },
    }
}

/// Helper: create a causal edge.
fn make_edge(cause: &str, effect: &str, confidence: f64) -> CausalEdge {
    CausalEdge {
        cause: FactId::new(cause).expect("valid test id"),
        effect: FactId::new(effect).expect("valid test id"),
        ordering: TemporalOrdering::Before,
        confidence,
        created_at: jiff::Timestamp::now(),
    }
}

#[test]
fn insert_and_query_causal_edge() {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");

    store
        .insert_fact(&make_fact("fact-a", "Alice uses Rust"))
        .expect("insert fact-a should succeed");
    store
        .insert_fact(&make_fact("fact-b", "Alice chose Rust for performance"))
        .expect("insert fact-b should succeed");

    let edge = make_edge("fact-a", "fact-b", 0.9);
    store
        .insert_causal_edge(&edge)
        .expect("insert causal edge should succeed");

    // Query effects of fact-a.
    let effects = store
        .query_effects(&FactId::new("fact-a").expect("valid test id"))
        .expect("query_effects should succeed");
    assert_eq!(effects.len(), 1, "should have one effect");
    assert_eq!(
        effects[0].effect.as_str(),
        "fact-b",
        "effect should be fact-b"
    );
    assert!(
        (effects[0].confidence - 0.9).abs() < f64::EPSILON,
        "confidence should be 0.9"
    );

    // Query causes of fact-b.
    let causes = store
        .query_causes(&FactId::new("fact-b").expect("valid test id"))
        .expect("query_causes should succeed");
    assert_eq!(causes.len(), 1, "should have one cause");
    assert_eq!(causes[0].cause.as_str(), "fact-a", "cause should be fact-a");
}

#[test]
fn list_causal_edges() {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");

    store
        .insert_fact(&make_fact("f1", "fact one"))
        .expect("insert f1");
    store
        .insert_fact(&make_fact("f2", "fact two"))
        .expect("insert f2");
    store
        .insert_fact(&make_fact("f3", "fact three"))
        .expect("insert f3");

    store
        .insert_causal_edge(&make_edge("f1", "f2", 0.8))
        .expect("insert edge f1->f2");
    store
        .insert_causal_edge(&make_edge("f2", "f3", 0.7))
        .expect("insert edge f2->f3");

    let all = store
        .list_causal_edges()
        .expect("list_causal_edges should succeed");
    assert_eq!(all.len(), 2, "should have two causal edges");
}

#[test]
fn remove_causal_edge() {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");

    store
        .insert_fact(&make_fact("f1", "fact one"))
        .expect("insert f1");
    store
        .insert_fact(&make_fact("f2", "fact two"))
        .expect("insert f2");

    store
        .insert_causal_edge(&make_edge("f1", "f2", 0.8))
        .expect("insert edge");

    store
        .remove_causal_edge(
            &FactId::new("f1").expect("valid test id"),
            &FactId::new("f2").expect("valid test id"),
        )
        .expect("remove edge should succeed");

    let effects = store
        .query_effects(&FactId::new("f1").expect("valid test id"))
        .expect("query_effects should succeed");
    assert!(effects.is_empty(), "should have no effects after removal");
}

#[test]
fn confidence_propagation_direct() {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");

    store
        .insert_fact(&make_fact("f1", "fact one"))
        .expect("insert f1");
    store
        .insert_fact(&make_fact("f2", "fact two"))
        .expect("insert f2");

    store
        .insert_causal_edge(&make_edge("f1", "f2", 0.9))
        .expect("insert edge");

    let conf = store
        .propagate_confidence(
            &FactId::new("f1").expect("valid test id"),
            &FactId::new("f2").expect("valid test id"),
        )
        .expect("propagate_confidence should succeed");
    assert!(conf.is_some(), "should find a path");
    assert!(
        (conf.expect("confidence exists") - 0.9).abs() < f64::EPSILON,
        "direct edge confidence should be 0.9"
    );
}

#[test]
fn confidence_propagation_chain() {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");

    store
        .insert_fact(&make_fact("a", "start"))
        .expect("insert a");
    store
        .insert_fact(&make_fact("b", "middle"))
        .expect("insert b");
    store.insert_fact(&make_fact("c", "end")).expect("insert c");

    // a -> b (0.8) -> c (0.5)
    store
        .insert_causal_edge(&make_edge("a", "b", 0.8))
        .expect("insert edge a->b");
    store
        .insert_causal_edge(&make_edge("b", "c", 0.5))
        .expect("insert edge b->c");

    let conf = store
        .propagate_confidence(
            &FactId::new("a").expect("valid test id"),
            &FactId::new("c").expect("valid test id"),
        )
        .expect("propagate_confidence should succeed");
    assert!(conf.is_some(), "should find a path a->b->c");
    let expected = 0.8 * 0.5;
    assert!(
        (conf.expect("confidence exists") - expected).abs() < f64::EPSILON,
        "transitive confidence should be product: {expected}"
    );
}

#[test]
fn confidence_propagation_no_path() {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");

    store
        .insert_fact(&make_fact("x", "isolated"))
        .expect("insert x");
    store
        .insert_fact(&make_fact("y", "also isolated"))
        .expect("insert y");

    let conf = store
        .propagate_confidence(
            &FactId::new("x").expect("valid test id"),
            &FactId::new("y").expect("valid test id"),
        )
        .expect("propagate_confidence should succeed");
    assert!(conf.is_none(), "should return None when no path exists");
}

#[test]
fn confidence_propagation_same_node() {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");

    store
        .insert_fact(&make_fact("z", "self"))
        .expect("insert z");

    let conf = store
        .propagate_confidence(
            &FactId::new("z").expect("valid test id"),
            &FactId::new("z").expect("valid test id"),
        )
        .expect("propagate_confidence should succeed");
    assert_eq!(conf, Some(1.0), "self-path should have confidence 1.0");
}

#[test]
fn temporal_ordering_roundtrip() {
    assert_eq!(
        TemporalOrdering::Before
            .as_str()
            .parse::<TemporalOrdering>()
            .expect("parse before"),
        TemporalOrdering::Before,
        "Before roundtrips"
    );
    assert_eq!(
        TemporalOrdering::After
            .as_str()
            .parse::<TemporalOrdering>()
            .expect("parse after"),
        TemporalOrdering::After,
        "After roundtrips"
    );
    assert_eq!(
        TemporalOrdering::Concurrent
            .as_str()
            .parse::<TemporalOrdering>()
            .expect("parse concurrent"),
        TemporalOrdering::Concurrent,
        "Concurrent roundtrips"
    );
}

#[test]
fn causal_edge_with_concurrent_ordering() {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");

    store
        .insert_fact(&make_fact("c1", "concurrent cause"))
        .expect("insert c1");
    store
        .insert_fact(&make_fact("c2", "concurrent effect"))
        .expect("insert c2");

    let edge = CausalEdge {
        cause: FactId::new("c1").expect("valid test id"),
        effect: FactId::new("c2").expect("valid test id"),
        ordering: TemporalOrdering::Concurrent,
        confidence: 0.75,
        created_at: jiff::Timestamp::now(),
    };
    store
        .insert_causal_edge(&edge)
        .expect("insert concurrent edge");

    let effects = store
        .query_effects(&FactId::new("c1").expect("valid test id"))
        .expect("query_effects should succeed");
    assert_eq!(effects.len(), 1, "one effect");
    assert_eq!(
        effects[0].ordering,
        TemporalOrdering::Concurrent,
        "ordering preserved"
    );
}

#[test]
fn rejects_invalid_confidence() {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");

    let edge = make_edge("a", "b", 1.5);
    let result = store.insert_causal_edge(&edge);
    assert!(result.is_err(), "should reject confidence > 1.0");

    let edge = make_edge("a", "b", -0.1);
    let result = store.insert_causal_edge(&edge);
    assert!(result.is_err(), "should reject confidence < 0.0");
}

#[test]
fn schema_version_is_six_after_fresh_init() {
    let store = KnowledgeStore::open_mem().expect("open_mem should succeed");
    let version = store
        .schema_version()
        .expect("schema_version should succeed");
    assert_eq!(version, 6, "fresh init should be at schema version 6");
}
