//! Integration tests for `KnowledgeStore`: fact round-trip (TEST-02) and HNSW vector search (TEST-03).
#![cfg(feature = "engine-tests")]
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after length assertions"
)]

use std::collections::BTreeMap;

use aletheia_mneme::embedding::{EmbeddingProvider, MockEmbeddingProvider};
use aletheia_mneme::id::{EmbeddingId, EntityId, FactId};
use aletheia_mneme::knowledge::{
    EmbeddedChunk, Entity, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance,
    FactTemporal, Relationship,
};
use aletheia_mneme::knowledge_store::{HybridQuery, KnowledgeConfig, KnowledgeStore};

const TS_2026: &str = "2026-01-01T00:00:00Z";
// WHY: jiff::Timestamp cannot parse "9999-12-31" from a string; use the
// programmatic constructor from eidos instead.
fn far_future() -> jiff::Timestamp {
    aletheia_mneme::knowledge::far_future()
}
const TS_RECORDED: &str = "2026-03-01T00:00:00Z";

fn ts(s: &str) -> jiff::Timestamp {
    s.parse().expect("valid timestamp")
}

fn make_fact(id: &str, nous_id: &str, content: &str, confidence: f64, tier: EpistemicTier) -> Fact {
    Fact {
        id: FactId::new(id).expect("valid test id"),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        fact_type: String::new(),
        temporal: FactTemporal {
            valid_from: ts(TS_2026),
            valid_to: far_future(),
            recorded_at: ts(TS_RECORDED),
        },
        provenance: FactProvenance {
            confidence,
            tier,
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
    }
}

fn make_entity(id: &str, name: &str, entity_type: &str) -> Entity {
    Entity {
        id: EntityId::new(id).expect("valid test id"),
        name: name.to_owned(),
        entity_type: entity_type.to_owned(),
        aliases: vec![],
        created_at: ts(TS_RECORDED),
        updated_at: ts(TS_RECORDED),
    }
}

fn make_relationship(src: &str, dst: &str, relation: &str, weight: f64) -> Relationship {
    Relationship {
        src: EntityId::new(src).expect("valid test id"),
        dst: EntityId::new(dst).expect("valid test id"),
        relation: relation.to_owned(),
        weight,
        created_at: ts(TS_RECORDED),
    }
}

fn make_chunk(
    id: &str,
    content: &str,
    source_id: &str,
    nous_id: &str,
    embedding: Vec<f32>,
) -> EmbeddedChunk {
    EmbeddedChunk {
        id: EmbeddingId::new(id).expect("valid test id"),
        content: content.to_owned(),
        source_type: "test".to_owned(),
        source_id: source_id.to_owned(),
        nous_id: nous_id.to_owned(),
        embedding,
        created_at: ts(TS_RECORDED),
    }
}

// TEST-02: Fact inserted via insert_fact is queryable and returns identical data.
#[test]
fn fact_round_trip() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem");

    let fact = Fact {
        id: FactId::new("f-1").expect("valid test id"),
        nous_id: "syn".to_owned(),
        content: "The researcher published findings on memory consolidation".to_owned(),
        fact_type: String::new(),
        temporal: FactTemporal {
            valid_from: ts(TS_2026),
            valid_to: far_future(),
            recorded_at: ts(TS_RECORDED),
        },
        provenance: FactProvenance {
            confidence: 0.95,
            tier: EpistemicTier::Verified,
            source_session_id: Some("ses-abc".to_owned()),
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
    };

    store.insert_fact(&fact).expect("insert");
    let results = store.query_facts("syn", "2026-06-01", 10).expect("query");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id.as_str(), "f-1");
    assert_eq!(
        results[0].content,
        "The researcher published findings on memory consolidation"
    );
    assert!((results[0].provenance.confidence - 0.95).abs() < f64::EPSILON);
    assert_eq!(results[0].provenance.tier, EpistemicTier::Verified);
}

