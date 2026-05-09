//! Integration tests for access tracking and knowledge graph data audit.
#![cfg(feature = "engine-tests")]
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after length assertions"
)]

use std::sync::Arc;

use mneme::embedding::{EmbeddingProvider, MockEmbeddingProvider};
use mneme::id::{EmbeddingId, FactId};
use mneme::knowledge::{
    EmbeddedChunk, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
    default_stability_hours,
};
use mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};

const TS_2026: &str = "2026-01-01T00:00:00Z";
fn far_future() -> jiff::Timestamp {
    mneme::knowledge::far_future()
}
const TS_RECORDED: &str = "2026-03-01T00:00:00Z";

fn ts(s: &str) -> jiff::Timestamp {
    s.parse().expect("valid timestamp")
}

fn open_store(dim: usize) -> Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig {
        dim,
        ..Default::default()
    })
    .expect("open_mem")
}

fn make_fact(id: &str, nous_id: &str, content: &str) -> Fact {
    Fact {
        id: FactId::new(id).expect("valid test id"),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        fact_type: "inference".to_owned(),
        scope: None,
        temporal: FactTemporal {
            valid_from: ts(TS_2026),
            valid_to: far_future(),
            recorded_at: ts(TS_RECORDED),
        },
        provenance: FactProvenance {
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            source_session_id: Some("ses-test".to_owned()),
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
        sensitivity: mneme::knowledge::FactSensitivity::Public,
        visibility: mneme::knowledge::Visibility::Private,
    }
}

#[test]
fn insert_fact_then_search_increments_access_count() {
    let dim = 8;
    let store = open_store(dim);
    let provider = MockEmbeddingProvider::new(dim);

    let fact = make_fact("f-track-1", "syn", "Rust is great for systems programming");
    store.insert_fact(&fact).expect("insert fact");

    let embedding = provider.embed(&fact.content).expect("embed");
    let chunk = EmbeddedChunk {
        id: EmbeddingId::new("emb-track-1").expect("valid test id"),
        content: fact.content.clone(),
        source_type: "fact".to_owned(),
        source_id: "f-track-1".to_owned(),
        nous_id: "syn".to_owned(),
        embedding: embedding.clone(),
        created_at: ts(TS_RECORDED),
    };
    store.insert_embedding(&chunk).expect("insert embedding");

    // WHY: search_vectors triggers access tracking on matched source_ids
    let results = store.search_vectors(embedding, 5, 20).expect("search");
    assert!(!results.is_empty(), "search must return results");
    assert_eq!(results[0].source_id, "f-track-1");

    let facts = store
        .query_facts("syn", "2026-06-01T00:00:00Z", 10)
        .expect("query facts");
    let tracked = facts
        .iter()
        .find(|f| f.id.as_str() == "f-track-1")
        .expect("find fact");
    assert_eq!(
        tracked.access.access_count, 1,
        "access_count should be 1 after one search"
    );
    assert!(
        tracked.access.last_accessed_at.is_some(),
        "last_accessed_at should be set"
    );
}

#[test]
fn triple_search_yields_access_count_3() {
    let dim = 8;
    let store = open_store(dim);
    let provider = MockEmbeddingProvider::new(dim);

    let fact = make_fact("f-triple", "syn", "triple access test fact");
    store.insert_fact(&fact).expect("insert fact");

    let embedding = provider.embed(&fact.content).expect("embed");
    let chunk = EmbeddedChunk {
        id: EmbeddingId::new("emb-triple").expect("valid test id"),
        content: fact.content.clone(),
        source_type: "fact".to_owned(),
        source_id: "f-triple".to_owned(),
        nous_id: "syn".to_owned(),
        embedding: embedding.clone(),
        created_at: ts(TS_RECORDED),
    };
    store.insert_embedding(&chunk).expect("insert embedding");

    for _ in 0..3 {
        store
            .search_vectors(embedding.clone(), 5, 20)
            .expect("search");
    }

    let facts = store
        .query_facts("syn", "2026-06-01T00:00:00Z", 10)
        .expect("query facts");
    let tracked = facts
        .iter()
        .find(|f| f.id.as_str() == "f-triple")
        .expect("find fact");
    assert_eq!(
        tracked.access.access_count, 3,
        "access_count should be 3 after three searches"
    );
}

#[tokio::test]
async fn increment_access_empty_ids_is_noop() {
    let store = open_store(4);
    store
        .increment_access_async(Vec::new())
        .await
        .expect("empty increment should succeed");
}

#[test]
fn default_stability_by_fact_type() {
    assert!((default_stability_hours("identity") - 17_520.0).abs() < f64::EPSILON);
    assert!((default_stability_hours("preference") - 8_760.0).abs() < f64::EPSILON);
    assert!((default_stability_hours("skill") - 4_380.0).abs() < f64::EPSILON);
    assert!((default_stability_hours("relationship") - 2_190.0).abs() < f64::EPSILON);
    assert!((default_stability_hours("event") - 720.0).abs() < f64::EPSILON);
    assert!((default_stability_hours("task") - 168.0).abs() < f64::EPSILON);
    assert!((default_stability_hours("audit") - 720.0).abs() < f64::EPSILON);
    // WHY: unknown types fall through to Observation (72 hours)
    assert!((default_stability_hours("unknown_type") - 72.0).abs() < f64::EPSILON);
}

#[test]
fn knowledge_graph_data_audit_asserts_seeded_invariants() {
    let store = open_store(4);
    let provider = MockEmbeddingProvider::new(4);

    let fact_a = make_fact(
        "f-audit-1",
        "syn",
        "audit fact with access tracking coverage",
    );
    let fact_b = make_fact("f-audit-2", "syn", "audit fact without access");
    store.insert_fact(&fact_a).expect("insert fact_a");
    store.insert_fact(&fact_b).expect("insert fact_b");

    let embedding = provider.embed(&fact_a.content).expect("embed fact_a");
    store
        .insert_embedding(&EmbeddedChunk {
            id: EmbeddingId::new("emb-audit-1").expect("valid test id"),
            content: fact_a.content.clone(),
            source_type: "fact".to_owned(),
            source_id: fact_a.id.to_string(),
            nous_id: "syn".to_owned(),
            embedding: embedding.clone(),
            created_at: ts(TS_RECORDED),
        })
        .expect("insert embedding");
    let search_results = store.search_vectors(embedding, 5, 20).expect("search");
    assert!(
        search_results.iter().any(|r| r.source_id == "f-audit-1"),
        "specific invariant: seeded audit fact must be reachable through vector search"
    );

    let facts = store
        .query_facts("syn", "2026-06-01T00:00:00Z", 1000)
        .expect("query facts");
    assert_eq!(
        facts.len(),
        2,
        "specific invariant: audit fixture should contain exactly two facts"
    );

    let entity_rows = store
        .run_query(
            "?[id, name, entity_type] := *entities{id, name, entity_type}",
            std::collections::BTreeMap::default(),
        )
        .map_or(0, |r| r.row_count());
    let rel_rows = store
        .run_query(
            "?[src, dst, relation] := *relationships{src, dst, relation}",
            std::collections::BTreeMap::default(),
        )
        .map_or(0, |r| r.row_count());

    let verified = facts
        .iter()
        .filter(|f| f.provenance.tier == EpistemicTier::Verified)
        .count();
    let inferred = facts
        .iter()
        .filter(|f| f.provenance.tier == EpistemicTier::Inferred)
        .count();
    let assumed = facts
        .iter()
        .filter(|f| f.provenance.tier == EpistemicTier::Assumed)
        .count();
    assert_eq!(
        verified, 0,
        "specific invariant: seeded fixture should have zero verified facts"
    );
    assert_eq!(
        inferred, 2,
        "specific invariant: seeded fixture should have two inferred facts"
    );
    assert_eq!(
        assumed, 0,
        "specific invariant: seeded fixture should have zero assumed facts"
    );

    let accessed = facts.iter().filter(|f| f.access.access_count > 0).count();
    let max_access = facts
        .iter()
        .map(|f| f.access.access_count)
        .max()
        .unwrap_or(0);
    let avg_access = if facts.is_empty() {
        0.0
    } else {
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "test: small counts"
        )]
        let avg = facts
            .iter()
            .map(|f| f64::from(f.access.access_count))
            .sum::<f64>()
            / facts.len() as f64;
        avg
    };
    assert_eq!(
        accessed, 1,
        "specific invariant: vector search should access exactly one seeded fact"
    );
    assert_eq!(
        max_access, 1,
        "specific invariant: max access count should reflect the single search"
    );
    assert!(
        (avg_access - 0.5).abs() < f64::EPSILON,
        "specific invariant: average access should be 0.5, got {avg_access}"
    );

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "test: small counts"
    )]
    let density = if entity_rows > 1 {
        rel_rows as f64 / (entity_rows as f64 * (entity_rows as f64 - 1.0))
    } else {
        0.0
    };
    assert_eq!(
        entity_rows, 0,
        "specific invariant: fixture should not seed graph entities"
    );
    assert_eq!(
        rel_rows, 0,
        "specific invariant: fixture should not seed graph relationships"
    );
    assert!(
        density.abs() < f64::EPSILON,
        "specific invariant: empty graph density should be zero, got {density}"
    );
}
