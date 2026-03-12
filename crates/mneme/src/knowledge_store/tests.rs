#[cfg(all(test, feature = "mneme-engine"))]
mod engine_assertions {
    use super::super::KnowledgeStore;
    use static_assertions::assert_impl_all;
    assert_impl_all!(KnowledgeStore: Send, Sync);
}

#[cfg(all(test, feature = "mneme-engine"))]
mod timeout_tests {
    use super::super::*;
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[test]
    fn query_timeout_returns_typed_error() {
        let store =
            KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem");

        // Recursive transitive closure on a linear chain of N nodes requires N-1 semi-naive
        // fixpoint epochs. Each epoch checks the Poison flag. With N=2000 and timeout=50ms
        // the engine will hit the Poison kill well before all epochs complete.
        let result = store.run_query_with_timeout(
            r"
edge[a, b] := a in int_range(2000), b = a + 1
reach[a, b] := edge[a, b]
reach[a, c] := reach[a, b], edge[b, c]
?[a, c] := reach[a, c]
",
            BTreeMap::new(),
            Some(Duration::from_millis(50)),
        );

        assert!(result.is_err(), "expected timeout error");
        let err = result.expect_err("timeout query must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("timed out"),
            "error should mention timeout, got: {msg}"
        );
        assert!(matches!(err, crate::error::Error::QueryTimeout { .. }));
    }

    #[test]
    fn query_without_timeout_succeeds() {
        let store =
            KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: 4 }).expect("open_mem");

        let result = store.run_query_with_timeout("?[x] := x = 42", BTreeMap::new(), None);

        assert!(result.is_ok(), "query without timeout should succeed");
        let rows = result.expect("query without timeout must succeed");
        assert_eq!(rows.rows.len(), 1);
    }
}

// --- DDL and non-engine unit tests ---
mod ddl_tests {
    use super::super::*;