// TEST-03: HNSW kNN returns the known-similar vector before dissimilar ones.
#[test]
fn hnsw_vector_search() {
    let dim = 16;
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");
    let provider = MockEmbeddingProvider::new(dim);

    let texts = [
        "apple", "banana", "cherry", "database", "engine", "function",
    ];
    for (i, text) in texts.iter().enumerate() {
        let embedding = provider.embed(text).expect("embed");
        let chunk = make_chunk(
            &format!("emb-{i}"),
            text,
            &format!("src-{i}"),
            "syn",
            embedding,
        );
        store.insert_embedding(&chunk).expect("insert embedding");
    }

    let query_vec = provider.embed("apple").expect("embed query");
    let results = store.search_vectors(query_vec, 3, 20).expect("search");

    assert!(!results.is_empty(), "should return results");
    assert_eq!(
        results[0].content, "apple",
        "nearest neighbor should be the exact match"
    );
    assert_eq!(results[0].source_type, "test");
    assert!(
        results[0].distance < 0.01,
        "self-distance should be near zero, got {}",
        results[0].distance
    );
}

// INTG-05: Schema version is queryable and matches SCHEMA_VERSION constant.
#[test]
fn schema_version_queryable() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem");
    let version = store.schema_version().expect("version");
    assert_eq!(version, 6);
}

// Verify ordering of multiple facts by confidence (descending).
#[test]
fn multiple_facts_ordered_by_confidence() {
    let store = KnowledgeStore::open_mem().expect("open_mem");

    for (i, conf) in [0.5f64, 0.9, 0.7].iter().enumerate() {
        let fact = make_fact(
            &format!("f-{i}"),
            "syn",
            &format!("fact {i}"),
            *conf,
            EpistemicTier::Inferred,
        );
        store.insert_fact(&fact).expect("insert");
    }

    let results = store.query_facts("syn", "2026-06-01", 10).expect("query");
    assert_eq!(results.len(), 3);
    assert!((results[0].provenance.confidence - 0.9).abs() < f64::EPSILON);
    assert!((results[1].provenance.confidence - 0.7).abs() < f64::EPSILON);
    assert!((results[2].provenance.confidence - 0.5).abs() < f64::EPSILON);
}

// INTG-04: spawn_blocking async wrappers work from #[tokio::test] context.
#[tokio::test]
async fn async_spawn_blocking_wrapper() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem");

    let fact = make_fact(
        "f-async",
        "syn",
        "async test fact",
        0.8,
        EpistemicTier::Assumed,
    );

    store.insert_fact_async(fact).await.expect("async insert");
    let results = store
        .query_facts_async("syn".to_owned(), "2026-06-01".to_owned(), 10)
        .await
        .expect("async query");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].content, "async test fact");
}

// Raw query escape hatch returns the expected relations.
#[test]
fn raw_query_escape_hatch() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem");

    let rows = store
        .run_query("::relations", BTreeMap::new())
        .expect("relations");
    assert!(
        rows.rows.len() >= 5,
        "expected at least 5 relations, got {}",
        rows.rows.len()
    );
}

