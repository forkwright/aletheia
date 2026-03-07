//! Integration tests for access tracking and knowledge graph data audit.
#![cfg(feature = "engine-tests")]

use aletheia_mneme::embedding::{EmbeddingProvider, MockEmbeddingProvider};
use aletheia_mneme::knowledge::{default_stability_hours, EmbeddedChunk, EpistemicTier, Fact};
use aletheia_mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};
use std::sync::Arc;

fn open_store(dim: usize) -> Arc<KnowledgeStore> {
    KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem")
}

fn make_fact(id: &str, nous_id: &str, content: &str) -> Fact {
    Fact {
        id: id.to_owned(),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        confidence: 0.9,
        tier: EpistemicTier::Inferred,
        valid_from: "2026-01-01T00:00:00Z".to_owned(),
        valid_to: "9999-12-31".to_owned(),
        superseded_by: None,
        source_session_id: Some("ses-test".to_owned()),
        recorded_at: "2026-03-01T00:00:00Z".to_owned(),
        access_count: 0,
        last_accessed_at: String::new(),
        stability_hours: 720.0,
        fact_type: "inference".to_owned(),
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
        id: "emb-track-1".to_owned(),
        content: fact.content.clone(),
        source_type: "fact".to_owned(),
        source_id: "f-track-1".to_owned(),
        nous_id: "syn".to_owned(),
        embedding: embedding.clone(),
        created_at: "2026-03-01T00:00:00Z".to_owned(),
    };
    store.insert_embedding(&chunk).expect("insert embedding");

    // Search with the same vector — should find our fact
    let results = store.search_vectors(embedding, 5, 20).expect("search");
    assert!(!results.is_empty(), "search must return results");
    assert_eq!(results[0].source_id, "f-track-1");

    // Verify access_count was incremented
    let facts = store
        .query_facts("syn", "2026-06-01T00:00:00Z", 10)
        .expect("query facts");
    let tracked = facts.iter().find(|f| f.id == "f-track-1").expect("find fact");
    assert_eq!(tracked.access_count, 1, "access_count should be 1 after one search");
    assert!(!tracked.last_accessed_at.is_empty(), "last_accessed_at should be set");
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
        id: "emb-triple".to_owned(),
        content: fact.content.clone(),
        source_type: "fact".to_owned(),
        source_id: "f-triple".to_owned(),
        nous_id: "syn".to_owned(),
        embedding: embedding.clone(),
        created_at: "2026-03-01T00:00:00Z".to_owned(),
    };
    store.insert_embedding(&chunk).expect("insert embedding");

    // Search 3 times
    for _ in 0..3 {
        store.search_vectors(embedding.clone(), 5, 20).expect("search");
    }

    let facts = store
        .query_facts("syn", "2026-06-01T00:00:00Z", 10)
        .expect("query facts");
    let tracked = facts.iter().find(|f| f.id == "f-triple").expect("find fact");
    assert_eq!(tracked.access_count, 3, "access_count should be 3 after three searches");
}

#[test]
fn increment_access_empty_ids_is_noop() {
    let store = open_store(4);
    store.increment_access(&[]).expect("empty increment should succeed");
}

#[test]
fn default_stability_by_fact_type() {
    assert!((default_stability_hours("identity") - 17520.0).abs() < f64::EPSILON);
    assert!((default_stability_hours("preference") - 8760.0).abs() < f64::EPSILON);
    assert!((default_stability_hours("relationship") - 4380.0).abs() < f64::EPSILON);
    assert!((default_stability_hours("skill") - 2190.0).abs() < f64::EPSILON);
    assert!((default_stability_hours("event") - 720.0).abs() < f64::EPSILON);
    assert!((default_stability_hours("task") - 168.0).abs() < f64::EPSILON);
    assert!((default_stability_hours("inference") - 720.0).abs() < f64::EPSILON);
    assert!((default_stability_hours("unknown_type") - 720.0).abs() < f64::EPSILON);
}

#[test]
#[ignore = "run manually: cargo test -p aletheia-integration-tests --features engine-tests audit -- --ignored --nocapture"]
fn knowledge_graph_data_audit() {
    let store = open_store(4);

    // Query facts, entities, relationships
    let facts = store.query_facts("", "9999-12-31", 1000).unwrap_or_default();

    let entity_rows = store
        .run_query("?[id, name, entity_type] := *entities{id, name, entity_type}", Default::default())
        .map(|r| r.rows.len())
        .unwrap_or(0);

    let rel_rows = store
        .run_query("?[src, dst, relation] := *relationships{src, dst, relation}", Default::default())
        .map(|r| r.rows.len())
        .unwrap_or(0);

    println!("\n=== Knowledge Graph Data Audit ===");
    println!("Facts:         {}", facts.len());
    println!("Entities:      {entity_rows}");
    println!("Relationships: {rel_rows}");

    // Fact tier distribution
    let verified = facts.iter().filter(|f| f.tier == EpistemicTier::Verified).count();
    let inferred = facts.iter().filter(|f| f.tier == EpistemicTier::Inferred).count();
    let assumed = facts.iter().filter(|f| f.tier == EpistemicTier::Assumed).count();
    println!("\nFact tiers:");
    println!("  Verified:  {verified}");
    println!("  Inferred:  {inferred}");
    println!("  Assumed:   {assumed}");

    // Access tracking stats
    let accessed = facts.iter().filter(|f| f.access_count > 0).count();
    let max_access = facts.iter().map(|f| f.access_count).max().unwrap_or(0);
    let avg_access = if facts.is_empty() {
        0.0
    } else {
        facts.iter().map(|f| f64::from(f.access_count)).sum::<f64>() / facts.len() as f64
    };
    println!("\nAccess tracking:");
    println!("  Facts accessed:    {accessed}/{}", facts.len());
    println!("  Max access count:  {max_access}");
    println!("  Avg access count:  {avg_access:.2}");

    // Graph density
    let density = if entity_rows > 1 {
        rel_rows as f64 / (entity_rows as f64 * (entity_rows as f64 - 1.0))
    } else {
        0.0
    };
    println!("\nGraph density: {density:.4}");

    // Phase F viability
    println!("\n--- Phase F Viability ---");
    if facts.is_empty() {
        println!("No data yet. Phase F has nothing to calibrate against.");
    } else {
        println!("Facts with access data: {accessed}/{} ({:.0}%)", facts.len(), accessed as f64 / facts.len() as f64 * 100.0);
        if accessed > 10 {
            println!("Sufficient access data for initial FSRS calibration.");
        } else {
            println!("Need more access data before FSRS calibration is viable.");
        }
    }
    println!("=================================\n");
}
