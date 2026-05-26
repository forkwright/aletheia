//! Tests for derived-rule materialization.
//!
//! Validates:
//! - Ontological IS-A closure: derived rows appear after `materialize_ontological_rules`.
//! - Causal chain closure: transitive confidence propagation via Datalog replaces BFS.
//! - Defeasible defaults: defaults fire when no override; suppressed by verified facts.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]

use eidos::id::{CausalEdgeId, EntityId, FactId};

use crate::knowledge::{
    CausalEdge, CausalRelationType, Entity, EpistemicTier, Fact, FactAccess, FactLifecycle,
    FactProvenance, FactSensitivity, FactTemporal, TemporalOrdering, far_future,
};
use crate::knowledge_store::KnowledgeStore;

// ── Helpers ────────────────────────────────────────────────────────────────────

fn make_entity(store: &std::sync::Arc<KnowledgeStore>, id: &str, name: &str, entity_type: &str) {
    store
        .insert_entity(&Entity {
            id: EntityId::new(id).expect("valid entity id"),
            name: name.to_owned(),
            entity_type: entity_type.to_owned(),
            aliases: vec![],
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        })
        .expect("insert entity should succeed");
}

fn make_fact_with_tier(
    store: &std::sync::Arc<KnowledgeStore>,
    id: &str,
    entity_id: &str,
    content: &str,
    tier: EpistemicTier,
) {
    let now = jiff::Timestamp::now();
    let fact = Fact {
        id: FactId::new(id).expect("valid fact id"),
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
            tier,
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
        sensitivity: FactSensitivity::Public,
        visibility: crate::knowledge::Visibility::Private,
        scope: None,
        project_id: None,
    };
    store
        .insert_fact(&fact)
        .expect("insert fact should succeed");
    store
        .insert_fact_entity(
            &FactId::new(id).expect("valid fact id"),
            &EntityId::new(entity_id).expect("valid entity id"),
        )
        .expect("insert fact-entity should succeed");
}

fn make_causal_edge(cause: &str, effect: &str, confidence: f64) -> CausalEdge {
    CausalEdge {
        id: CausalEdgeId::new(format!("ce-{cause}-{effect}")).expect("valid edge id"),
        source_id: FactId::new(cause).expect("valid source id"),
        target_id: FactId::new(effect).expect("valid target id"),
        relationship_type: CausalRelationType::Caused,
        ordering: TemporalOrdering::Before,
        confidence,
        evidence_session_id: Some("test-session".to_owned()),
        timestamp: jiff::Timestamp::now(),
    }
}

// ── Ontological rule tests ─────────────────────────────────────────────────────

#[test]
fn ontological_is_a_direct_edge_produces_derived_fact() {
    let store = KnowledgeStore::open_mem().expect("open_mem");

    // alice is a data_scientist
    make_entity(&store, "alice", "Alice", "data_scientist");

    // data_scientist IS-A analyst
    store
        .insert_type_hierarchy("data_scientist", "analyst")
        .expect("insert type hierarchy");

    let count = store
        .materialize_ontological_rules()
        .expect("materialize ontological rules");

    assert!(count > 0, "should produce at least one derived fact");

    let derived = store
        .query_derived_facts("alice")
        .expect("query derived facts for alice");
    let is_a = derived
        .iter()
        .find(|d| d.rule_id == "ontological:is_a" && d.derived_content == "type:analyst");
    assert!(
        is_a.is_some(),
        "alice should have derived is_a:analyst; got {derived:?}"
    );
}

