//! Integration tests for `KnowledgeStore`: fact round-trip (TEST-02) and HNSW vector search (TEST-03).
#![cfg(feature = "engine-tests")]

use aletheia_mneme::embedding::{EmbeddingProvider, MockEmbeddingProvider};
use aletheia_mneme::knowledge::{EmbeddedChunk, Entity, EpistemicTier, Fact, Relationship};
use aletheia_mneme::knowledge_store::{HybridQuery, KnowledgeConfig, KnowledgeStore};
use std::collections::BTreeMap;

fn make_fact(id: &str, nous_id: &str, content: &str, confidence: f64, tier: EpistemicTier) -> Fact {
    Fact {
        id: id.to_owned(),
        nous_id: nous_id.to_owned(),
        content: content.to_owned(),
        confidence,
        tier,
        valid_from: "2026-01-01".to_owned(),
        valid_to: "9999-12-31".to_owned(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: "2026-03-01T00:00:00Z".to_owned(),
    }
}

// TEST-02: Fact inserted via insert_fact is queryable and returns identical data.
#[test]
fn fact_round_trip() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem");

    let fact = Fact {
        id: "f-1".to_owned(),
        nous_id: "syn".to_owned(),
        content: "Alice lives in Springfield".to_owned(),
        confidence: 0.95,
        tier: EpistemicTier::Verified,
        valid_from: "2026-01-01".to_owned(),
        valid_to: "9999-12-31".to_owned(),
        superseded_by: None,
        source_session_id: Some("ses-abc".to_owned()),
        recorded_at: "2026-03-01T00:00:00Z".to_owned(),
    };

    store.insert_fact(&fact).expect("insert");
    let results = store.query_facts("syn", "2026-06-01", 10).expect("query");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "f-1");
    assert_eq!(results[0].content, "Alice lives in Springfield");
    assert!((results[0].confidence - 0.95).abs() < f64::EPSILON);
    assert_eq!(results[0].tier, EpistemicTier::Verified);
}

// TEST-03: HNSW kNN returns the known-similar vector before dissimilar ones.
#[test]
fn hnsw_vector_search() {
    let dim = 16;
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");
    // MockEmbeddingProvider is deterministic by text input — satisfies the
    // "deterministic random vectors" constraint from CONTEXT.md.
    let provider = MockEmbeddingProvider::new(dim);

    let texts = [
        "apple", "banana", "cherry", "database", "engine", "function",
    ];
    for (i, text) in texts.iter().enumerate() {
        let embedding = provider.embed(text).expect("embed");
        let chunk = EmbeddedChunk {
            id: format!("emb-{i}"),
            content: text.to_string(),
            source_type: "test".to_owned(),
            source_id: format!("src-{i}"),
            nous_id: "syn".to_owned(),
            embedding,
            created_at: "2026-03-01T00:00:00Z".to_owned(),
        };
        store.insert_embedding(&chunk).expect("insert embedding");
    }

    // Query with the "apple" vector — should find "apple" as nearest neighbor.
    let query_vec = provider.embed("apple").expect("embed query");
    let results = store.search_vectors(query_vec, 3, 20).expect("search");

    assert!(!results.is_empty(), "should return results");
    assert_eq!(
        results[0].content, "apple",
        "nearest neighbor should be the exact match"
    );
    assert_eq!(results[0].source_type, "test");
    // Cosine distance to identical vector should be ~0.
    assert!(
        results[0].distance < 0.01,
        "self-distance should be near zero, got {}",
        results[0].distance
    );
}

// INTG-05: Schema version is queryable and returns 1.
#[test]
fn schema_version_queryable() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem");
    let version = store.schema_version().expect("version");
    assert_eq!(version, 1);
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
    // FULL_CURRENT_FACTS orders by -confidence
    assert!((results[0].confidence - 0.9).abs() < f64::EPSILON);
    assert!((results[1].confidence - 0.7).abs() < f64::EPSILON);
    assert!((results[2].confidence - 0.5).abs() < f64::EPSILON);
}

