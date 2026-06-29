#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]

use crate::knowledge::{EmbeddedChunk, ForgetReason, Visibility};
use crate::test_fixtures::{make_entity, make_fact, make_store, test_ts};

fn extraction_entity(name: &str, entity_type: &str) -> eidos::bookkeeping::ExtractedEntity {
    eidos::bookkeeping::ExtractedEntity {
        name: name.to_owned(),
        entity_type: entity_type.to_owned(),
        description: String::new(),
    }
}

fn make_embedding(id: &str, content: &str, source_id: &str, nous_id: &str) -> EmbeddedChunk {
    EmbeddedChunk {
        id: crate::id::EmbeddingId::new(id).expect("valid test id"),
        content: content.to_owned(),
        source_type: "fact".to_owned(),
        source_id: source_id.to_owned(),
        nous_id: nous_id.to_owned(),
        embedding: vec![0.5, 0.5, 0.5, 0.5],
        created_at: test_ts("2026-03-01T00:00:00Z"),
    }
}

fn make_visible_fact(
    id: &str,
    nous_id: &str,
    content: &str,
    visibility: Visibility,
) -> crate::knowledge::Fact {
    let mut fact = make_fact(id, nous_id, content);
    fact.visibility = visibility;
    fact
}

fn result_ids(results: &[crate::knowledge::RecallResult]) -> Vec<&str> {
    results
        .iter()
        .map(|result| result.source_id.as_str())
        .collect()
}

/// #3380 integration: when the real embedding provider fails to initialize,
/// the system must fall back to [`DegradedEmbeddingProvider`] and the
/// `KnowledgeStore` must still build and serve BM25 recall.
///
/// WHY: A broken embedding model (missing files, HF fetch failure, bad
/// config) must not crash startup — basic conversation and text-based
/// recall should keep working in degraded mode.
#[test]
fn knowledge_store_builds_and_bm25_works_with_degraded_embedding() {
    use std::sync::Arc;

    use crate::embedding::{
        DegradedEmbeddingProvider, EmbeddingConfig, EmbeddingProvider, create_provider,
        is_degraded_provider,
    };

    // 1. Deliberately-invalid embedding config: unknown provider name.
    let bad_config = EmbeddingConfig {
        provider: "this-provider-does-not-exist".to_owned(),
        model: None,
        dimension: Some(4),
        api_key: None,
        base_url: None,
    };
    assert!(
        create_provider(&bad_config).is_err(),
        "invalid embedding config must yield Err, not panic"
    );

    // 2. Fallback: DegradedEmbeddingProvider stands in.
    let fallback: Arc<dyn EmbeddingProvider> = Arc::new(DegradedEmbeddingProvider::new(4));
    assert!(
        is_degraded_provider(fallback.as_ref()),
        "fallback must be flagged as degraded for health reporting"
    );
    assert_eq!(fallback.dimension(), 4);
    assert!(
        fallback.embed("anything").is_err(),
        "degraded provider's embed() must fail so recall skips vector search"
    );

    // 3. KnowledgeStore still builds with no real embeddings available.
    let store = make_store();

    // 4. Insert facts with distinct text content.
    let f1 = make_fact(
        "f-rust",
        "agent-a",
        "Rust memory safety is enforced by the borrow checker",
    );
    let f2 = make_fact(
        "f-python",
        "agent-a",
        "Python uses reference counting for garbage collection",
    );
    let f3 = make_fact(
        "f-go",
        "agent-a",
        "Go has a concurrent tracing garbage collector",
    );
    store.insert_fact(&f1).expect("insert f1");
    store.insert_fact(&f2).expect("insert f2");
    store.insert_fact(&f3).expect("insert f3");

    // 5. BM25 search (no embeddings required) returns results.
    let results = store
        .search_text_for_recall("garbage collector", 10)
        .expect("BM25 recall must succeed in degraded mode");
    assert!(
        !results.is_empty(),
        "BM25 must return matches for 'garbage collector' even without embeddings"
    );
    let ids: Vec<String> = results.iter().map(|r| r.source_id.clone()).collect();
    assert!(
        ids.iter().any(|id| id == "f-python" || id == "f-go"),
        "BM25 must match documents containing 'garbage' or 'collector'; got ids {ids:?}"
    );
}

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

