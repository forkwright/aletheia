#![cfg_attr(
    feature = "mneme-engine",
    expect(
        clippy::expect_used,
        reason = "test assertions in feature-gated engine tests"
    )
)]
#![expect(
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use super::super::*;

#[test]
fn ddl_templates_are_valid_strings() {
    assert!(KNOWLEDGE_DDL.len() == 6);
    let emb = embeddings_ddl(1024);
    assert!(emb.contains("1024"));
    let idx = hnsw_ddl(1024);
    assert!(idx.contains("1024"));
    let fts = fts_ddl();
    assert!(fts.contains("content_fts"));
    assert!(fts.contains("bm25") || fts.contains("Simple"));
}

#[cfg(feature = "mneme-engine")]
#[test]
fn query_templates_contain_params() {
    let current = queries::current_facts();
    assert!(current.contains("$nous_id"));
    assert!(current.contains("$now"));
    assert!(queries::SEMANTIC_SEARCH.contains("$query_vec"));
    assert!(queries::ENTITY_NEIGHBORHOOD.contains("$entity_id"));
    let supersede = queries::supersede_fact();
    assert!(supersede.contains("$old_id"));
    assert!(supersede.contains("$new_id"));
    assert!(queries::HYBRID_SEARCH_BASE.contains("$query_text"));
    assert!(queries::HYBRID_SEARCH_BASE.contains("$query_vec"));
    assert!(queries::HYBRID_SEARCH_BASE.contains("ReciprocalRankFusion"));
}

#[cfg(feature = "mneme-engine")]
#[test]
fn build_hybrid_query_empty_seeds() {
    let q = HybridQuery {
        text: "test".into(),
        embedding: vec![0.0; 4],
        seed_entities: vec![],
        limit: 5,
        ef: 20,
    };
    let script = marshal::build_hybrid_query(&q);
    assert!(
        script.contains("graph[id, score] <- []"),
        "empty seeds must produce empty graph relation"
    );
    assert!(script.contains("ReciprocalRankFusion"));
}

#[cfg(feature = "mneme-engine")]
#[test]
fn build_hybrid_query_with_seeds() {
    let q = HybridQuery {
        text: "test".into(),
        embedding: vec![0.0; 4],
        seed_entities: vec!["e-rust".into(), "e-python".into()],
        limit: 5,
        ef: 20,
    };
    let script = marshal::build_hybrid_query(&q);
    assert!(
        script.contains("seed_list"),
        "non-empty seeds must produce seed_list relation"
    );
    assert!(script.contains("e-rust"));
    assert!(script.contains("e-python"));
    assert!(script.contains("*relationships"));
    assert!(
        script.contains("graph_raw"),
        "must use graph_raw intermediate for aggregation"
    );
    assert!(
        script.contains("sum(score)"),
        "must aggregate scores per entity"
    );
}

#[cfg(feature = "mneme-engine")]
#[test]
fn build_hybrid_query_accepts_valid_seed_ids() {
    let valid_seeds = ["e-1", "some_entity", "CamelCase123", "a-b_c"];
    for seed in valid_seeds {
        let q = HybridQuery {
            text: "test".into(),
            embedding: vec![0.0; 4],
            seed_entities: vec![crate::id::EntityId::from(seed)],
            limit: 5,
            ef: 20,
        };
        let script = marshal::build_hybrid_query(&q);
        assert!(
            !script.is_empty(),
            "valid seed {seed:?} must produce a non-empty script"
        );
    }
}