// TEST-04, RETR-02, RETR-03: hybrid query fuses BM25 + vector + graph signals via RRF.
#[test]
fn hybrid_retrieval_end_to_end() {
    let dim = 8;
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

    let facts = [
        ("fact-1", "Rust is a systems programming language", 0.9f64),
        ("fact-2", "Python is used for machine learning", 0.8),
        ("fact-3", "Rust memory safety prevents data races", 0.85),
        ("fact-4", "TypeScript adds types to JavaScript", 0.7),
        ("fact-5", "Rust async runtime uses Tokio", 0.75),
    ];
    for (id, content, confidence) in &facts {
        let fact = make_fact(id, "test", content, *confidence, EpistemicTier::Inferred);
        store.insert_fact(&fact).expect("insert fact");
    }

    let rust_vec: Vec<f32> = vec![0.9, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1];
    let other_vec: Vec<f32> = vec![0.1, 0.9, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1];
    let embeddings: Vec<(&str, Vec<f32>)> = vec![
        ("fact-1", rust_vec.clone()),
        ("fact-2", other_vec.clone()),
        ("fact-3", {
            let mut v = rust_vec.clone();
            v[1] = 0.15;
            v
        }),
        ("fact-4", other_vec.clone()),
        ("fact-5", {
            let mut v = rust_vec.clone();
            v[2] = 0.15;
            v
        }),
    ];
    for (id, vec) in &embeddings {
        let chunk = make_chunk(id, id, id, "test", vec.clone());
        store.insert_embedding(&chunk).expect("insert embedding");
    }

    for (id, name) in [
        ("e-rust", "Rust"),
        ("e-python", "Python"),
        ("e-tokio", "Tokio"),
    ] {
        store
            .insert_entity(&make_entity(id, name, "technology"))
            .expect("insert entity");
    }

    store
        .insert_relationship(&make_relationship("e-rust", "e-tokio", "uses", 0.8))
        .expect("insert relationship");
    store
        .insert_relationship(&make_relationship("e-rust", "fact-1", "described_by", 0.9))
        .expect("insert relationship");

    let results = store
        .search_hybrid(&HybridQuery {
            text: "Rust programming".to_owned(),
            embedding: vec![0.9, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1],
            seed_entities: vec![EntityId::new("e-rust").expect("valid test id")],
            limit: 5,
            ef: 20,
        })
        .expect("hybrid search");

    assert!(!results.is_empty(), "hybrid search must return results");
    assert!(
        results[0].rrf_score > 0.0,
        "top result must have nonzero rrf_score, got {}",
        results[0].rrf_score
    );

    for window in results.windows(2) {
        assert!(
            window[0].rrf_score >= window[1].rrf_score,
            "results must be ordered descending: {} >= {}",
            window[0].rrf_score,
            window[1].rrf_score
        );
    }

    assert!(
        results.iter().any(|r| r.bm25_rank > 0),
        "at least one result must have nonzero bm25_rank"
    );

    assert!(
        results.iter().any(|r| r.vec_rank > 0),
        "at least one result must have nonzero vec_rank"
    );
}

// TEST-06, PERF-01: HNSW recall stays >= 95% after 5 delete+reinsert cycles.
#[test]
fn hnsw_connectivity_after_delete_reinsert_cycles() {
    let dim = 8;
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

    let primes: [u64; 8] = [7, 11, 13, 17, 19, 23, 29, 31];
    let make_vec = |i: u64| -> Vec<f32> {
        primes
            .iter()
            .enumerate()
            .map(|(d, &p)| {
                #[expect(clippy::as_conversions, reason = "test: d fits in u64")]
                let offset = (d as u64 + 1) * 37;
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::as_conversions,
                    reason = "test: modular arithmetic result fits in f32"
                )]
                let val = ((i * p + offset) % 256) as f32 / 255.0;
                val
            })
            .collect()
    };

    for i in 0u64..50 {
        let chunk = make_chunk(
            &format!("chunk-{i}"),
            &format!("chunk-{i}"),
            &format!("test-{i}"),
            "test",
            make_vec(i),
        );
        store.insert_embedding(&chunk).expect("insert embedding");
    }

    let query_vec = make_vec(25);

    let baseline = store
        .search_vectors(query_vec.clone(), 10, 40)
        .expect("baseline search");
    let baseline_ids: std::collections::HashSet<String> =
        baseline.iter().map(|r| r.source_id.clone()).collect();

    assert!(
        !baseline_ids.is_empty(),
        "baseline search must return results"
    );

    for _cycle in 0..5 {
        for i in 0u64..10 {
            let delete_script = format!("?[id] <- [[\"chunk-{i}\"]] :rm embeddings {{ id }}");
            store
                .run_mut_query(&delete_script, BTreeMap::new())
                .expect("delete embedding");
        }

        for i in 0u64..10 {
            let chunk = make_chunk(
                &format!("chunk-{i}"),
                &format!("chunk-{i}"),
                &format!("test-{i}"),
                "test",
                make_vec(i),
            );
            store.insert_embedding(&chunk).expect("reinsert embedding");
        }
    }

    let after = store
        .search_vectors(query_vec.clone(), 10, 40)
        .expect("post-cycle search");
    let after_ids: std::collections::HashSet<String> =
        after.iter().map(|r| r.source_id.clone()).collect();

    let intersection = baseline_ids.intersection(&after_ids).count();
    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "test: small counts"
    )]
    let recall = intersection as f64 / baseline_ids.len() as f64;

    assert!(
        recall >= 0.95,
        "HNSW recall after 5 delete+reinsert cycles must be >= 0.95, got {recall:.3} ({intersection}/{} results matched)",
        baseline_ids.len()
    );
}