#[test]
fn ontological_is_a_transitive_two_hops() {
    let store = KnowledgeStore::open_mem().expect("open_mem");

    // bob is a data_scientist
    make_entity(&store, "bob", "Bob", "data_scientist");

    // data_scientist IS-A analyst IS-A knowledge_worker
    store
        .insert_type_hierarchy("data_scientist", "analyst")
        .expect("insert d->a");
    store
        .insert_type_hierarchy("analyst", "knowledge_worker")
        .expect("insert a->kw");

    store
        .materialize_ontological_rules()
        .expect("materialize ontological rules");

    let derived = store
        .query_derived_facts("bob")
        .expect("query derived facts for bob");

    let has_analyst = derived
        .iter()
        .any(|d| d.rule_id == "ontological:is_a" && d.derived_content == "type:analyst");
    let has_kw = derived
        .iter()
        .any(|d| d.rule_id == "ontological:is_a" && d.derived_content == "type:knowledge_worker");

    assert!(has_analyst, "bob should transitively be_a analyst");
    assert!(has_kw, "bob should transitively be_a knowledge_worker");
}

#[test]
fn ontological_no_hierarchy_produces_no_derived_facts() {
    let store = KnowledgeStore::open_mem().expect("open_mem");

    // entity exists but no type_hierarchy rows
    make_entity(&store, "charlie", "Charlie", "wizard");

    let count = store
        .materialize_ontological_rules()
        .expect("materialize ontological rules");
    assert_eq!(count, 0, "no hierarchy → no derived facts");
}

// ── Causal chain rule tests ────────────────────────────────────────────────────

#[test]
fn causal_chain_direct_edge_appears_in_derived_facts() {
    let store = KnowledgeStore::open_mem().expect("open_mem");

    // Two facts with a direct causal edge.
    let now = jiff::Timestamp::now();
    let make = |id: &str, content: &str| Fact {
        id: FactId::new(id).expect("valid"),
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
            source_session_id: None,
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
        sensitivity: FactSensitivity::Public,
        visibility: crate::knowledge::Visibility::Private,
        scope: None,
        project_id: None,
    };

    store
        .insert_fact(&make("root", "root cause"))
        .expect("root");
    store
        .insert_fact(&make("effect1", "first effect"))
        .expect("e1");
    store
        .insert_causal_edge(&make_causal_edge("root", "effect1", 0.8))
        .expect("edge root->e1");

    let count = store
        .materialize_causal_chain_rules()
        .expect("materialize causal chain rules");
    assert!(count > 0, "should produce at least one derived causal row");

    let derived = store
        .query_derived_facts_by_rule("root", "causal")
        .expect("query causal derived facts");
    let chain = derived
        .iter()
        .find(|d| d.derived_content == "causes:effect1");
    assert!(
        chain.is_some(),
        "root should have causal:transitive_chain for effect1; got {derived:?}"
    );
    assert!(
        (chain.unwrap().confidence - 0.8).abs() < 1e-9,
        "direct edge confidence should be 0.8"
    );
}

