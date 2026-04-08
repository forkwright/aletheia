//! Integration tests for access tracking and knowledge graph data audit.
#![cfg(feature = "engine-tests")]
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after length assertions"
)]

use std::sync::Arc;

use aletheia_mneme::embedding::{EmbeddingProvider, MockEmbeddingProvider};
use aletheia_mneme::id::{EmbeddingId, FactId};
use aletheia_mneme::knowledge::{
    EmbeddedChunk, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactTemporal,
    default_stability_hours,
};
use aletheia_mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};

const TS_2026: &str = "2026-01-01T00:00:00Z";
fn far_future() -> jiff::Timestamp {
    aletheia_mneme::knowledge::far_future()
}
const TS_RECORDED: &str = "2026-03-01T00:00:00Z";

fn ts(s: &str) -> jiff::Timestamp {
    s.parse().expect("valid timestamp")
}

fn open_store(dim: usize) -> Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem")
}

fn make_fact(id: &str, nous_id: &str, content: &str) -> Fact {
    Fact {
        id: FactId::new(id).expect("valid test id"),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        fact_type: "inference".to_owned(),
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

#[test]
fn increment_access_empty_ids_is_noop() {
    let store = open_store(4);
    store
        .increment_access(&[])
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
#[ignore = "run manually: cargo test -p aletheia-integration-tests --features engine-tests audit -- --ignored --nocapture"]
fn knowledge_graph_data_audit() {
    let store = open_store(4);

    let facts = store
        .query_facts("", "9999-12-31", 1000)
        .unwrap_or_default();

    let entity_rows = store
        .run_query(
            "?[id, name, entity_type] := *entities{id, name, entity_type}",
            Default::default(),
        )
        .map(|r| r.rows.len())
        .unwrap_or(0);

    let rel_rows = store
        .run_query(
            "?[src, dst, relation] := *relationships{src, dst, relation}",
            Default::default(),
        )
        .map(|r| r.rows.len())
        .unwrap_or(0);

    println!("\n=== Knowledge Graph Data Audit ===");
    println!("Facts:         {}", facts.len());
    println!("Entities:      {entity_rows}");
    println!("Relationships: {rel_rows}");

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
    println!("\nFact tiers:");
    println!("  Verified:  {verified}");
    println!("  Inferred:  {inferred}");
    println!("  Assumed:   {assumed}");

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
    println!("\nAccess tracking:");
    println!("  Facts accessed:    {accessed}/{}", facts.len());
    println!("  Max access count:  {max_access}");
    println!("  Avg access count:  {avg_access:.2}");

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
    println!("\nGraph density: {density:.4}");

    println!("\n--- Phase F Viability ---");
    if facts.is_empty() {
        println!("No data yet. Phase F has nothing to calibrate against.");
    } else {
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "test: small counts"
        )]
        let pct = accessed as f64 / facts.len() as f64 * 100.0;
        println!(
            "Facts with access data: {accessed}/{} ({pct:.0}%)",
            facts.len(),
        );
        if accessed > 10 {
            println!("Sufficient access data for initial FSRS calibration.");
        } else {
            println!("Need more access data before FSRS calibration is viable.");
        }
    }
    println!("=================================\n");
}