#[cfg(feature = "mneme-engine")]
#[test]
fn hybrid_search_empty_seeds_returns_results() {
    use crate::knowledge::{EmbeddedChunk, EpistemicTier, Fact};

    let dim = 4;
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

    let fact = Fact {
        id: crate::id::FactId::new_unchecked("f1"),
        nous_id: "test".to_owned(),
        content: "Rust systems programming".to_owned(),
        confidence: 0.9,
        tier: EpistemicTier::Inferred,
        valid_from: crate::knowledge::parse_timestamp("2026-01-01").expect("valid test timestamp"),
        valid_to: crate::knowledge::far_future(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
            .expect("valid test timestamp"),
        access_count: 0,
        last_accessed_at: None,
        stability_hours: 720.0,
        fact_type: String::new(),
        is_forgotten: false,
        forgotten_at: None,
        forget_reason: None,
    };
    store.insert_fact(&fact).expect("insert fact");

    let chunk = EmbeddedChunk {
        id: crate::id::EmbeddingId::new_unchecked("f1"),
        content: "Rust systems programming".to_owned(),
        source_type: "fact".to_owned(),
        source_id: "f1".to_owned(),
        nous_id: "test".to_owned(),
        embedding: vec![0.9, 0.1, 0.1, 0.1],
        created_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
            .expect("valid test timestamp"),
    };
    store.insert_embedding(&chunk).expect("insert embedding");

    let results = store
        .search_hybrid(&HybridQuery {
            text: "Rust programming".to_owned(),
            embedding: vec![0.9, 0.1, 0.1, 0.1],
            seed_entities: vec![],
            limit: 5,
            ef: 20,
        })
        .expect("hybrid search with empty seeds");

    assert!(
        !results.is_empty(),
        "empty seeds must still return BM25+vec results"
    );
    for r in &results {
        assert_eq!(
            r.graph_rank, -1,
            "graph_rank must be -1 when seeds are empty"
        );
    }
}

#[cfg(feature = "mneme-engine")]
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "integration test with setup/assert phases"
)]
fn hybrid_search_graph_aggregation() {
    use crate::knowledge::{EmbeddedChunk, Entity, EpistemicTier, Fact, Relationship};

    let dim = 4;
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

    let f1 = Fact {
        id: crate::id::FactId::new_unchecked("f1"),
        nous_id: "test".to_owned(),
        content: "Rust systems programming".to_owned(),
        confidence: 0.9,
        tier: EpistemicTier::Inferred,
        valid_from: crate::knowledge::parse_timestamp("2026-01-01").expect("valid test timestamp"),
        valid_to: crate::knowledge::far_future(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
            .expect("valid test timestamp"),
        access_count: 0,
        last_accessed_at: None,
        stability_hours: 720.0,
        fact_type: String::new(),
        is_forgotten: false,
        forgotten_at: None,
        forget_reason: None,
    };
    store.insert_fact(&f1).expect("insert f1");
    store
        .insert_embedding(&EmbeddedChunk {
            id: crate::id::EmbeddingId::new_unchecked("f1"),
            content: "Rust systems programming".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "f1".to_owned(),
            nous_id: "test".to_owned(),
            embedding: vec![0.9, 0.1, 0.1, 0.1],
            created_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                .expect("valid test timestamp"),
        })
        .expect("insert f1 embedding");

    let f2 = Fact {
        id: crate::id::FactId::new_unchecked("f2"),
        nous_id: "test".to_owned(),
        content: "Rust memory safety".to_owned(),
        confidence: 0.9,
        tier: EpistemicTier::Inferred,
        valid_from: crate::knowledge::parse_timestamp("2026-01-01").expect("valid test timestamp"),
        valid_to: crate::knowledge::far_future(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
            .expect("valid test timestamp"),
        access_count: 0,
        last_accessed_at: None,
        stability_hours: 720.0,
        fact_type: String::new(),
        is_forgotten: false,
        forgotten_at: None,
        forget_reason: None,
    };
    store.insert_fact(&f2).expect("insert f2");
    store
        .insert_embedding(&EmbeddedChunk {
            id: crate::id::EmbeddingId::new_unchecked("f2"),
            content: "Rust memory safety".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "f2".to_owned(),
            nous_id: "test".to_owned(),
            embedding: vec![0.8, 0.2, 0.1, 0.1],
            created_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                .expect("valid test timestamp"),
        })
        .expect("insert f2 embedding");

    for (id, name) in [("s1", "Seed1"), ("s2", "Seed2"), ("s3", "Seed3")] {
        store
            .insert_entity(&Entity {
                id: crate::id::EntityId::new_unchecked(id),
                name: name.to_owned(),
                entity_type: "concept".to_owned(),
                aliases: vec![],
                created_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                    .expect("valid test timestamp"),
                updated_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                    .expect("valid test timestamp"),
            })
            .expect("insert entity");
        store
            .insert_relationship(&Relationship {
                src: crate::id::EntityId::new_unchecked(id),
                dst: crate::id::EntityId::new_unchecked("f1"),
                relation: "describes".to_owned(),
                weight: 0.7,
                created_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                    .expect("valid test timestamp"),
            })
            .expect("insert relationship to f1");
    }
    store
        .insert_relationship(&Relationship {
            src: crate::id::EntityId::new_unchecked("s1"),
            dst: crate::id::EntityId::new_unchecked("f2"),
            relation: "describes".to_owned(),
            weight: 0.7,
            created_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                .expect("valid test timestamp"),
        })
        .expect("insert relationship to f2");

    let results = store
        .search_hybrid(&HybridQuery {
            text: "Rust programming".to_owned(),
            embedding: vec![0.9, 0.1, 0.1, 0.1],
            seed_entities: vec![
                crate::id::EntityId::new_unchecked("s1"),
                crate::id::EntityId::new_unchecked("s2"),
                crate::id::EntityId::new_unchecked("s3"),
            ],
            limit: 10,
            ef: 20,
        })
        .expect("hybrid search with three seeds");

    let f1_hits: Vec<_> = results.iter().filter(|r| r.id.as_str() == "f1").collect();
    assert_eq!(
        f1_hits.len(),
        1,
        "entity reachable via multiple paths must appear once"
    );
    assert!(
        f1_hits[0].graph_rank > 0,
        "f1 must have a positive graph rank"
    );

    let f2_hits: Vec<_> = results.iter().filter(|r| r.id.as_str() == "f2").collect();
    assert_eq!(f2_hits.len(), 1, "f2 must appear once");
    assert!(
        f2_hits[0].graph_rank > 0,
        "f2 must have a positive graph rank"
    );

    assert!(
        f1_hits[0].rrf_score > f2_hits[0].rrf_score,
        "3-path entity must score higher than 1-path entity: f1={} vs f2={}",
        f1_hits[0].rrf_score,
        f2_hits[0].rrf_score,
    );
}

#[cfg(feature = "mneme-engine")]
#[test]
fn hybrid_search_two_signal_no_graph() {
    use crate::knowledge::{EmbeddedChunk, Entity, EpistemicTier, Fact};

    let dim = 4;
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

    let fact = Fact {
        id: crate::id::FactId::new_unchecked("f-twosig"),
        nous_id: "test".to_owned(),
        content: "unique harpsichord melody testing".to_owned(),
        confidence: 0.9,
        tier: EpistemicTier::Inferred,
        valid_from: crate::knowledge::parse_timestamp("2026-01-01").expect("valid test timestamp"),
        valid_to: crate::knowledge::far_future(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
            .expect("valid test timestamp"),
        access_count: 0,
        last_accessed_at: None,
        stability_hours: 720.0,
        fact_type: String::new(),
        is_forgotten: false,
        forgotten_at: None,
        forget_reason: None,
    };
    store.insert_fact(&fact).expect("insert fact");

    store
        .insert_embedding(&EmbeddedChunk {
            id: crate::id::EmbeddingId::new_unchecked("f-twosig"),
            content: "unique harpsichord melody testing".to_owned(),
            source_type: "fact".to_owned(),
            source_id: "f-twosig".to_owned(),
            nous_id: "test".to_owned(),
            embedding: vec![0.7, 0.3, 0.2, 0.1],
            created_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                .expect("valid test timestamp"),
        })
        .expect("insert embedding");

    store
        .insert_entity(&Entity {
            id: crate::id::EntityId::new_unchecked("e-unrelated"),
            name: "Unrelated".to_owned(),
            entity_type: "concept".to_owned(),
            aliases: vec![],
            created_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                .expect("valid test timestamp"),
            updated_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                .expect("valid test timestamp"),
        })
        .expect("insert entity");

    let results = store
        .search_hybrid(&HybridQuery {
            text: "harpsichord melody".to_owned(),
            embedding: vec![0.7, 0.3, 0.2, 0.1],
            seed_entities: vec![crate::id::EntityId::new_unchecked("e-unrelated")],
            limit: 5,
            ef: 20,
        })
        .expect("hybrid search two signals");

    let hit = results.iter().find(|r| r.id.as_str() == "f-twosig");
    assert!(hit.is_some(), "BM25+vector fact must appear in results");
    let hit = hit.expect("f-twosig must appear in hybrid results");
    assert!(hit.bm25_rank > 0, "must have positive BM25 rank");
    assert!(hit.vec_rank > 0, "must have positive vector rank");
    assert_eq!(hit.graph_rank, -1, "absent from graph signal must be -1");
    assert!(
        hit.rrf_score > 0.0,
        "RRF score must be positive from two signals"
    );
}

#[cfg(feature = "mneme-engine")]
#[test]
fn hybrid_search_absent_signal_rank_is_negative_one() {
    use crate::knowledge::{EpistemicTier, Fact};

    let dim = 4;
    let store = KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

    let fact = Fact {
        id: crate::id::FactId::new_unchecked("f-bm25-only"),
        nous_id: "test".to_owned(),
        content: "unique xylophone testing keyword".to_owned(),
        confidence: 0.9,
        tier: EpistemicTier::Inferred,
        valid_from: crate::knowledge::parse_timestamp("2026-01-01").expect("valid test timestamp"),
        valid_to: crate::knowledge::far_future(),
        superseded_by: None,
        source_session_id: None,
        recorded_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
            .expect("valid test timestamp"),
        access_count: 0,
        last_accessed_at: None,
        stability_hours: 720.0,
        fact_type: String::new(),
        is_forgotten: false,
        forgotten_at: None,
        forget_reason: None,
    };
    store.insert_fact(&fact).expect("insert fact");

    let results = store
        .search_hybrid(&HybridQuery {
            text: "xylophone testing".to_owned(),
            embedding: vec![0.5, 0.5, 0.5, 0.5],
            seed_entities: vec![],
            limit: 5,
            ef: 20,
        })
        .expect("hybrid search bm25-only");

    let hit = results.iter().find(|r| r.id.as_str() == "f-bm25-only");
    assert!(hit.is_some(), "BM25-only fact must appear in results");
    let hit = hit.expect("f-bm25-only must appear in hybrid results");
    assert!(hit.bm25_rank > 0, "must have positive BM25 rank");
    assert_eq!(hit.vec_rank, -1, "absent from vector signal must be -1");
    assert_eq!(hit.graph_rank, -1, "absent from graph signal must be -1");
}