#[test]
fn scoped_visible_fact_query_hides_foreign_private_and_keeps_shared() {
    let store = make_store();
    store
        .insert_fact(&make_visible_fact(
            "alice-private",
            "alice",
            "alice private recall secret",
            Visibility::Private,
        ))
        .expect("insert alice private");
    store
        .insert_fact(&make_visible_fact(
            "alice-shared",
            "alice",
            "alice shared recall note",
            Visibility::Shared,
        ))
        .expect("insert alice shared");

    let visible = store
        .query_visible_facts("bob", "2026-06-01T00:00:00Z", 10)
        .expect("query visible facts");
    let ids: Vec<&str> = visible.iter().map(|fact| fact.id.as_str()).collect();

    assert!(
        !ids.contains(&"alice-private"),
        "foreign private fact must be hidden from direct visible fact query"
    );
    assert!(
        ids.contains(&"alice-shared"),
        "shared fact must remain visible across nouses"
    );
}

#[test]
fn public_scoped_visibility_policy_matches_store_fixture() {
    let store = make_store();
    store
        .insert_fact(&make_visible_fact(
            "policy-private",
            "alice",
            "alice private policy note",
            Visibility::Private,
        ))
        .expect("insert private");
    store
        .insert_fact(&make_visible_fact(
            "policy-shared",
            "alice",
            "alice shared policy note",
            Visibility::Shared,
        ))
        .expect("insert shared");

    let script = format!(
        r"
        {visible}
        ?[id] :=
            visible_fact[id],
            *facts{{id, is_forgotten}},
            is_forgotten == false
        :order id
        ",
        visible = crate::knowledge_store::scoped_visibility_rules()
    );
    let mut params = std::collections::BTreeMap::new();
    params.insert(
        crate::knowledge_store::ScopedVisibilityPolicy::REQUESTER_NOUS_ID_PARAM.to_owned(),
        crate::engine::DataValue::Str("bob".into()),
    );

    let rows = store.run_query(&script, params).expect("visible facts");
    let ids: Vec<String> = (0..rows.row_count())
        .filter_map(|row| rows.get_string(row, "id"))
        .collect();

    assert!(!ids.iter().any(|id| id == "policy-private"));
    assert!(ids.iter().any(|id| id == "policy-shared"));
}

#[test]
fn scoped_bm25_hides_foreign_private_and_keeps_shared() {
    let store = make_store();
    store
        .insert_fact(&make_visible_fact(
            "bm25-private",
            "alice",
            "cross nous telescope topic private",
            Visibility::Private,
        ))
        .expect("insert private");
    store
        .insert_fact(&make_visible_fact(
            "bm25-shared",
            "alice",
            "cross nous telescope topic shared",
            Visibility::Shared,
        ))
        .expect("insert shared");

    let results = store
        .search_text_for_recall_scoped("telescope topic", 10, "bob")
        .expect("scoped bm25");
    let ids = result_ids(&results);

    assert!(!ids.contains(&"bm25-private"));
    assert!(ids.contains(&"bm25-shared"));
}

#[test]
fn scoped_vector_search_hides_foreign_private_and_keeps_shared() {
    let store = make_store();
    for (id, visibility) in [
        ("vec-private", Visibility::Private),
        ("vec-shared", Visibility::Shared),
    ] {
        store
            .insert_fact(&make_visible_fact(
                id,
                "alice",
                &format!("{id} semantic recall"),
                visibility,
            ))
            .expect("insert fact");
        let mut chunk = make_embedding(
            &format!("emb-{id}"),
            &format!("{id} semantic recall"),
            id,
            "alice",
        );
        chunk.embedding = vec![1.0, 0.0, 0.0, 0.0];
        store.insert_embedding(&chunk).expect("insert embedding");
    }

    let results = store
        .search_vectors_scoped(vec![1.0, 0.0, 0.0, 0.0], 10, 20, "bob")
        .expect("scoped vector search");
    let ids = result_ids(&results);

    assert!(!ids.contains(&"vec-private"));
    assert!(ids.contains(&"vec-shared"));
}