#[test]
fn causal_chain_transitive_confidence_is_product() {
    let store = KnowledgeStore::open_mem().expect("open_mem");

    let now = jiff::Timestamp::now();
    let make = |id: &str| Fact {
        id: FactId::new(id).expect("valid"),
        nous_id: "test-nous".to_owned(),
        content: id.to_owned(),
        fact_type: "observation".to_owned(),
        temporal: FactTemporal {
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            source_session_id: None,
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
        sensitivity: FactSensitivity::Public,
        visibility: crate::knowledge::Visibility::Private,
        scope: None,
        project_id: None,
    };

    store.insert_fact(&make("fa")).expect("fa");
    store.insert_fact(&make("fb")).expect("fb");
    store.insert_fact(&make("fc")).expect("fc");

    // fa -> fb (0.8) -> fc (0.6) => transitive confidence = 0.48
    store
        .insert_causal_edge(&make_causal_edge("fa", "fb", 0.8))
        .expect("fa->fb");
    store
        .insert_causal_edge(&make_causal_edge("fb", "fc", 0.6))
        .expect("fb->fc");

    store
        .materialize_causal_chain_rules()
        .expect("materialize causal chain");

    let derived = store
        .query_derived_facts_by_rule("fa", "causal")
        .expect("query causal derived facts for fa");

    let transitive = derived.iter().find(|d| d.derived_content == "causes:fc");
    assert!(
        transitive.is_some(),
        "fa should have transitive chain to fc; got {derived:?}"
    );
    let expected = 0.8 * 0.6;
    assert!(
        (transitive.unwrap().confidence - expected).abs() < 1e-9,
        "transitive confidence should be 0.8*0.6={expected}"
    );
}

#[test]
fn causal_chain_low_confidence_pruned() {
    let store = KnowledgeStore::open_mem().expect("open_mem");

    let now = jiff::Timestamp::now();
    let make = |id: &str| Fact {
        id: FactId::new(id).expect("valid"),
        nous_id: "test-nous".to_owned(),
        content: id.to_owned(),
        fact_type: "observation".to_owned(),
        temporal: FactTemporal {
            valid_from: now,
            valid_to: far_future(),
            recorded_at: now,
        },
        provenance: FactProvenance {
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            source_session_id: None,
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
        sensitivity: FactSensitivity::Public,
        visibility: crate::knowledge::Visibility::Private,
        scope: None,
        project_id: None,
    };

    store.insert_fact(&make("g1")).expect("g1");
    store.insert_fact(&make("g2")).expect("g2");
    store.insert_fact(&make("g3")).expect("g3");

    // g1 -> g2 (0.1) -> g3 (0.1) => product = 0.01 < 0.05 threshold
    store
        .insert_causal_edge(&make_causal_edge("g1", "g2", 0.1))
        .expect("g1->g2");
    store
        .insert_causal_edge(&make_causal_edge("g2", "g3", 0.1))
        .expect("g2->g3");

    store
        .materialize_causal_chain_rules()
        .expect("materialize causal chain");

    let derived = store
        .query_derived_facts_by_rule("g1", "causal")
        .expect("query g1 causal facts");

    // Direct edge g1->g2 with conf 0.1 should appear (>= 0.05).
    let direct = derived.iter().find(|d| d.derived_content == "causes:g2");
    assert!(
        direct.is_some(),
        "direct edge g1->g2 (conf=0.1) should appear"
    );

    // Transitive g1->g3 with conf 0.01 should be pruned (< 0.05).
    let transitive = derived.iter().find(|d| d.derived_content == "causes:g3");
    assert!(
        transitive.is_none(),
        "transitive g1->g3 (conf=0.01) should be pruned by 0.05 threshold; got {transitive:?}"
    );
}

// ── Defeasible default tests ───────────────────────────────────────────────────

#[test]
fn defeasible_default_fires_without_override() {
    let store = KnowledgeStore::open_mem().expect("open_mem");

    make_entity(&store, "acme", "Acme Corp", "organisation");

    // Insert a default for acme: "acme is a reliable supplier"
    store
        .insert_default("acme", "supplier", "acme is a reliable supplier", 0.7)
        .expect("insert default");

    let count = store
        .materialize_defeasible_rules()
        .expect("materialize defeasible rules");
    assert!(count > 0, "default should fire when no override");

    let derived = store
        .query_derived_facts_by_rule("acme", "defeasible")
        .expect("query defeasible derived facts for acme");
    let default_fact = derived.iter().find(|d| {
        d.rule_id == "defeasible:default" && d.derived_content == "acme is a reliable supplier"
    });
    assert!(
        default_fact.is_some(),
        "default should appear as derived fact; got {derived:?}"
    );
    assert!(
        (default_fact.unwrap().confidence - 0.7).abs() < 1e-9,
        "confidence should match inserted default"
    );
}

#[test]
fn defeasible_default_suppressed_by_verified_fact() {
    let store = KnowledgeStore::open_mem().expect("open_mem");

    make_entity(&store, "bob", "Bob", "person");

    // Insert a default for bob about skill
    store
        .insert_default("bob", "skill", "bob is a beginner programmer", 0.6)
        .expect("insert default");

    // Insert a verified fact that covers the tag "skill"
    make_fact_with_tier(
        &store,
        "skill-fact",
        "bob",
        "bob is an expert programmer with 10 years skill",
        EpistemicTier::Verified,
    );

    let count = store
        .materialize_defeasible_rules()
        .expect("materialize defeasible rules");

    // The default should be suppressed (override matched), so count = 0
    // for bob's skill default. We query to verify.
    let derived = store
        .query_derived_facts_by_rule("bob", "defeasible")
        .expect("query defeasible derived facts for bob");

    let suppressed = derived
        .iter()
        .any(|d| d.derived_content == "bob is a beginner programmer");
    assert!(
        !suppressed,
        "default should be suppressed by verified override; count={count}, derived={derived:?}"
    );
}

#[test]
fn defeasible_default_entity_scoped_override_does_not_suppress_other_entity() {
    let store = KnowledgeStore::open_mem().expect("open_mem");

    make_entity(&store, "alice", "Alice", "person");
    make_entity(&store, "carol", "Carol", "person");

    // Both have a "skill" default
    store
        .insert_default("alice", "skill", "alice is a beginner programmer", 0.6)
        .expect("insert alice default");
    store
        .insert_default("carol", "skill", "carol is a beginner programmer", 0.6)
        .expect("insert carol default");

    // Only alice has a verified override
    make_fact_with_tier(
        &store,
        "alice-skill",
        "alice",
        "alice is an expert with skill",
        EpistemicTier::Verified,
    );

    store
        .materialize_defeasible_rules()
        .expect("materialize defeasible rules");

    let alice_derived = store
        .query_derived_facts_by_rule("alice", "defeasible")
        .expect("query alice defeasible");
    let carol_derived = store
        .query_derived_facts_by_rule("carol", "defeasible")
        .expect("query carol defeasible");

    let alice_default_suppressed = !alice_derived
        .iter()
        .any(|d| d.derived_content == "alice is a beginner programmer");
    let carol_default_present = carol_derived
        .iter()
        .any(|d| d.derived_content == "carol is a beginner programmer");

    assert!(
        alice_default_suppressed,
        "alice's default should be suppressed by her verified fact"
    );
    assert!(
        carol_default_present,
        "carol's default should NOT be suppressed by alice's verified fact"
    );
}

// ── Schema version test ────────────────────────────────────────────────────────

#[test]
fn schema_version_is_current_after_fresh_init() {
    let store = KnowledgeStore::open_mem().expect("open_mem");
    let version = store
        .schema_version()
        .expect("schema_version should succeed");
    assert_eq!(
        version,
        KnowledgeStore::SCHEMA_VERSION,
        "fresh init should be at current schema version"
    );
}

// ── Rule-ID query filter ───────────────────────────────────────────────────────

#[test]
fn query_derived_facts_by_rule_prefix_filters_correctly() {
    let store = KnowledgeStore::open_mem().expect("open_mem");

    make_entity(&store, "dave", "Dave", "analyst");

    // analyst IS-A knowledge_worker
    store
        .insert_type_hierarchy("analyst", "knowledge_worker")
        .expect("type hierarchy");

    // causal edge from fact-dave to fact-report
    let now = jiff::Timestamp::now();
    let make = |id: &str, content: &str| Fact {
        id: FactId::new(id).expect("valid"),
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
            source_session_id: None,
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
        sensitivity: FactSensitivity::Public,
        visibility: crate::knowledge::Visibility::Private,
        scope: None,
        project_id: None,
    };
    store
        .insert_fact(&make("fact-dave", "dave does analysis"))
        .expect("fact-dave");
    store
        .insert_fact(&make("fact-report", "report produced"))
        .expect("fact-report");
    store
        .insert_causal_edge(&make_causal_edge("fact-dave", "fact-report", 0.7))
        .expect("edge");

    store.materialize_derived_facts().expect("materialize all");

    let ontological = store
        .query_derived_facts_by_rule("dave", "ontological")
        .expect("ontological filter");
    let causal = store
        .query_derived_facts_by_rule("fact-dave", "causal")
        .expect("causal filter");

    assert!(
        ontological
            .iter()
            .all(|d| d.rule_id.starts_with("ontological")),
        "prefix filter should only return ontological rows"
    );
    assert!(
        causal.iter().all(|d| d.rule_id.starts_with("causal")),
        "prefix filter should only return causal rows"
    );
}
