//! Integration: recall pipeline end-to-end with mock providers.

use aletheia_mneme::embedding::MockEmbeddingProvider;
use aletheia_mneme::knowledge::RecallResult as KnowledgeRecallResult;
use aletheia_nous::recall::{RecallConfig, RecallStage, VectorSearch};

struct MockVectorSearch {
    results: Vec<KnowledgeRecallResult>,
}

impl VectorSearch for MockVectorSearch {
    fn search_vectors(
        &self,
        _query_vec: Vec<f32>,
        _k: usize,
        _ef: usize,
    ) -> aletheia_nous::error::Result<Vec<KnowledgeRecallResult>> {
        Ok(self.results.clone())
    }
}

#[test]
fn recall_with_mock_vectors_end_to_end() {
    let embedder = MockEmbeddingProvider::new(384);
    let search = MockVectorSearch {
        results: vec![
            KnowledgeRecallResult {
                content: "The researcher published findings on memory consolidation".to_owned(),
                distance: 0.05,
                source_type: "fact".to_owned(),
                source_id: "f-1".to_owned(),
            },
            KnowledgeRecallResult {
                content: "Aletheia uses Tokio actors".to_owned(),
                distance: 0.3,
                source_type: "fact".to_owned(),
                source_id: "f-2".to_owned(),
            },
        ],
    };

    let stage = RecallStage::new(RecallConfig {
        min_score: 0.0,
        ..RecallConfig::default()
    });

    let result = stage
        .run("What did the researcher publish?", "syn", &embedder, &search, 10000)
        .expect("recall should succeed");

    assert!(result.candidates_found >= 1);
    assert!(result.results_injected >= 1);
    let section = result.recall_section.expect("should have recall section");
    assert!(
        section.contains("The researcher published findings on memory consolidation"),
        "section should contain the closest fact"
    );
}

#[test]
fn recall_empty_store_graceful() {
    let embedder = MockEmbeddingProvider::new(384);
    let search = MockVectorSearch { results: vec![] };

    let stage = RecallStage::new(RecallConfig::default());

    let result = stage
        .run("anything", "syn", &embedder, &search, 10000)
        .expect("recall with empty store should not error");

    assert_eq!(result.candidates_found, 0);
    assert_eq!(result.results_injected, 0);
    assert!(result.recall_section.is_none());
}