#[test]
fn scoped_hybrid_search_hides_foreign_private_and_keeps_shared() {
    let store = make_store();
    for (id, visibility) in [
        ("hybrid-private", Visibility::Private),
        ("hybrid-shared", Visibility::Shared),
    ] {
        store
            .insert_fact(&make_visible_fact(
                id,
                "alice",
                &format!("hybrid anchor {id}"),
                visibility,
            ))
            .expect("insert fact");
        let mut chunk = make_embedding(
            &format!("emb-{id}"),
            &format!("hybrid anchor {id}"),
            id,
            "alice",
        );
        chunk.embedding = vec![1.0, 0.0, 0.0, 0.0];
        store.insert_embedding(&chunk).expect("insert embedding");
    }

    let results = store
        .search_hybrid_scoped(
            &crate::knowledge_store::HybridQuery {
                text: "hybrid anchor".to_owned(),
                embedding: vec![1.0, 0.0, 0.0, 0.0],
                seed_entities: Vec::new(),
                limit: 10,
                ef: 20,
            },
            "bob",
        )
        .expect("scoped hybrid search");
    let ids: Vec<&str> = results.iter().map(|result| result.id.as_str()).collect();

    assert!(!ids.contains(&"hybrid-private"));
    assert!(ids.contains(&"hybrid-shared"));
}

/// Requirement #5846: forgotten (soft-deleted) facts must not leak into scoped
/// hybrid search results, even when the underlying BM25/vector/graph indices
/// still match the query.
#[test]
fn scoped_hybrid_search_excludes_forgotten_facts() {
    let store = make_store();
    let fact = make_visible_fact(
        "hybrid-forgotten",
        "alice",
        "hybrid anchor forgotten",
        Visibility::Private,
    );
    store.insert_fact(&fact).expect("insert fact");

    let mut chunk = make_embedding(
        "emb-forgotten",
        "hybrid anchor forgotten",
        "hybrid-forgotten",
        "alice",
    );
    chunk.embedding = vec![1.0, 0.0, 0.0, 0.0];
    store.insert_embedding(&chunk).expect("insert embedding");

    store
        .forget_fact(
            &crate::id::FactId::new("hybrid-forgotten").expect("valid test id"),
            ForgetReason::UserRequested,
        )
        .expect("forget fact");

    let results = store
        .search_hybrid_scoped(
            &crate::knowledge_store::HybridQuery {
                text: "hybrid anchor".to_owned(),
                embedding: vec![1.0, 0.0, 0.0, 0.0],
                seed_entities: Vec::new(),
                limit: 10,
                ef: 20,
            },
            "alice",
        )
        .expect("scoped hybrid search");
    let ids: Vec<&str> = results.iter().map(|result| result.id.as_str()).collect();

    assert!(
        !ids.contains(&"hybrid-forgotten"),
        "forgotten fact must not appear in scoped hybrid search results; got {ids:?}"
    );
}