// INTG-04: spawn_blocking async wrappers work from #[tokio::test] context.
#[tokio::test]
async fn async_spawn_blocking_wrapper() {
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem");

    let fact = Fact {
        id: "f-async".to_owned(),
        nous_id: "syn".to_owned(),
        content: "async test fact".to_owned(),
        confidence: 0.8,
        tier: EpistemicTier::Assumed,
        valid_from: "2026-01-01".to_owned(),
        valid_to: "9999-12-31".to_owned(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: "2026-03-01T00:00:00Z".to_owned(),
    };

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
    // Should have at least: facts, entities, relationships, embeddings, _schema_version
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

    // Insert 5 facts with distinct topics.
    let facts = [
        ("fact-1", "Rust is a systems programming language", 0.9f64),
        ("fact-2", "Python is used for machine learning", 0.8),
        ("fact-3", "Rust memory safety prevents data races", 0.85),
        ("fact-4", "TypeScript adds types to JavaScript", 0.7),
        ("fact-5", "Rust async runtime uses Tokio", 0.75),
    ];
    for (id, content, confidence) in &facts {
        let fact = Fact {
            id: id.to_string(),
            nous_id: "test".to_string(),
            content: content.to_string(),
            confidence: *confidence,
            tier: EpistemicTier::Inferred,
            valid_from: "2026-01-01".to_owned(),
            valid_to: "9999-12-31".to_owned(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: "2026-03-01T00:00:00Z".to_owned(),
        };
        store.insert_fact(&fact).expect("insert fact");
    }

    // Insert 5 embeddings — Rust facts get a similar vector cluster.
    // fact-1, fact-3, fact-5: Rust-related [0.9, 0.1, ...]; fact-2, fact-4: other [0.1, 0.9, ...]
    let rust_vec: Vec<f32> = vec![0.9, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1];
    let other_vec: Vec<f32> = vec![0.1, 0.9, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1];
    let embeddings = [
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
        let chunk = EmbeddedChunk {
            id: id.to_string(),
            content: id.to_string(),
            source_type: "fact".to_owned(),
            source_id: id.to_string(),
            nous_id: "test".to_owned(),
            embedding: vec.clone(),
            created_at: "2026-03-01T00:00:00Z".to_owned(),
        };
        store.insert_embedding(&chunk).expect("insert embedding");
    }

    // Insert entities and relationships for graph signal.
    for (id, name) in [
        ("e-rust", "Rust"),
        ("e-python", "Python"),
        ("e-tokio", "Tokio"),
    ] {
        let entity = Entity {
            id: id.to_owned(),
            name: name.to_owned(),
            entity_type: "technology".to_owned(),
            aliases: vec![],
            created_at: "2026-03-01T00:00:00Z".to_owned(),
            updated_at: "2026-03-01T00:00:00Z".to_owned(),
        };
        store.insert_entity(&entity).expect("insert entity");
    }
    // e-rust -> e-tokio (Rust uses Tokio async runtime)
    store
        .insert_relationship(&Relationship {
            src: "e-rust".to_owned(),
            dst: "e-tokio".to_owned(),
            relation: "uses".to_owned(),
            weight: 0.8,
            created_at: "2026-03-01T00:00:00Z".to_owned(),
        })
        .expect("insert relationship");
    // e-rust -> fact-1 (Rust fact)
    store
        .insert_relationship(&Relationship {
            src: "e-rust".to_owned(),
            dst: "fact-1".to_owned(),
            relation: "described_by".to_owned(),
            weight: 0.9,
            created_at: "2026-03-01T00:00:00Z".to_owned(),
        })
        .expect("insert relationship");

    // Run hybrid search with seed entity e-rust.
    let results = store
        .search_hybrid(&HybridQuery {
            text: "Rust programming".to_owned(),
            embedding: vec![0.9, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1],
            seed_entities: vec!["e-rust".to_owned()],
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

    // Results must be ordered by rrf_score descending.
    for window in results.windows(2) {
        assert!(
            window[0].rrf_score >= window[1].rrf_score,
            "results must be ordered descending: {} >= {}",
            window[0].rrf_score,
            window[1].rrf_score
        );
    }

    // At least one result must have a nonzero bm25_rank (text signal contributed).
    assert!(
        results.iter().any(|r| r.bm25_rank > 0),
        "at least one result must have nonzero bm25_rank"
    );

    // At least one result must have a nonzero vec_rank (vector signal contributed).
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

    // Generate 50 deterministic pseudo-random 8-dim vectors using different primes per dimension.
    let primes: [u64; 8] = [7, 11, 13, 17, 19, 23, 29, 31];
    let make_vec = |i: u64| -> Vec<f32> {
        primes
            .iter()
            .enumerate()
            .map(|(d, &p)| {
                let offset = (d as u64 + 1) * 37;
                ((i * p + offset) % 256) as f32 / 255.0
            })
            .collect()
    };

    // Insert 50 embeddings.
    for i in 0u64..50 {
        let chunk = EmbeddedChunk {
            id: format!("chunk-{i}"),
            content: format!("chunk-{i}"),
            source_type: "test".to_owned(),
            source_id: format!("test-{i}"),
            nous_id: "test".to_owned(),
            embedding: make_vec(i),
            created_at: "2026-03-01T00:00:00Z".to_owned(),
        };
        store.insert_embedding(&chunk).expect("insert embedding");
    }

    // Use chunk-25 as query vector.
    let query_vec = make_vec(25);

    // Baseline recall@10.
    let baseline = store
        .search_vectors(query_vec.clone(), 10, 40)
        .expect("baseline search");
    let baseline_ids: std::collections::HashSet<String> =
        baseline.iter().map(|r| r.source_id.clone()).collect();

    assert!(
        !baseline_ids.is_empty(),
        "baseline search must return results"
    );

    // 5 delete+reinsert cycles for chunk-0..chunk-9.
    for _cycle in 0..5 {
        // Delete chunk-0..chunk-9 via :rm
        for i in 0u64..10 {
            let delete_script = format!("?[id] <- [[\"chunk-{i}\"]] :rm embeddings {{ id }}");
            store
                .run_mut_query(&delete_script, BTreeMap::new())
                .expect("delete embedding");
        }

        // Reinsert chunk-0..chunk-9 with same IDs and vectors.
        for i in 0u64..10 {
            let chunk = EmbeddedChunk {
                id: format!("chunk-{i}"),
                content: format!("chunk-{i}"),
                source_type: "test".to_owned(),
                source_id: format!("test-{i}"),
                nous_id: "test".to_owned(),
                embedding: make_vec(i),
                created_at: "2026-03-01T00:00:00Z".to_owned(),
            };
            store.insert_embedding(&chunk).expect("reinsert embedding");
        }
    }

    // Post-cycle recall@10.
    let after = store
        .search_vectors(query_vec.clone(), 10, 40)
        .expect("post-cycle search");
    let after_ids: std::collections::HashSet<String> =
        after.iter().map(|r| r.source_id.clone()).collect();

    let intersection = baseline_ids.intersection(&after_ids).count();
    let recall = intersection as f64 / baseline_ids.len() as f64;

    assert!(
        recall >= 0.95,
        "HNSW recall after 5 delete+reinsert cycles must be >= 0.95, got {:.3} ({}/{} results matched)",
        recall,
        intersection,
        baseline_ids.len()
    );
}