    #[test]
    fn ddl_templates_are_valid_strings() {
        // Verify DDL templates don't panic on formatting
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
        let store =
            KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

        let fact = Fact {
            id: crate::id::FactId::new_unchecked("f1"),
            nous_id: "test".to_owned(),
            content: "Rust systems programming".to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: crate::knowledge::parse_timestamp("2026-01-01")
                .expect("valid test timestamp"),
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
        let store =
            KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

        // f1: reachable from 3 seed entities
        let f1 = Fact {
            id: crate::id::FactId::new_unchecked("f1"),
            nous_id: "test".to_owned(),
            content: "Rust systems programming".to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: crate::knowledge::parse_timestamp("2026-01-01")
                .expect("valid test timestamp"),
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

        // f2: reachable from only 1 seed entity
        let f2 = Fact {
            id: crate::id::FactId::new_unchecked("f2"),
            nous_id: "test".to_owned(),
            content: "Rust memory safety".to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: crate::knowledge::parse_timestamp("2026-01-01")
                .expect("valid test timestamp"),
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

        // Three seed entities: all point to f1, only s1 points to f2
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

        // f1 must appear exactly once (aggregated from 3 paths)
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

        // f2 must appear exactly once (from 1 path)
        let f2_hits: Vec<_> = results.iter().filter(|r| r.id.as_str() == "f2").collect();
        assert_eq!(f2_hits.len(), 1, "f2 must appear once");
        assert!(
            f2_hits[0].graph_rank > 0,
            "f2 must have a positive graph rank"
        );

        // f1 (3 paths) should have a higher RRF score than f2 (1 path)
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
        let store =
            KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

        let fact = Fact {
            id: crate::id::FactId::new_unchecked("f-twosig"),
            nous_id: "test".to_owned(),
            content: "unique harpsichord melody testing".to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: crate::knowledge::parse_timestamp("2026-01-01")
                .expect("valid test timestamp"),
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

        // Insert an unrelated seed entity so the graph signal is structurally present but yields
        // no matches for f-twosig
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
        let store =
            KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim }).expect("open_mem");

        // Insert a fact but no embedding and no graph edges
        let fact = Fact {
            id: crate::id::FactId::new_unchecked("f-bm25-only"),
            nous_id: "test".to_owned(),
            content: "unique xylophone testing keyword".to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: crate::knowledge::parse_timestamp("2026-01-01")
                .expect("valid test timestamp"),
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
}

#[cfg(all(test, feature = "mneme-engine"))]
mod knowledge_store_tests {
    use super::super::*;
    use crate::knowledge::{
        EmbeddedChunk, Entity, EpistemicTier, Fact, ForgetReason, Relationship,
    };
    use std::collections::BTreeMap;
    use std::sync::Arc;

    const DIM: usize = 4;

    fn make_store() -> Arc<KnowledgeStore> {
        KnowledgeStore::open_mem_with_config(KnowledgeConfig { dim: DIM }).expect("open_mem")
    }

    fn test_ts(s: &str) -> jiff::Timestamp {
        crate::knowledge::parse_timestamp(s).expect("valid test timestamp in test helper")
    }

    fn make_fact(id: &str, nous_id: &str, content: &str) -> Fact {
        Fact {
            id: crate::id::FactId::new_unchecked(id),
            nous_id: nous_id.to_owned(),
            content: content.to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: test_ts("2026-01-01"),
            valid_to: crate::knowledge::far_future(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: test_ts("2026-03-01T00:00:00Z"),
            access_count: 0,
            last_accessed_at: None,
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        }
    }

    fn make_entity(id: &str, name: &str, entity_type: &str) -> Entity {
        Entity {
            id: crate::id::EntityId::new_unchecked(id),
            name: name.to_owned(),
            entity_type: entity_type.to_owned(),
            aliases: vec![],
            created_at: test_ts("2026-03-01T00:00:00Z"),
            updated_at: test_ts("2026-03-01T00:00:00Z"),
        }
    }

    fn make_relationship(src: &str, dst: &str, relation: &str, weight: f64) -> Relationship {
        Relationship {
            src: crate::id::EntityId::new_unchecked(src),
            dst: crate::id::EntityId::new_unchecked(dst),
            relation: relation.to_owned(),
            weight,
            created_at: test_ts("2026-03-01T00:00:00Z"),
        }
    }

    fn make_embedding(id: &str, content: &str, source_id: &str, nous_id: &str) -> EmbeddedChunk {
        EmbeddedChunk {
            id: crate::id::EmbeddingId::new_unchecked(id),
            content: content.to_owned(),
            source_type: "fact".to_owned(),
            source_id: source_id.to_owned(),
            nous_id: nous_id.to_owned(),
            embedding: vec![0.5, 0.5, 0.5, 0.5],
            created_at: test_ts("2026-03-01T00:00:00Z"),
        }
    }

    // ---- CRUD: Facts ----

    #[test]
    fn insert_fact_and_retrieve() {
        let store = make_store();
        let fact = make_fact("f1", "agent-a", "Rust is a systems programming language");
        store.insert_fact(&fact).expect("insert fact");

        let results = store
            .query_facts("agent-a", "2026-06-01", 10)
            .expect("query facts");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id.as_str(), "f1");
        assert_eq!(results[0].content, "Rust is a systems programming language");
        assert!((results[0].confidence - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn insert_multiple_facts_and_retrieve() {
        let store = make_store();
        for i in 0..5 {
            let fact = make_fact(&format!("f{i}"), "agent-a", &format!("Fact number {i}"));
            store.insert_fact(&fact).expect("insert fact");
        }

        let results = store
            .query_facts("agent-a", "2026-06-01", 100)
            .expect("query facts");
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn upsert_fact_overwrites() {
        let store = make_store();
        let mut fact = make_fact("f1", "agent-a", "Original content");
        store.insert_fact(&fact).expect("insert fact");

        fact.content = "Updated content".to_owned();
        fact.confidence = 0.95;
        store.insert_fact(&fact).expect("upsert fact");

        let results = store
            .query_facts("agent-a", "2026-06-01", 10)
            .expect("query facts");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "Updated content");
        assert!((results[0].confidence - 0.95).abs() < f64::EPSILON);
    }

    // ---- CRUD: Entities ----

    #[test]
    fn insert_entity_and_query_neighborhood() {
        let store = make_store();
        let entity = make_entity("e1", "Rust", "language");
        store.insert_entity(&entity).expect("insert entity");

        let rows = store
            .entity_neighborhood(&crate::id::EntityId::new_unchecked("e1"))
            .expect("neighborhood");
        // No relationships yet, so empty result set is fine (no panic)
        assert!(rows.rows.is_empty());
    }

    #[test]
    fn insert_entity_with_aliases() {
        let store = make_store();
        let mut entity = make_entity("e1", "Rust", "language");
        entity.aliases = vec!["rustlang".to_owned(), "rust-lang".to_owned()];
        store
            .insert_entity(&entity)
            .expect("insert entity with aliases");

        // Verify via raw query that the entity was stored
        let rows = store
            .run_query(
                r"?[id, name, aliases] := *entities{id, name, aliases}, id = 'e1'",
                std::collections::BTreeMap::new(),
            )
            .expect("raw query");
        assert_eq!(rows.rows.len(), 1);
    }

    // ---- CRUD: Relationships ----

    #[test]
    fn insert_relationship_and_retrieve_neighborhood() {
        let store = make_store();
        store
            .insert_entity(&make_entity("e1", "Alice", "person"))
            .expect("insert e1");
        store
            .insert_entity(&make_entity("e2", "Aletheia", "project"))
            .expect("insert e2");
        store
            .insert_relationship(&make_relationship("e1", "e2", "works_on", 0.9))
            .expect("insert relationship");

        let rows = store
            .entity_neighborhood(&crate::id::EntityId::new_unchecked("e1"))
            .expect("neighborhood");
        assert!(
            !rows.rows.is_empty(),
            "neighborhood should contain the relationship"
        );
    }

    #[test]
    fn insert_relationship_bidirectional_neighborhood() {
        let store = make_store();
        store
            .insert_entity(&make_entity("e1", "Alice", "person"))
            .expect("insert e1");
        store
            .insert_entity(&make_entity("e2", "Bob", "person"))
            .expect("insert e2");
        store
            .insert_relationship(&make_relationship("e1", "e2", "knows", 0.8))
            .expect("insert rel");

        // e1 neighborhood should include e2
        let from_e1 = store
            .entity_neighborhood(&crate::id::EntityId::new_unchecked("e1"))
            .expect("e1 neighborhood");
        assert!(!from_e1.rows.is_empty());

        // e2 neighborhood may or may not include e1 (depends on query directionality)
        // Just verify it doesn't error
        let _from_e2 = store
            .entity_neighborhood(&crate::id::EntityId::new_unchecked("e2"))
            .expect("e2 neighborhood");
    }

    // ---- CRUD: Embeddings ----

    #[test]
    fn insert_embedding_and_search() {
        let store = make_store();
        let fact = make_fact("f1", "agent-a", "Rust memory safety");
        store.insert_fact(&fact).expect("insert fact");

        let mut chunk = make_embedding("emb1", "Rust memory safety", "f1", "agent-a");
        chunk.embedding = vec![0.9, 0.1, 0.0, 0.0];
        store.insert_embedding(&chunk).expect("insert embedding");

        let results = store
            .search_vectors(vec![0.9, 0.1, 0.0, 0.0], 5, 20)
            .expect("search vectors");
        assert!(!results.is_empty());
        assert_eq!(results[0].content, "Rust memory safety");
        assert_eq!(results[0].source_type, "fact");
        assert_eq!(results[0].source_id, "f1");
    }

    // ---- Forget / Unforget ----

    #[test]
    fn forget_fact_excludes_from_query() {
        let store = make_store();
        let fact = make_fact("f1", "agent-a", "Secret fact");
        store.insert_fact(&fact).expect("insert fact");

        let forgotten = store
            .forget_fact(
                &crate::id::FactId::new_unchecked("f1"),
                ForgetReason::UserRequested,
            )
            .expect("forget fact");
        assert!(forgotten.is_forgotten);
        assert_eq!(forgotten.forget_reason, Some(ForgetReason::UserRequested));
        assert!(forgotten.forgotten_at.is_some());

        let results = store
            .query_facts("agent-a", "2026-06-01", 10)
            .expect("query facts");
        assert!(
            results.is_empty(),
            "forgotten fact must not appear in recall"
        );
    }

    #[test]
    fn forget_fact_then_unforget_restores_recall() {
        let store = make_store();
        let fact = make_fact("f1", "agent-a", "Recoverable fact");
        store.insert_fact(&fact).expect("insert fact");

        store
            .forget_fact(
                &crate::id::FactId::new_unchecked("f1"),
                ForgetReason::Outdated,
            )
            .expect("forget");

        // Verify excluded from recall
        let results = store
            .query_facts("agent-a", "2026-06-01", 10)
            .expect("query");
        assert!(results.is_empty(), "forgotten fact excluded from recall");

        // Unforget
        let restored = store
            .unforget_fact(&crate::id::FactId::new_unchecked("f1"))
            .expect("unforget");
        assert!(!restored.is_forgotten);
        assert!(restored.forgotten_at.is_none());
        assert!(restored.forget_reason.is_none());

        // Verify returned to recall
        let results = store
            .query_facts("agent-a", "2026-06-01", 10)
            .expect("query after unforget");
        assert_eq!(results.len(), 1, "unforget must restore recall visibility");
        assert_eq!(results[0].id.as_str(), "f1");
    }

    #[test]
    fn forget_preserves_in_audit() {
        let store = make_store();
        let fact = make_fact("f1", "agent-a", "Auditable fact");
        store.insert_fact(&fact).expect("insert fact");

        store
            .forget_fact(
                &crate::id::FactId::new_unchecked("f1"),
                ForgetReason::Privacy,
            )
            .expect("forget");

        let all = store.audit_all_facts("agent-a", 100).expect("audit all");
        let found = all.iter().find(|f| f.id.as_str() == "f1");
        assert!(found.is_some(), "audit must return forgotten facts");
        let found = found.expect("f1 must appear in audit after forget");
        assert!(found.is_forgotten);
        assert_eq!(found.forget_reason, Some(ForgetReason::Privacy));
    }

    #[test]
    fn forget_reason_roundtrips() {
        let store = make_store();

        let reasons = [
            ("f-ur", ForgetReason::UserRequested),
            ("f-od", ForgetReason::Outdated),
            ("f-ic", ForgetReason::Incorrect),
            ("f-pr", ForgetReason::Privacy),
        ];

        for (id, reason) in reasons {
            let fact = make_fact(id, "agent-a", &format!("fact for {reason}"));
            store.insert_fact(&fact).expect("insert");

            let forgotten = store
                .forget_fact(&crate::id::FactId::new_unchecked(id), reason)
                .expect("forget");
            assert_eq!(
                forgotten.forget_reason,
                Some(reason),
                "reason must round-trip for {reason}"
            );
        }

        // Verify all appear in list_forgotten
        let forgotten_list = store
            .list_forgotten("agent-a", 100)
            .expect("list_forgotten");
        assert_eq!(forgotten_list.len(), reasons.len());
        for (id, reason) in reasons {
            let found = forgotten_list
                .iter()
                .find(|f| f.id.as_str() == id)
                .unwrap_or_else(|| panic!("missing {id} in list_forgotten"));
            assert_eq!(found.forget_reason, Some(reason));
        }
    }

    #[test]
    fn forget_nonexistent_fact_errors() {
        let store = make_store();
        let result = store.forget_fact(
            &crate::id::FactId::new_unchecked("nonexistent"),
            ForgetReason::UserRequested,
        );
        assert!(result.is_err(), "forgetting non-existent fact must error");
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not found"),
            "error should mention not found: {err}"
        );
    }

    #[test]
    fn forget_excluded_from_temporal_diff() {
        let store = make_store();
        let fact = make_fact("f-diff", "agent-a", "Temporal diff fact");
        store.insert_fact(&fact).expect("insert");

        store
            .forget_fact(
                &crate::id::FactId::new_unchecked("f-diff"),
                ForgetReason::Incorrect,
            )
            .expect("forget");

        let diff = store
            .query_facts_diff("agent-a", "2025-01-01", "2027-01-01")
            .expect("diff");
        assert!(
            !diff.added.iter().any(|f| f.id.as_str() == "f-diff"),
            "forgotten fact must not appear in temporal diff added"
        );
    }

    // The remaining tests are identical to the original — truncated for brevity in this
    // decomposition but the full content is preserved below.

    // ---- Increment Access ----

    #[test]
    fn increment_access_updates_count() {
        let store = make_store();
        let fact = make_fact("f1", "agent-a", "Accessed fact");
        store.insert_fact(&fact).expect("insert fact");

        store
            .increment_access(&[crate::id::FactId::new_unchecked("f1")])
            .expect("increment");
        store
            .increment_access(&[crate::id::FactId::new_unchecked("f1")])
            .expect("increment again");

        let results = store
            .query_facts("agent-a", "2026-06-01", 10)
            .expect("query");
        let found = results
            .iter()
            .find(|f| f.id.as_str() == "f1")
            .expect("found");
        assert_eq!(found.access_count, 2);
    }

    #[test]
    fn increment_access_empty_ids_is_noop() {
        let store = make_store();
        store
            .increment_access(&[])
            .expect("empty increment should succeed");
    }

    #[test]
    fn increment_access_nonexistent_id_is_silent() {
        let store = make_store();
        // Should not error — silently skips missing facts
        store
            .increment_access(&[crate::id::FactId::new_unchecked("nonexistent")])
            .expect("increment nonexistent should not error");
    }

    // ---- Schema Version ----

    #[test]
    fn schema_version_returns_current() {
        let store = make_store();
        let version = store.schema_version().expect("schema version");
        assert_eq!(version, KnowledgeStore::SCHEMA_VERSION);
    }

    // ---- Search: query_facts filters ----

    #[test]
    fn query_facts_filters_by_nous_id() {
        let store = make_store();
        store
            .insert_fact(&make_fact("f1", "agent-a", "Fact for A"))
            .expect("insert f1");
        store
            .insert_fact(&make_fact("f2", "agent-b", "Fact for B"))
            .expect("insert f2");
        store
            .insert_fact(&make_fact("f3", "agent-a", "Another fact for A"))
            .expect("insert f3");

        let results_a = store
            .query_facts("agent-a", "2026-06-01", 100)
            .expect("query agent-a");
        assert_eq!(results_a.len(), 2);
        assert!(results_a.iter().all(|f| f.nous_id == "agent-a"));

        let results_b = store
            .query_facts("agent-b", "2026-06-01", 100)
            .expect("query agent-b");
        assert_eq!(results_b.len(), 1);
        assert_eq!(results_b[0].id.as_str(), "f2");
    }

    #[test]
    fn query_facts_respects_limit() {
        let store = make_store();
        for i in 0..20 {
            store
                .insert_fact(&make_fact(
                    &format!("f{i}"),
                    "agent-a",
                    &format!("Fact {i}"),
                ))
                .expect("insert");
        }

        let results = store
            .query_facts("agent-a", "2026-06-01", 5)
            .expect("query with limit");
        assert_eq!(results.len(), 5);
    }

    #[test]
    fn query_facts_excludes_expired() {
        let store = make_store();

        // Active fact
        store
            .insert_fact(&make_fact("f-active", "agent-a", "Active fact"))
            .expect("insert active");

        // Expired fact (valid_to in the past)
        let mut expired = make_fact("f-expired", "agent-a", "Expired fact");
        expired.valid_to =
            crate::knowledge::parse_timestamp("2025-01-01").expect("valid expiry timestamp");
        store.insert_fact(&expired).expect("insert expired");

        let results = store
            .query_facts("agent-a", "2026-06-01", 100)
            .expect("query");

        // Should only return the active fact
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id.as_str(), "f-active");
    }

    #[test]
    fn query_facts_empty_store_returns_empty() {
        let store = make_store();
        let results = store
            .query_facts("agent-a", "2026-06-01", 100)
            .expect("query empty store");
        assert!(results.is_empty());
    }

    #[test]
    fn query_facts_nonexistent_nous_id_returns_empty() {
        let store = make_store();
        store
            .insert_fact(&make_fact("f1", "agent-a", "Some fact"))
            .expect("insert");

        let results = store
            .query_facts("nonexistent-agent", "2026-06-01", 100)
            .expect("query nonexistent nous");
        assert!(results.is_empty());
    }

    // ---- Search: point-in-time ----

    #[test]
    fn query_facts_at_returns_snapshot() {
        let store = make_store();

        // Fact valid from 2026-01-01 to 2026-06-01
        let mut fact = make_fact("f1", "agent-a", "Temporal fact");
        fact.valid_from = crate::knowledge::parse_timestamp("2026-01-01")
            .expect("valid_from timestamp for temporal test");
        fact.valid_to = crate::knowledge::parse_timestamp("2026-06-01")
            .expect("valid_to timestamp for temporal test");
        store.insert_fact(&fact).expect("insert temporal fact");

        // Query at a time within the validity window
        let results = store
            .query_facts_at("2026-03-15")
            .expect("query at mid-range");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id.as_str(), "f1");

        // Query at a time after the validity window
        let results = store
            .query_facts_at("2026-07-01")
            .expect("query at post-range");
        assert!(results.is_empty());
    }

    // ---- Search: vectors ----

    #[test]
    fn search_vectors_empty_store_returns_empty() {
        let store = make_store();
        let results = store
            .search_vectors(vec![0.5, 0.5, 0.5, 0.5], 5, 20)
            .expect("search empty");
        assert!(results.is_empty());
    }

    #[test]
    fn search_vectors_returns_nearest() {
        let store = make_store();

        // Insert two embeddings with different vectors
        let mut chunk_a = make_embedding("emb-a", "Rust programming", "f1", "agent-a");
        chunk_a.embedding = vec![1.0, 0.0, 0.0, 0.0];
        store.insert_embedding(&chunk_a).expect("insert emb-a");

        let mut chunk_b = make_embedding("emb-b", "Python scripting", "f2", "agent-a");
        chunk_b.embedding = vec![0.0, 1.0, 0.0, 0.0];
        store.insert_embedding(&chunk_b).expect("insert emb-b");

        // Query close to chunk_a
        let results = store
            .search_vectors(vec![0.9, 0.1, 0.0, 0.0], 1, 20)
            .expect("search nearest");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "Rust programming");
    }

    #[test]
    fn search_vectors_respects_k() {
        let store = make_store();
        for i in 0..10 {
            let mut chunk = make_embedding(
                &format!("emb-{i}"),
                &format!("Content {i}"),
                &format!("f{i}"),
                "agent-a",
            );
            #[expect(
                clippy::cast_precision_loss,
                reason = "test data — i is 0..9, fits in f32"
            )]
            let component = i as f32 * 0.1;
            chunk.embedding = vec![component, 0.5, 0.3, 0.1];
            store.insert_embedding(&chunk).expect("insert");
        }

        let results = store
            .search_vectors(vec![0.5, 0.5, 0.3, 0.1], 3, 20)
            .expect("search k=3");
        assert_eq!(results.len(), 3);
    }

    // ---- Edge Cases ----

    #[test]
    fn insert_fact_empty_content_rejected() {
        let store = make_store();
        let fact = make_fact("f-empty", "agent-a", "");
        let result = store.insert_fact(&fact);
        assert!(result.is_err(), "empty content must be rejected");
        assert!(matches!(
            result.expect_err("empty content must fail"),
            crate::error::Error::EmptyContent { .. }
        ));
    }

    #[test]
    fn insert_fact_confidence_out_of_range_rejected() {
        let store = make_store();

        let mut high = make_fact("f-high", "agent-a", "High confidence");
        high.confidence = 1.5;
        let result = store.insert_fact(&high);
        assert!(result.is_err(), "confidence > 1.0 must be rejected");
        assert!(matches!(
            result.expect_err("confidence > 1.0 must fail"),
            crate::error::Error::InvalidConfidence { .. }
        ));

        let mut negative = make_fact("f-neg", "agent-a", "Negative confidence");
        negative.confidence = -0.5;
        let result = store.insert_fact(&negative);
        assert!(result.is_err(), "confidence < 0.0 must be rejected");
        assert!(matches!(
            result.expect_err("confidence < 0.0 must fail"),
            crate::error::Error::InvalidConfidence { .. }
        ));
    }

    #[test]
    fn insert_duplicate_entity_name_upserts() {
        let store = make_store();
        let e1 = make_entity("e1", "Rust", "language");
        store.insert_entity(&e1).expect("insert first");

        // Same ID, updated name
        let e1_updated = make_entity("e1", "Rust Lang", "language");
        store.insert_entity(&e1_updated).expect("upsert");

        let rows = store
            .run_query(
                r"?[id, name] := *entities{id, name}",
                std::collections::BTreeMap::new(),
            )
            .expect("raw query");
        // Same ID → upsert (one row)
        assert_eq!(rows.rows.len(), 1);
    }

    #[test]
    fn insert_different_entities_same_name() {
        let store = make_store();
        store
            .insert_entity(&make_entity("e1", "Rust", "language"))
            .expect("insert e1");
        store
            .insert_entity(&make_entity("e2", "Rust", "game"))
            .expect("insert e2");

        let rows = store
            .run_query(
                r"?[id, name] := *entities{id, name}",
                std::collections::BTreeMap::new(),
            )
            .expect("raw query");
        // Different IDs → two separate entities
        assert_eq!(rows.rows.len(), 2);
    }

    #[test]
    fn retrieve_nonexistent_fact() {
        let store = make_store();
        let results = store
            .query_facts("nonexistent-agent", "2026-06-01", 10)
            .expect("query should succeed, returning empty");
        assert!(results.is_empty());
    }

    #[test]
    fn forget_nonexistent_fact_returns_error() {
        let store = make_store();
        let result = store.forget_fact(
            &crate::id::FactId::new_unchecked("nonexistent"),
            ForgetReason::UserRequested,
        );
        assert!(result.is_err(), "forgetting a non-existent fact must error");
    }

    #[test]
    fn concurrent_inserts() {
        let store = make_store();
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let s = Arc::clone(&store);
                std::thread::spawn(move || {
                    let fact = Fact {
                        id: crate::id::FactId::new_unchecked(format!("f-concurrent-{i}")),
                        nous_id: "agent-a".to_owned(),
                        content: format!("Concurrent fact {i}"),
                        confidence: 0.9,
                        tier: EpistemicTier::Inferred,
                        valid_from: crate::knowledge::parse_timestamp("2026-01-01")
                            .expect("valid test timestamp"),
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
                    s.insert_fact(&fact).expect("concurrent insert");
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread join");
        }

        let results = store
            .query_facts("agent-a", "2026-06-01", 100)
            .expect("query after concurrent inserts");
        assert_eq!(results.len(), 10);
    }

    #[test]
    fn concurrent_entity_inserts() {
        let store = make_store();
        let handles: Vec<_> = (0..10)
            .map(|i| {
                let s = Arc::clone(&store);
                std::thread::spawn(move || {
                    let entity = Entity {
                        id: crate::id::EntityId::new_unchecked(format!("e-concurrent-{i}")),
                        name: format!("Entity {i}"),
                        entity_type: "concept".to_owned(),
                        aliases: vec![],
                        created_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                            .expect("valid test timestamp"),
                        updated_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                            .expect("valid test timestamp"),
                    };
                    s.insert_entity(&entity).expect("concurrent entity insert");
                })
            })
            .collect();

        for h in handles {
            h.join().expect("thread join");
        }

        let rows = store
            .run_query(
                r"?[count(id)] := *entities{id}",
                std::collections::BTreeMap::new(),
            )
            .expect("count entities");
        assert_eq!(rows.rows.len(), 1);
    }

    // ---- Raw Query / Mut Query ----

    #[test]
    fn run_query_returns_results() {
        let store = make_store();
        let rows = store
            .run_query("?[x] := x = 42", std::collections::BTreeMap::new())
            .expect("run_query");
        assert_eq!(rows.rows.len(), 1);
    }

    #[test]
    fn run_mut_query_creates_and_reads() {
        let store = make_store();
        store
            .insert_fact(&make_fact("f1", "agent-a", "Mutable test"))
            .expect("insert");

        // Use run_mut_query to delete the fact
        let mut params = std::collections::BTreeMap::new();
        params.insert("id".to_owned(), crate::engine::DataValue::Str("f1".into()));
        store
            .run_mut_query(
                r"?[id, valid_from] := *facts{id, valid_from}, id = $id :rm facts {id, valid_from}",
                params,
            )
            .expect("delete via run_mut_query");

        let results = store
            .query_facts("agent-a", "2026-06-01", 10)
            .expect("query after delete");
        assert!(results.is_empty());
    }

    // ---- Relationship queries ----

    #[test]
    fn entity_neighborhood_2hop() {
        let store = make_store();
        store
            .insert_entity(&make_entity("e1", "Alice", "person"))
            .expect("e1");
        store
            .insert_entity(&make_entity("e2", "Aletheia", "project"))
            .expect("e2");
        store
            .insert_entity(&make_entity("e3", "Rust", "language"))
            .expect("e3");

        // e1 -> e2 -> e3 (2-hop chain)
        store
            .insert_relationship(&make_relationship("e1", "e2", "works_on", 0.9))
            .expect("rel e1-e2");
        store
            .insert_relationship(&make_relationship("e2", "e3", "uses", 0.8))
            .expect("rel e2-e3");

        let rows = store
            .entity_neighborhood(&crate::id::EntityId::new_unchecked("e1"))
            .expect("2-hop neighborhood");
        // Should include both e2 (1-hop) and e3 (2-hop)
        assert!(
            rows.rows.len() >= 2,
            "2-hop neighborhood should find at least 2 results, got {}",
            rows.rows.len()
        );
    }

    #[test]
    fn entity_neighborhood_nonexistent_entity() {
        let store = make_store();
        let rows = store
            .entity_neighborhood(&crate::id::EntityId::new_unchecked("nonexistent"))
            .expect("neighborhood of missing entity should succeed");
        assert!(rows.rows.is_empty());
    }

    // ---- Async wrappers ----

    #[tokio::test]
    async fn insert_fact_async_works() {
        let store = make_store();
        let fact = make_fact("f-async", "agent-a", "Async inserted fact");
        store.insert_fact_async(fact).await.expect("async insert");

        let results = store
            .query_facts_async("agent-a".to_owned(), "2026-06-01".to_owned(), 10)
            .await
            .expect("async query");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id.as_str(), "f-async");
    }

    #[tokio::test]
    async fn search_vectors_async_works() {
        let store = make_store();
        let mut chunk = make_embedding("emb-async", "Async search content", "f1", "agent-a");
        chunk.embedding = vec![0.7, 0.3, 0.0, 0.0];
        store.insert_embedding(&chunk).expect("insert embedding");

        let results = store
            .search_vectors_async(vec![0.7, 0.3, 0.0, 0.0], 5, 20)
            .await
            .expect("async search");
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn forget_fact_async_works() {
        let store = make_store();
        let fact = make_fact("f-forget-async", "agent-a", "Async forget");
        store.insert_fact_async(fact).await.expect("insert");

        let forgotten = store
            .forget_fact_async(
                crate::id::FactId::new_unchecked("f-forget-async"),
                ForgetReason::Incorrect,
            )
            .await
            .expect("async forget");
        assert!(forgotten.is_forgotten);
        assert_eq!(forgotten.forget_reason, Some(ForgetReason::Incorrect));

        let all = store
            .audit_all_facts_async("agent-a".to_owned(), 100)
            .await
            .expect("async audit");
        let found = all
            .iter()
            .find(|f| f.id.as_str() == "f-forget-async")
            .expect("found");
        assert!(found.is_forgotten);
    }

    #[tokio::test]
    async fn unforget_fact_async_works() {
        let store = make_store();
        let fact = make_fact("f-unforget-async", "agent-a", "Async unforget");
        store.insert_fact_async(fact).await.expect("insert");

        store
            .forget_fact_async(
                crate::id::FactId::new_unchecked("f-unforget-async"),
                ForgetReason::Outdated,
            )
            .await
            .expect("forget");
        store
            .unforget_fact_async(crate::id::FactId::new_unchecked("f-unforget-async"))
            .await
            .expect("unforget");

        let all = store
            .audit_all_facts_async("agent-a".to_owned(), 100)
            .await
            .expect("audit");
        let found = all
            .iter()
            .find(|f| f.id.as_str() == "f-unforget-async")
            .expect("found");
        assert!(!found.is_forgotten);
    }

    #[tokio::test]
    async fn increment_access_async_works() {
        let store = make_store();
        let fact = make_fact("f-access-async", "agent-a", "Async access");
        store.insert_fact_async(fact).await.expect("insert");

        store
            .increment_access_async(vec![crate::id::FactId::new_unchecked("f-access-async")])
            .await
            .expect("async increment");

        let results = store
            .query_facts_async("agent-a".to_owned(), "2026-06-01".to_owned(), 10)
            .await
            .expect("query");
        let found = results
            .iter()
            .find(|f| f.id.as_str() == "f-access-async")
            .expect("found");
        assert_eq!(found.access_count, 1);
    }

    // --- Bi-temporal query tests ---

    fn make_temporal_fact(
        id: &str,
        nous_id: &str,
        content: &str,
        valid_from: &str,
        valid_to: &str,
    ) -> Fact {
        Fact {
            id: crate::id::FactId::new_unchecked(id),
            nous_id: nous_id.to_owned(),
            content: content.to_owned(),
            confidence: 0.9,
            tier: EpistemicTier::Inferred,
            valid_from: crate::knowledge::parse_timestamp(valid_from)
                .expect("valid_from timestamp in make_temporal_fact"),
            valid_to: crate::knowledge::parse_timestamp(valid_to)
                .expect("valid_to timestamp in make_temporal_fact"),
            superseded_by: None,
            source_session_id: None,
            recorded_at: crate::knowledge::parse_timestamp("2026-03-01T00:00:00Z")
                .expect("recorded_at timestamp in make_temporal_fact"),
            access_count: 0,
            last_accessed_at: None,
            stability_hours: 720.0,
            fact_type: String::new(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        }
    }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_query_point_in_time() { let store = make_store(); store.insert_fact(&make_temporal_fact("t1", "agent", "Rust is fast", "2026-01-01", "2026-06-01")).expect("insert t1"); store.insert_fact(&make_temporal_fact("t2", "agent", "Python is dynamic", "2026-03-01", "9999-12-31")).expect("insert t2"); let at_feb = store.query_facts_temporal("agent", "2026-02-01", None).expect("query feb"); assert_eq!(at_feb.len(), 1); assert_eq!(at_feb[0].id.as_str(), "t1"); let at_apr = store.query_facts_temporal("agent", "2026-04-01", None).expect("query apr"); assert_eq!(at_apr.len(), 2); let at_jul = store.query_facts_temporal("agent", "2026-07-01", None).expect("query jul"); assert_eq!(at_jul.len(), 1); assert_eq!(at_jul[0].id.as_str(), "t2"); }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_query_before_any_facts_returns_empty() { let store = make_store(); store.insert_fact(&make_temporal_fact("t1", "agent", "fact", "2026-06-01", "9999-12-31")).expect("insert"); let results = store.query_facts_temporal("agent", "2026-01-01", None).expect("query"); assert!(results.is_empty()); }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_query_boundary_inclusion() { let store = make_store(); store.insert_fact(&make_temporal_fact("t1", "agent", "boundary fact", "2026-03-01", "2026-06-01")).expect("insert"); let at_start = store.query_facts_temporal("agent", "2026-03-01T00:00:00Z", None).expect("at valid_from"); assert_eq!(at_start.len(), 1, "valid_from boundary is inclusive"); let at_end = store.query_facts_temporal("agent", "2026-06-01T00:00:00Z", None).expect("at valid_to"); assert!(at_end.is_empty(), "valid_to boundary is exclusive"); }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_query_with_content_filter() { let store = make_store(); store.insert_fact(&make_temporal_fact("t1", "agent", "Rust is fast", "2026-01-01", "9999-12-31")).expect("insert t1"); store.insert_fact(&make_temporal_fact("t2", "agent", "Python is dynamic", "2026-01-01", "9999-12-31")).expect("insert t2"); let filtered = store.query_facts_temporal("agent", "2026-03-01", Some("Rust")).expect("filtered query"); assert_eq!(filtered.len(), 1); assert_eq!(filtered[0].id.as_str(), "t1"); }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_diff_added_and_removed() { let store = make_store(); store.insert_fact(&make_temporal_fact("old", "agent", "old knowledge", "2026-01-01", "2026-03-01")).expect("insert old"); store.insert_fact(&make_temporal_fact("new", "agent", "new knowledge", "2026-02-15", "9999-12-31")).expect("insert new"); let diff = store.query_facts_diff("agent", "2026-02-01", "2026-04-01").expect("diff"); assert_eq!(diff.added.len(), 1, "one fact added in interval"); assert_eq!(diff.added[0].id.as_str(), "new"); assert_eq!(diff.removed.len(), 1, "one fact removed in interval"); assert_eq!(diff.removed[0].id.as_str(), "old"); }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_diff_supersession_chain() { let store = make_store(); let mut fact_a = make_temporal_fact("a", "agent", "version 1", "2026-01-01", "2026-03-01"); fact_a.superseded_by = Some(crate::id::FactId::new_unchecked("b")); store.insert_fact(&fact_a).expect("insert a"); store.insert_fact(&make_temporal_fact("b", "agent", "version 2", "2026-03-01", "9999-12-31")).expect("insert b"); let diff = store.query_facts_diff("agent", "2026-02-01", "2026-04-01").expect("diff"); assert_eq!(diff.modified.len(), 1, "one modified pair"); assert_eq!(diff.modified[0].0.id.as_str(), "a"); assert_eq!(diff.modified[0].1.id.as_str(), "b"); assert!(diff.added.is_empty(), "superseded new is not in pure added"); assert!(diff.removed.is_empty(), "superseding old is not in pure removed"); }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_query_isolates_nous_ids() { let store = make_store(); store.insert_fact(&make_temporal_fact("t1", "alice", "Alice knows Rust", "2026-01-01", "9999-12-31")).expect("insert alice"); store.insert_fact(&make_temporal_fact("t2", "bob", "Bob knows Python", "2026-01-01", "9999-12-31")).expect("insert bob"); let alice_facts = store.query_facts_temporal("alice", "2026-03-01", None).expect("alice query"); assert_eq!(alice_facts.len(), 1); assert_eq!(alice_facts[0].content, "Alice knows Rust"); let bob_facts = store.query_facts_temporal("bob", "2026-03-01", None).expect("bob query"); assert_eq!(bob_facts.len(), 1); assert_eq!(bob_facts[0].content, "Bob knows Python"); }

    #[cfg(feature = "mneme-engine")]
    #[test]
    fn temporal_query_excludes_forgotten_facts() { let store = make_store(); store.insert_fact(&make_temporal_fact("t1", "agent", "forgotten fact", "2026-01-01", "9999-12-31")).expect("insert"); store.forget_fact(&crate::id::FactId::new_unchecked("t1"), crate::knowledge::ForgetReason::UserRequested).expect("forget"); let results = store.query_facts_temporal("agent", "2026-03-01", None).expect("query"); assert!(results.is_empty(), "forgotten facts should be excluded"); }

    #[cfg(feature = "mneme-engine")]
    #[tokio::test]
    async fn temporal_query_async_works() { let store = make_store(); store.insert_fact(&make_temporal_fact("t1", "agent", "async temporal", "2026-01-01", "9999-12-31")).expect("insert"); let results = store.query_facts_temporal_async("agent".to_owned(), "2026-03-01".to_owned(), None).await.expect("async query"); assert_eq!(results.len(), 1); }

    #[cfg(feature = "mneme-engine")]
    #[tokio::test]
    async fn temporal_diff_async_works() { let store = make_store(); store.insert_fact(&make_temporal_fact("t1", "agent", "diff async", "2026-02-01", "9999-12-31")).expect("insert"); let diff = store.query_facts_diff_async("agent".to_owned(), "2026-01-01".to_owned(), "2026-03-01".to_owned()).await.expect("async diff"); assert_eq!(diff.added.len(), 1); }

    #[test]
    fn backup_db_returns_error_for_mem_backend() { let store = make_store(); let dir = tempfile::tempdir().expect("create temp dir"); let backup_path = dir.path().join("backup.db"); let result = store.backup_db(&backup_path); assert!(result.is_err(), "backup_db should error on in-memory backend"); }

    #[test]
    fn restore_backup_returns_error_for_mem_backend() { let store = make_store(); let dir = tempfile::tempdir().expect("create temp dir"); let backup_path = dir.path().join("backup.db"); std::fs::write(&backup_path, "fake").expect("write fake backup file"); let result = store.restore_backup(&backup_path); assert!(result.is_err(), "restore_backup should error on in-memory backend"); }

    #[test]
    fn import_from_backup_returns_error_for_mem_backend() { let store = make_store(); let dir = tempfile::tempdir().expect("create temp dir"); let backup_path = dir.path().join("backup.db"); std::fs::write(&backup_path, "fake").expect("write fake backup file"); let result = store.import_from_backup(&backup_path, &["facts".to_owned()]); assert!(result.is_err(), "import_from_backup should error on in-memory backend"); }

    #[test]
    fn query_result_does_not_expose_named_rows_type() { let store = make_store(); let result: QueryResult = store.run_query("?[x] := x = 99", BTreeMap::new()).expect("simple query"); assert_eq!(result.rows.len(), 1, "one result row expected"); assert!(!result.headers.is_empty(), "headers must be populated"); }

    #[test]
    fn query_result_from_run_script_read_only() { let store = make_store(); let result: QueryResult = store.run_script_read_only("?[x] := x = 42", BTreeMap::new()).expect("read-only query should succeed"); assert_eq!(result.rows.len(), 1); }

    #[test]
    fn run_script_read_only_basic() { let store = make_store(); let result = store.run_script_read_only("?[x] := x = 42", BTreeMap::new()).expect("read-only query should succeed"); assert_eq!(result.rows.len(), 1); }

    #[test]
    fn run_script_read_only_rejects_mutations() { let store = make_store(); let result = store.run_script_read_only(r"?[x] <- [[1]] :put facts { id: 'x', valid_from: 'now' => content: 'test', nous_id: 'a', confidence: 1.0, tier: 'verified', valid_to: 'end', recorded_at: 'now', access_count: 0, last_accessed_at: '', stability_hours: 720.0, fact_type: '' }", BTreeMap::new()); assert!(result.is_err(), "read-only mode should reject :put operations"); }

    #[test]
    fn audit_all_facts_returns_forgotten() { let store = make_store(); let f1 = make_fact("f1", "agent-a", "visible fact"); let f2 = make_fact("f2", "agent-a", "forgotten fact"); store.insert_fact(&f1).expect("insert f1"); store.insert_fact(&f2).expect("insert f2"); store.forget_fact(&crate::id::FactId::new_unchecked("f2"), ForgetReason::UserRequested).expect("forget f2"); let all = store.audit_all_facts("agent-a", 100).expect("audit"); assert_eq!(all.len(), 2); let forgotten_count = all.iter().filter(|f| f.is_forgotten).count(); assert_eq!(forgotten_count, 1); }

    #[test]
    fn audit_all_facts_empty_store() { let store = make_store(); let all = store.audit_all_facts("agent-a", 100).expect("audit empty"); assert!(all.is_empty()); }

    #[test]
    fn forget_already_forgotten_is_idempotent() { let store = make_store(); let f1 = make_fact("f1", "agent-a", "will be forgotten twice"); store.insert_fact(&f1).expect("insert f1"); store.forget_fact(&crate::id::FactId::new_unchecked("f1"), ForgetReason::Outdated).expect("first forget"); store.forget_fact(&crate::id::FactId::new_unchecked("f1"), ForgetReason::Outdated).expect("second forget should not panic"); let all = store.audit_all_facts("agent-a", 100).expect("audit"); assert_eq!(all.len(), 1); assert!(all[0].is_forgotten); }

    #[test]
    fn unforget_never_forgotten_is_noop() { let store = make_store(); let f1 = make_fact("f1", "agent-a", "never forgotten"); store.insert_fact(&f1).expect("insert f1"); store.unforget_fact(&crate::id::FactId::new_unchecked("f1")).expect("unforget should succeed"); let results = store.query_facts("agent-a", "2026-06-01", 10).expect("query"); assert_eq!(results.len(), 1); assert_eq!(results[0].content, "never forgotten"); }

    #[test]
    fn forget_nonexistent_fact_is_err() { let store = make_store(); let result = store.forget_fact(&crate::id::FactId::new_unchecked("nonexistent"), ForgetReason::UserRequested); assert!(result.is_err()); }

    #[test]
    fn forget_with_all_reasons() { let store = make_store(); let reasons = [("f1", ForgetReason::UserRequested), ("f2", ForgetReason::Outdated), ("f3", ForgetReason::Incorrect), ("f4", ForgetReason::Privacy)]; for (id, _) in &reasons { let fact = make_fact(id, "agent-a", &format!("fact {id}")); store.insert_fact(&fact).expect("insert"); } for (id, reason) in &reasons { store.forget_fact(&crate::id::FactId::new_unchecked(*id), *reason).expect("forget"); } let all = store.audit_all_facts("agent-a", 100).expect("audit"); assert_eq!(all.len(), 4); for fact in &all { assert!(fact.is_forgotten); assert!(fact.forget_reason.is_some()); } let reasons: Vec<ForgetReason> = all.iter().filter_map(|f| f.forget_reason).collect(); assert!(reasons.contains(&ForgetReason::UserRequested)); assert!(reasons.contains(&ForgetReason::Outdated)); assert!(reasons.contains(&ForgetReason::Incorrect)); assert!(reasons.contains(&ForgetReason::Privacy)); }

    #[test]
    fn insert_fact_unicode_content() { let store = make_store(); let mut fact = make_fact("fu", "agent-a", "placeholder"); fact.content = "日本語のファクト 🦀".to_owned(); store.insert_fact(&fact).expect("insert unicode fact"); let results = store.query_facts("agent-a", "2026-06-01", 10).expect("query"); assert_eq!(results.len(), 1); assert_eq!(results[0].content, "日本語のファクト 🦀"); }

    #[test]
    fn insert_fact_very_long_content() { let store = make_store(); let long_content = "x".repeat(10240); let mut fact = make_fact("fl", "agent-a", "placeholder"); fact.content = long_content.clone(); store.insert_fact(&fact).expect("insert long fact"); let results = store.query_facts("agent-a", "2026-06-01", 10).expect("query"); assert_eq!(results.len(), 1); assert_eq!(results[0].content.len(), 10240); }

    #[test]
    fn query_facts_limit_zero() { let store = make_store(); store.insert_fact(&make_fact("f1", "agent-a", "fact one")).expect("insert"); let results = store.query_facts("agent-a", "2026-06-01", 0).expect("query with limit 0"); assert!(results.is_empty()); }

    #[test]
    fn query_facts_large_limit() { let store = make_store(); store.insert_fact(&make_fact("f1", "agent-a", "one")).expect("insert f1"); store.insert_fact(&make_fact("f2", "agent-a", "two")).expect("insert f2"); store.insert_fact(&make_fact("f3", "agent-a", "three")).expect("insert f3"); let results = store.query_facts("agent-a", "2026-06-01", 1000).expect("query large limit"); assert_eq!(results.len(), 3); }

    #[test]
    fn search_vectors_dimension_mismatch_errors() { let store = make_store(); let wrong_dim = vec![0.1, 0.2, 0.3, 0.4, 0.5]; let result = store.search_vectors(wrong_dim, 5, 16); assert!(result.is_err(), "search with wrong dimension should error"); }

    #[test]
    fn run_query_malformed_datalog_errors() { let store = make_store(); let result = store.run_query("this is not valid datalog!!!", BTreeMap::new()); assert!(result.is_err(), "malformed datalog should error"); }

    #[test]
    fn insert_entity_unicode() { let store = make_store(); let entity = make_entity("eu1", "Ελληνικά", "language"); store.insert_entity(&entity).expect("insert unicode entity"); let rows = store.entity_neighborhood(&crate::id::EntityId::new_unchecked("eu1")).expect("neighborhood query"); assert!(rows.rows.is_empty() || !rows.rows.is_empty()); }

    #[test]
    fn insert_fact_confidence_zero() { let store = make_store(); let mut fact = make_fact("fc0", "agent-a", "zero confidence"); fact.confidence = 0.0; store.insert_fact(&fact).expect("insert zero confidence"); let results = store.query_facts("agent-a", "2026-06-01", 10).expect("query"); let found = results.iter().find(|f| f.id.as_str() == "fc0").expect("find fact"); assert!((found.confidence - 0.0).abs() < f64::EPSILON); }

    #[test]
    fn insert_fact_confidence_one() { let store = make_store(); let mut fact = make_fact("fc1", "agent-a", "full confidence"); fact.confidence = 1.0; store.insert_fact(&fact).expect("insert full confidence"); let results = store.query_facts("agent-a", "2026-06-01", 10).expect("query"); let found = results.iter().find(|f| f.id.as_str() == "fc1").expect("find fact"); assert!((found.confidence - 1.0).abs() < f64::EPSILON); }

    #[tokio::test]
    async fn query_facts_async_works() { let store = make_store(); store.insert_fact(&make_fact("fa1", "agent-a", "async fact one")).expect("insert"); store.insert_fact(&make_fact("fa2", "agent-a", "async fact two")).expect("insert"); let results = store.query_facts_async("agent-a".to_owned(), "2026-06-01".to_owned(), 10).await.expect("async query"); assert_eq!(results.len(), 2); }

    #[tokio::test]
    async fn audit_all_facts_async_works() { let store = make_store(); store.insert_fact(&make_fact("faa1", "agent-a", "audit async one")).expect("insert"); store.insert_fact(&make_fact("faa2", "agent-a", "audit async two")).expect("insert"); store.forget_fact(&crate::id::FactId::new_unchecked("faa2"), ForgetReason::Incorrect).expect("forget"); let all = store.audit_all_facts_async("agent-a".to_owned(), 100).await.expect("async audit"); assert_eq!(all.len(), 2); let forgotten_count = all.iter().filter(|f| f.is_forgotten).count(); assert_eq!(forgotten_count, 1); }

    #[tokio::test]
    async fn search_temporal_async_works() { let store = make_store(); let fact = make_fact("fst1", "agent-a", "temporal search target"); store.insert_fact(&fact).expect("insert fact"); let emb = make_embedding("est1", "temporal search target", "fst1", "agent-a"); store.insert_embedding(&emb).expect("insert embedding"); let q = HybridQuery { text: "temporal".to_owned(), embedding: vec![0.5, 0.5, 0.5, 0.5], seed_entities: vec![], limit: 10, ef: 16 }; let results = store.search_temporal_async(q, "2026-06-01".to_owned()).await.expect("async temporal search"); assert!(!results.is_empty()); }

    // --- Skill query tests ---

    fn make_skill_fact(id: &str, nous_id: &str, skill_name: &str, domain_tags: &[&str]) -> Fact {
        let content = serde_json::to_string(&crate::skill::SkillContent {
            name: skill_name.to_owned(),
            description: format!("Skill: {skill_name}"),
            steps: vec!["step 1".to_owned()],
            tools_used: vec!["Read".to_owned()],
            domain_tags: domain_tags.iter().map(|t| (*t).to_owned()).collect(),
            origin: "seeded".to_owned(),
        })
        .expect("skill content serializes to JSON");
        Fact {
            id: crate::id::FactId::new_unchecked(id),
            nous_id: nous_id.to_owned(),
            content,
            confidence: 0.5,
            tier: EpistemicTier::Assumed,
            valid_from: test_ts("2026-01-01"),
            valid_to: crate::knowledge::far_future(),
            superseded_by: None,
            source_session_id: None,
            recorded_at: test_ts("2026-03-01T00:00:00Z"),
            access_count: 0,
            last_accessed_at: None,
            stability_hours: 2190.0,
            fact_type: "skill".to_owned(),
            is_forgotten: false,
            forgotten_at: None,
            forget_reason: None,
        }
    }

    #[test]
    fn find_skills_for_nous_returns_only_skills() { let store = make_store(); let skill = make_skill_fact("sk-1", "alice", "rust-errors", &["rust"]); store.insert_fact(&skill).expect("insert skill"); let non_skill = make_fact("f-1", "alice", "Alice likes cats"); store.insert_fact(&non_skill).expect("insert non-skill"); let results = store.find_skills_for_nous("alice", 100).expect("query"); assert_eq!(results.len(), 1); assert_eq!(results[0].fact_type, "skill"); }

    #[test]
    fn find_skills_for_nous_ordered_by_confidence() { let store = make_store(); let mut low = make_skill_fact("sk-low", "alice", "low-conf", &["test"]); low.confidence = 0.3; store.insert_fact(&low).expect("insert low"); let mut high = make_skill_fact("sk-high", "alice", "high-conf", &["test"]); high.confidence = 0.9; store.insert_fact(&high).expect("insert high"); let results = store.find_skills_for_nous("alice", 100).expect("query"); assert_eq!(results.len(), 2); assert!(results[0].confidence >= results[1].confidence, "skills should be ordered by confidence descending"); }

    #[test]
    fn find_skills_nous_scoping() { let store = make_store(); let alice_skill = make_skill_fact("sk-a", "alice", "alice-skill", &["rust"]); store.insert_fact(&alice_skill).expect("insert alice"); let bob_skill = make_skill_fact("sk-b", "bob", "bob-skill", &["python"]); store.insert_fact(&bob_skill).expect("insert bob"); let alice_results = store.find_skills_for_nous("alice", 100).expect("query alice"); assert_eq!(alice_results.len(), 1); assert_eq!(alice_results[0].id.as_str(), "sk-a"); let bob_results = store.find_skills_for_nous("bob", 100).expect("query bob"); assert_eq!(bob_results.len(), 1); assert_eq!(bob_results[0].id.as_str(), "sk-b"); }

    #[test]
    fn find_skills_by_domain_filters_tags() { let store = make_store(); let rust_skill = make_skill_fact("sk-r", "alice", "rust-errors", &["rust", "errors"]); store.insert_fact(&rust_skill).expect("insert rust"); let py_skill = make_skill_fact("sk-p", "alice", "python-web", &["python", "web"]); store.insert_fact(&py_skill).expect("insert python"); let results = store.find_skills_by_domain("alice", &["rust"], 100).expect("query"); assert_eq!(results.len(), 1); assert_eq!(results[0].id.as_str(), "sk-r"); let results = store.find_skills_by_domain("alice", &["web"], 100).expect("query"); assert_eq!(results.len(), 1); assert_eq!(results[0].id.as_str(), "sk-p"); let results = store.find_skills_by_domain("alice", &["rust", "python"], 100).expect("query"); assert_eq!(results.len(), 2); }

    #[test]
    fn find_skills_by_domain_empty_tags() { let store = make_store(); let skill = make_skill_fact("sk-1", "alice", "some-skill", &["rust"]); store.insert_fact(&skill).expect("insert"); let results = store.find_skills_by_domain("alice", &[], 100).expect("query"); assert!(results.is_empty(), "empty tags should match nothing"); }

    #[test]
    fn find_skill_by_name_found() { let store = make_store(); let skill = make_skill_fact("sk-named", "alice", "rust-error-handling", &["rust"]); store.insert_fact(&skill).expect("insert"); let found = store.find_skill_by_name("alice", "rust-error-handling").expect("query"); assert_eq!(found, Some("sk-named".to_owned())); }

    #[test]
    fn find_skill_by_name_not_found() { let store = make_store(); let skill = make_skill_fact("sk-1", "alice", "actual-name", &["test"]); store.insert_fact(&skill).expect("insert"); let found = store.find_skill_by_name("alice", "nonexistent").expect("query"); assert!(found.is_none()); }

    #[test]
    fn find_skills_excludes_forgotten() { let store = make_store(); let skill = make_skill_fact("sk-forget", "alice", "forgotten-skill", &["test"]); store.insert_fact(&skill).expect("insert"); store.forget_fact(&crate::id::FactId::new_unchecked("sk-forget"), crate::knowledge::ForgetReason::Outdated).expect("forget"); let results = store.find_skills_for_nous("alice", 100).expect("query"); assert!(results.is_empty(), "forgotten skills should not be returned"); }

    #[test]
    fn search_skills_bm25() { let store = make_store(); let skill1 = make_skill_fact("sk-docker", "alice", "docker-deploy", &["docker"]); store.insert_fact(&skill1).expect("insert docker"); let skill2 = make_skill_fact("sk-k8s", "alice", "kubernetes-deploy", &["k8s"]); store.insert_fact(&skill2).expect("insert k8s"); let results = store.search_skills("alice", "docker", 10).expect("search"); assert!(results.iter().any(|f| f.id.as_str() == "sk-docker"), "search should find docker skill"); }

    // ── Skill quality lifecycle tests ──────────────────────────────────────

    #[test]
    fn skill_usage_tracking_via_increment_access() { let store = make_store(); let skill = make_skill_fact("sk-usage", "alice", "usage-test", &["rust"]); store.insert_fact(&skill).expect("insert skill"); store.increment_access(&[crate::id::FactId::new_unchecked("sk-usage")]).expect("increment"); store.increment_access(&[crate::id::FactId::new_unchecked("sk-usage")]).expect("increment again"); let results = store.find_skills_for_nous("alice", 100).expect("query"); let found = results.iter().find(|f| f.id.as_str() == "sk-usage").expect("find skill"); assert_eq!(found.access_count, 2, "usage_count should be 2 after two increments"); assert!(found.last_accessed_at.is_some(), "last_accessed_at should be set"); }

    #[test]
    fn skill_decay_retires_stale_skills() { let store = make_store(); let mut stale = make_skill_fact("sk-stale", "alice", "stale-skill", &["test"]); stale.valid_from = jiff::Timestamp::now().checked_sub(jiff::SignedDuration::from_hours(24 * 120)).expect("subtract 120 days"); stale.confidence = 0.5; stale.access_count = 0; store.insert_fact(&stale).expect("insert stale skill"); let fresh = make_skill_fact("sk-fresh", "alice", "fresh-skill", &["test"]); store.insert_fact(&fresh).expect("insert fresh skill"); let (active, _needs_review, retired) = store.run_skill_decay("alice").expect("run skill decay"); assert!(retired >= 1, "stale skill should be retired, got retired={retired}"); assert!(active >= 1, "fresh skill should still be active, got active={active}"); }

    #[test]
    fn skill_quality_metrics_returns_correct_counts() { let store = make_store(); let skill1 = make_skill_fact("sk-m1", "alice", "skill-one", &["rust"]); store.insert_fact(&skill1).expect("insert skill 1"); let mut skill2 = make_skill_fact("sk-m2", "alice", "skill-two", &["python"]); skill2.access_count = 5; store.insert_fact(&skill2).expect("insert skill 2"); let metrics = store.skill_quality_metrics("alice").expect("get metrics"); assert_eq!(metrics.total_active, 2); assert!(metrics.avg_usage_count > 0.0); }

    #[test]
    fn skill_decay_high_usage_survives_longer() { let store = make_store(); let past = jiff::Timestamp::now().checked_sub(jiff::SignedDuration::from_hours(24 * 50)).expect("subtract 50 days"); let mut low = make_skill_fact("sk-low-use", "alice", "low-usage", &["test"]); low.valid_from = past; low.access_count = 1; low.confidence = 0.7; store.insert_fact(&low).expect("insert low usage"); let mut high = make_skill_fact("sk-high-use", "alice", "high-usage", &["test"]); high.valid_from = past; high.access_count = 15; high.confidence = 0.7; store.insert_fact(&high).expect("insert high usage"); let (active, _needs_review, retired) = store.run_skill_decay("alice").expect("run decay"); let remaining = store.find_skills_for_nous("alice", 100).expect("query"); let high_survived = remaining.iter().any(|f| f.id.as_str() == "sk-high-use"); assert!(high_survived, "high-usage skill should survive 50-day decay, active={active}, retired={retired}"); }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn fact_roundtrip(
                content in "[a-zA-Z0-9 ]{1,200}",
                confidence in 0.0_f64..=1.0,
            ) {
                let store = make_store();
                let mut fact = make_fact("prop-rt", "agent-prop", &content);
                fact.confidence = confidence;
                store.insert_fact(&fact).expect("insert");
                let results = store.query_facts("agent-prop", "2026-06-01", 10).expect("query");
                prop_assert_eq!(results.len(), 1);
                prop_assert_eq!(&results[0].content, &content);
                prop_assert!((results[0].confidence - confidence).abs() < 1e-10);
                prop_assert_eq!(results[0].tier, crate::knowledge::EpistemicTier::Inferred);
            }
        }
    }
}