#[test]
fn scoped_cluster_expansion_hides_foreign_private_cluster_mates() {
    let store = make_store();
    let shared = make_visible_fact(
        "cluster-shared",
        "alice",
        "cluster anchor visible",
        Visibility::Shared,
    );
    let private = make_visible_fact(
        "cluster-private",
        "alice",
        "cluster hidden private",
        Visibility::Private,
    );
    store.insert_fact(&shared).expect("insert shared");
    store.insert_fact(&private).expect("insert private");

    let shared_entity = make_entity("entity-cluster-shared", "Shared Anchor", "topic");
    let private_entity = make_entity("entity-cluster-private", "Private Anchor", "topic");
    store
        .insert_entity(&shared_entity)
        .expect("insert shared entity");
    store
        .insert_entity(&private_entity)
        .expect("insert private entity");
    store
        .insert_fact_entity(&shared.id, &shared_entity.id)
        .expect("link shared");
    store
        .insert_fact_entity(&private.id, &private_entity.id)
        .expect("link private");

    store
        .run_mut_query(
            r#"
            ?[entity_id, score_type, score, cluster_id, updated_at] <- [
                ["entity-cluster-shared", "cluster", 0.0, 7, "2026-06-01T00:00:00Z"],
                ["entity-cluster-private", "cluster", 0.0, 7, "2026-06-01T00:00:00Z"]
            ]
            :put graph_scores { entity_id, score_type => score, cluster_id, updated_at }
            "#,
            std::collections::BTreeMap::new(),
        )
        .expect("seed cluster scores");

    let results = store
        .search_text_for_recall_scoped("cluster anchor visible", 10, "bob")
        .expect("scoped bm25 with cluster expansion");
    let ids = result_ids(&results);

    assert!(ids.contains(&"cluster-shared"));
    assert!(
        !ids.contains(&"cluster-private"),
        "cluster expansion must not add foreign private facts"
    );
}

/// #4620: cluster expansion must hydrate a cluster-mate's real scope, project,
/// visibility, and sensitivity rather than synthesizing permissive defaults
/// (`None` / `Public` / `Private`). A regression turns a project-scoped or
/// confidential cluster mate into a globally eligible candidate, because the
/// downstream quota and visibility filters trust whatever metadata expansion
/// attached.
#[test]
fn cluster_expansion_hydrates_scope_project_and_sensitivity() {
    let store = make_store();
    let project =
        eidos::workspace::ProjectId::from_git_remote("https://github.com/acme/cluster.git")
            .expect("valid remote");

    // Anchor fact: matched by the BM25 query, selects the cluster.
    let anchor = make_visible_fact(
        "cluster-anchor",
        "alice",
        "cluster anchor visible",
        Visibility::Shared,
    );
    store.insert_fact(&anchor).expect("insert anchor");

    // Cluster mate: project-scoped and confidential. Shared visibility keeps it
    // in the unscoped expansion so we can observe whether its policy metadata
    // survived, rather than being filtered out for an unrelated reason.
    let mut mate = make_visible_fact(
        "cluster-mate",
        "alice",
        "unrelated cluster mate body",
        Visibility::Shared,
    );
    mate.scope = Some(crate::knowledge::MemoryScope::Project);
    mate.project_id = Some(project.clone());
    mate.sensitivity = crate::knowledge::FactSensitivity::Confidential;
    store.insert_fact(&mate).expect("insert mate");

    let anchor_entity = make_entity("entity-anchor", "Anchor", "topic");
    let mate_entity = make_entity("entity-mate", "Mate", "topic");
    store
        .insert_entity(&anchor_entity)
        .expect("insert anchor entity");
    store
        .insert_entity(&mate_entity)
        .expect("insert mate entity");
    store
        .insert_fact_entity(&anchor.id, &anchor_entity.id)
        .expect("link anchor");
    store
        .insert_fact_entity(&mate.id, &mate_entity.id)
        .expect("link mate");

    store
        .run_mut_query(
            r#"
            ?[entity_id, score_type, score, cluster_id, updated_at] <- [
                ["entity-anchor", "cluster", 0.0, 11, "2026-06-01T00:00:00Z"],
                ["entity-mate", "cluster", 0.0, 11, "2026-06-01T00:00:00Z"]
            ]
            :put graph_scores { entity_id, score_type => score, cluster_id, updated_at }
            "#,
            std::collections::BTreeMap::new(),
        )
        .expect("seed cluster scores");

    let results = store
        .search_text_for_recall("cluster anchor visible", 10)
        .expect("bm25 with cluster expansion");
    let mate_result = results
        .iter()
        .find(|result| result.source_id.as_str() == "cluster-mate")
        .expect("cluster mate added via expansion");

    assert_eq!(
        mate_result.scope,
        Some(crate::knowledge::MemoryScope::Project),
        "expansion must hydrate real scope, not default to None"
    );
    assert_eq!(
        mate_result.project_id,
        Some(project),
        "expansion must hydrate real project_id, not default to None"
    );
    assert_eq!(
        mate_result.sensitivity,
        crate::knowledge::FactSensitivity::Confidential,
        "expansion must hydrate real sensitivity, not default to Public"
    );
    assert_eq!(
        mate_result.visibility,
        Visibility::Shared,
        "expansion must hydrate real visibility, not default to Private"
    );
}

