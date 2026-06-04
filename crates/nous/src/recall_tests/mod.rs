use std::sync::atomic::{AtomicUsize, Ordering};

use mneme::embedding::MockEmbeddingProvider;

// Split from monolithic recall_tests.rs to satisfy RUST/file-too-long.

use super::*;

struct MockVectorSearch {
    results: Vec<KnowledgeRecallResult>,
}

impl MockVectorSearch {
    fn new(results: Vec<KnowledgeRecallResult>) -> Self {
        Self { results }
    }

    fn empty() -> Self {
        Self::new(vec![])
    }
}

impl VectorSearch for MockVectorSearch {
    fn search_vectors(
        &self,
        _query_vec: Vec<f32>,
        _k: usize,
        _ef: usize,
    ) -> error::Result<Vec<KnowledgeRecallResult>> {
        Ok(self.results.clone())
    }
}

/// Mock that returns different results on successive search calls.
struct CycledMockSearch {
    cycles: Vec<Vec<KnowledgeRecallResult>>,
    call_index: AtomicUsize,
}

impl CycledMockSearch {
    fn new(cycles: Vec<Vec<KnowledgeRecallResult>>) -> Self {
        Self {
            cycles,
            call_index: AtomicUsize::new(0),
        }
    }

    fn call_count(&self) -> usize {
        self.call_index.load(Ordering::Relaxed)
    }
}

impl VectorSearch for CycledMockSearch {
    fn search_vectors(
        &self,
        _query_vec: Vec<f32>,
        _k: usize,
        _ef: usize,
    ) -> error::Result<Vec<KnowledgeRecallResult>> {
        let idx = self.call_index.fetch_add(1, Ordering::Relaxed);
        Ok(self.cycles.get(idx).cloned().unwrap_or_default())
    }
}

fn mock_embed() -> MockEmbeddingProvider {
    MockEmbeddingProvider::new(384)
}

fn make_knowledge_result(content: &str, distance: f64) -> KnowledgeRecallResult {
    KnowledgeRecallResult {
        content: content.to_owned(),
        distance,
        source_type: "fact".to_owned(),
        source_id: format!("fact-{}", content.len()),
        nous_id: "syn".to_owned(),
        sensitivity: mneme::knowledge::FactSensitivity::Public,
        graph_importance: 0.0,
        scope: None,
        project_id: None,
        visibility: mneme::knowledge::Visibility::Private,
        source_count: 0,
    }
}

fn make_knowledge_result_with_id(
    content: &str,
    distance: f64,
    source_id: &str,
) -> KnowledgeRecallResult {
    KnowledgeRecallResult {
        content: content.to_owned(),
        distance,
        source_type: "fact".to_owned(),
        source_id: source_id.to_owned(),
        nous_id: "syn".to_owned(),
        sensitivity: mneme::knowledge::FactSensitivity::Public,
        graph_importance: 0.0,
        scope: None,
        project_id: None,
        visibility: mneme::knowledge::Visibility::Private,
        source_count: 0,
    }
}

fn make_knowledge_result_with_scope(
    content: &str,
    distance: f64,
    scope: Option<mneme::knowledge::MemoryScope>,
) -> KnowledgeRecallResult {
    KnowledgeRecallResult {
        content: content.to_owned(),
        distance,
        source_type: "fact".to_owned(),
        source_id: format!("fact-{}", content.len()),
        nous_id: "syn".to_owned(),
        sensitivity: mneme::knowledge::FactSensitivity::Public,
        graph_importance: 0.0,
        scope,
        project_id: None,
        visibility: mneme::knowledge::Visibility::Private,
        source_count: 0,
    }
}

fn make_scored(content: &str, score: f64) -> ScoredResult {
    ScoredResult {
        content: content.to_owned(),
        source_type: "fact".to_owned(),
        source_id: "f1".to_owned(),
        nous_id: "syn".to_owned(),
        factors: FactorScores::default(),
        score,
        sensitivity: mneme::knowledge::FactSensitivity::Public,
        visibility: mneme::knowledge::Visibility::Private,
        scope: None,
        project_id: None,
    }
}

#[cfg(feature = "knowledge-store")]
mod knowledge_bridge;
mod recall_core;
mod speculative_recall;
mod terminology_discovery;
