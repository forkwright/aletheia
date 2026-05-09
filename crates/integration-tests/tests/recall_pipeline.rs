//! Integration: recall pipeline end-to-end through the real knowledge store.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::sync::Arc;

use mneme::embedding::{EmbeddingProvider, MockEmbeddingProvider};
use mneme::id::{EmbeddingId, FactId};
use mneme::knowledge::{
    EmbeddedChunk, EpistemicTier, Fact, FactAccess, FactLifecycle, FactProvenance, FactSensitivity,
    FactTemporal, Visibility,
};
use mneme::knowledge_store::{KnowledgeConfig, KnowledgeStore};
use nous::recall::{KnowledgeVectorSearch, RecallConfig, RecallStage};

const DIM: usize = 384;
const RECORDED_AT: &str = "2026-03-01T00:00:00Z";

fn ts() -> jiff::Timestamp {
    RECORDED_AT.parse().expect("valid timestamp")
}

fn open_fjall_store(dim: usize) -> (tempfile::TempDir, Arc<KnowledgeStore>) {
    let dir = tempfile::TempDir::new().expect("tmpdir");
    let store = KnowledgeStore::open_fjall(
        dir.path().join("knowledge").join("shared"),
        KnowledgeConfig {
            dim,
            ..KnowledgeConfig::default()
        },
    )
    .expect("open tempfile fjall knowledge store");
    (dir, store)
}

fn fact(id: &str, content: &str) -> Fact {
    Fact {
        id: FactId::new(id).expect("valid fact id"),
        nous_id: "syn".to_owned(),
        content: content.to_owned(),
        fact_type: "observation".to_owned(),
        scope: None,
        temporal: FactTemporal {
            valid_from: ts(),
            valid_to: mneme::knowledge::far_future(),
            recorded_at: ts(),
        },
        provenance: FactProvenance {
            confidence: 0.95,
            tier: EpistemicTier::Verified,
            source_session_id: Some("ses-recall".to_owned()),
            stability_hours: mneme::knowledge::default_stability_hours("observation"),
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
        visibility: Visibility::Private,
    }
}

fn embedded_chunk(
    embedder: &dyn EmbeddingProvider,
    embedding_id: &str,
    fact_id: &str,
    content: &str,
) -> EmbeddedChunk {
    EmbeddedChunk {
        id: EmbeddingId::new(embedding_id).expect("valid embedding id"),
        content: content.to_owned(),
        source_type: "fact".to_owned(),
        source_id: fact_id.to_owned(),
        nous_id: "syn".to_owned(),
        embedding: embedder.embed(content).expect("embed fixture"),
        created_at: ts(),
    }
}

fn seed_fact(store: &KnowledgeStore, embedder: &dyn EmbeddingProvider, id: &str, content: &str) {
    store.insert_fact(&fact(id, content)).expect("insert fact");
    store
        .insert_embedding(&embedded_chunk(embedder, &format!("emb-{id}"), id, content))
        .expect("insert embedding");
}

#[test]
fn recall_uses_real_fjall_vector_search_end_to_end() {
    let (_tmp, store) = open_fjall_store(DIM);
    let embedder = MockEmbeddingProvider::new(DIM);
    seed_fact(
        &store,
        &embedder,
        "fact-recall-1",
        "The researcher published findings on memory consolidation",
    );
    seed_fact(
        &store,
        &embedder,
        "fact-recall-2",
        "Aletheia uses Tokio actors",
    );

    let search = KnowledgeVectorSearch::new(Arc::clone(&store));
    let stage = RecallStage::new(RecallConfig {
        min_score: 0.0,
        ..RecallConfig::default()
    });

    let result = stage
        .run(
            "The researcher published findings on memory consolidation",
            "syn",
            &embedder,
            &search,
            10000,
        )
        .expect("recall should succeed");

    assert!(
        result.candidates_found >= 1,
        "specific invariant: real vector search must return at least one candidate"
    );
    assert!(
        result.results_injected >= 1,
        "specific invariant: recall stage must inject at least one stored result"
    );
    let section = result.recall_section.expect("should have recall section");
    assert!(
        section.contains("The researcher published findings on memory consolidation"),
        "specific invariant: section should contain the real-store nearest fact; got: {section}"
    );
}

#[test]
fn recall_empty_real_store_is_graceful() {
    let (_tmp, store) = open_fjall_store(DIM);
    let embedder = MockEmbeddingProvider::new(DIM);
    let search = KnowledgeVectorSearch::new(store);
    let stage = RecallStage::new(RecallConfig::default());

    let result = stage
        .run("anything", "syn", &embedder, &search, 10000)
        .expect("recall with empty store should not error");

    assert_eq!(
        result.candidates_found, 0,
        "specific invariant: empty real store has zero candidates"
    );
    assert_eq!(
        result.results_injected, 0,
        "specific invariant: empty real store injects no results"
    );
    assert!(
        result.recall_section.is_none(),
        "specific invariant: empty real store has no recall section"
    );
}

#[test]
fn recall_surfaces_real_store_dimension_mismatch() {
    let (_tmp, store) = open_fjall_store(4);
    let embedder = MockEmbeddingProvider::new(DIM);
    let search = KnowledgeVectorSearch::new(store);
    let stage = RecallStage::new(RecallConfig::default());

    let result = stage.run("dimension mismatch", "syn", &embedder, &search, 10000);

    assert!(
        result.is_err(),
        "specific invariant: real vector search must reject query/store dimension mismatch"
    );
}