#[test]
fn cluster_expansion_uses_fact_entities_created_by_extraction_persist() {
    let store = make_store();
    let engine = crate::extract::ExtractionEngine::new(crate::extract::ExtractionConfig::default());
    let extraction = crate::extract::Extraction {
        entities: vec![
            extraction_entity("Recall Anchor", "topic"),
            extraction_entity("Cluster Mate", "topic"),
            extraction_entity("Shared Topic", "topic"),
        ],
        relationships: vec![],
        facts: vec![
            crate::extract::ExtractedFact {
                subject: "Recall Anchor".to_owned(),
                predicate: "links".to_owned(),
                object: "Shared Topic".to_owned(),
                confidence: 0.95,
                fact_type: None,
                is_correction: false,
            },
            crate::extract::ExtractedFact {
                subject: "Cluster Mate".to_owned(),
                predicate: "uses".to_owned(),
                object: "Shared Topic".to_owned(),
                confidence: 0.95,
                fact_type: None,
                is_correction: false,
            },
        ],
    };

    let persisted = engine
        .persist(&extraction, &store, "session:test", "alice")
        .expect("persist extraction");
    assert_eq!(
        persisted.fact_entities_inserted, 4,
        "persist must create fact-entity edges for both extracted facts"
    );

    store
        .run_mut_query(
            r#"
            ?[entity_id, score_type, score, cluster_id, updated_at] <- [
                ["recall-anchor", "cluster", 0.0, 19, "2026-06-01T00:00:00Z"],
                ["cluster-mate", "cluster", 0.0, 19, "2026-06-01T00:00:00Z"]
            ]
            :put graph_scores { entity_id, score_type => score, cluster_id, updated_at }
            "#,
            std::collections::BTreeMap::new(),
        )
        .expect("seed cluster scores");

    let results = store
        .search_text_for_recall("Recall Anchor links", 10)
        .expect("bm25 with extraction-created cluster expansion");
    let ids = result_ids(&results);

    assert!(ids.contains(&"recall-anchor-links-0"));
    assert!(
        ids.contains(&"cluster-mate-uses-1"),
        "cluster mate must be discovered through extraction-created fact_entities"
    );
}

#[test]
fn search_enhanced_errors_when_every_variant_fails() {
    let store = make_store();
    let query = crate::knowledge_store::HybridQuery {
        text: "rust".to_owned(),
        embedding: vec![1.0],
        seed_entities: Vec::new(),
        limit: 3,
        ef: 10,
    };

    let result = store.search_enhanced(&query, &["rust memory".to_owned()]);

    assert!(
        matches!(result, Err(crate::error::Error::EnhancedSearch { .. })),
        "all failed enhanced-search variants must return typed error, got {result:?}"
    );
}

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

    let mut chunk_a = make_embedding("emb-a", "Rust programming", "f1", "agent-a");
    chunk_a.embedding = vec![1.0, 0.0, 0.0, 0.0];
    store.insert_embedding(&chunk_a).expect("insert emb-a");

    let mut chunk_b = make_embedding("emb-b", "Python scripting", "f2", "agent-a");
    chunk_b.embedding = vec![0.0, 1.0, 0.0, 0.0];
    store.insert_embedding(&chunk_b).expect("insert emb-b");

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

#[test]
fn search_vectors_dimension_mismatch_errors() {
    let store = make_store();
    let wrong_dim = vec![0.1, 0.2, 0.3, 0.4, 0.5];
    let result = store.search_vectors(wrong_dim, 5, 16);
    assert!(result.is_err(), "search with wrong dimension should error");
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

mod correctness;
