//! Recall pipeline stage — retrieves relevant knowledge and injects into context.

use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_mneme::knowledge::RecallResult as KnowledgeRecallResult;
use aletheia_mneme::recall::{FactorScores, RecallEngine, ScoredResult};

use crate::error;

/// Abstracts vector knowledge search.
///
/// `KnowledgeStore` implements this when the `mneme-engine` feature is available.
/// For tests, use `MockVectorSearch`.
pub trait VectorSearch: Send + Sync {
    /// Search for the `k` nearest vectors with HNSW `ef` parameter.
    fn search_vectors(
        &self,
        query_vec: Vec<f32>,
        k: usize,
        ef: usize,
    ) -> error::Result<Vec<KnowledgeRecallResult>>;
}

/// Configuration for the recall stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallConfig {
    /// Whether recall is enabled.
    pub enabled: bool,
    /// Maximum number of recalled items to inject.
    pub max_results: usize,
    /// Minimum score threshold to include a result.
    pub min_score: f64,
    /// Maximum tokens to allocate for recalled knowledge.
    pub max_recall_tokens: u64,
}

impl Default for RecallConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_results: 5,
            min_score: 0.3,
            max_recall_tokens: 2000,
        }
    }
}

/// Output of the recall pipeline stage.
#[derive(Debug, Clone)]
pub struct RecallStageResult {
    /// Number of candidates retrieved from knowledge store.
    pub candidates_found: usize,
    /// Number that passed scoring threshold.
    pub results_injected: usize,
    /// Tokens consumed by injected knowledge.
    pub tokens_consumed: u64,
    /// The formatted recall section (appended to system prompt).
    pub recall_section: Option<String>,
}

impl RecallStageResult {
    fn empty() -> Self {
        Self {
            candidates_found: 0,
            results_injected: 0,
            tokens_consumed: 0,
            recall_section: None,
        }
    }
}

/// Recall stage — scores and formats knowledge for injection into the system prompt.
pub struct RecallStage {
    engine: RecallEngine,
    config: RecallConfig,
}

impl RecallStage {
    /// Create a recall stage with default scoring weights.
    #[must_use]
    pub fn new(config: RecallConfig) -> Self {
        Self {
            engine: RecallEngine::new(),
            config,
        }
    }

    /// Run the recall stage.
    ///
    /// Embeds the query, searches for nearest vectors, scores and ranks results,
    /// then formats the top results as a markdown section for the system prompt.
    ///
    /// Non-fatal errors are returned as `Err` — the caller should catch and continue.
    #[instrument(skip_all, fields(nous_id = %nous_id))]
    pub fn run(
        &self,
        query: &str,
        nous_id: &str,
        embedding_provider: &dyn EmbeddingProvider,
        vector_search: &dyn VectorSearch,
        remaining_budget: u64,
    ) -> error::Result<RecallStageResult> {
        if !self.config.enabled {
            debug!("recall disabled");
            return Ok(RecallStageResult::empty());
        }

        let query_vec = embedding_provider.embed(query).map_err(|e| {
            error::RecallEmbeddingSnafu {
                message: e.to_string(),
            }
            .build()
        })?;

        let k = self.config.max_results * 3;
        let raw_results = vector_search
            .search_vectors(query_vec, k, 50)
            .map_err(|e| {
                error::RecallSearchSnafu {
                    message: e.to_string(),
                }
                .build()
            })?;

        let candidates_found = raw_results.len();
        if candidates_found == 0 {
            debug!("no recall candidates found");
            return Ok(RecallStageResult::empty());
        }

        let candidates = self.build_candidates(raw_results, nous_id);
        let ranked = self.engine.rank(candidates);
        let filtered = self.filter(ranked);

        if filtered.is_empty() {
            debug!(candidates_found, "all candidates below min_score");
            return Ok(RecallStageResult {
                candidates_found,
                ..RecallStageResult::empty()
            });
        }

        let budget = remaining_budget.min(self.config.max_recall_tokens);
        let (final_results, section, tokens) = self.format_within_budget(&filtered, budget);

        debug!(
            candidates_found,
            results_injected = final_results,
            tokens_consumed = tokens,
            "recall complete"
        );

        Ok(RecallStageResult {
            candidates_found,
            results_injected: final_results,
            tokens_consumed: tokens,
            recall_section: Some(section),
        })
    }

    fn build_candidates(
        &self,
        raw: Vec<KnowledgeRecallResult>,
        _nous_id: &str,
    ) -> Vec<ScoredResult> {
        raw.into_iter()
            .map(|r| ScoredResult {
                content: r.content,
                source_type: r.source_type,
                source_id: r.source_id,
                nous_id: String::new(),
                factors: FactorScores {
                    vector_similarity: self.engine.score_vector_similarity(r.distance),
                    recency: 0.5,
                    relevance: 0.5,
                    epistemic_tier: 0.3,
                    relationship_proximity: 0.0,
                    access_frequency: 0.0,
                },
                score: 0.0,
            })
            .collect()
    }

    fn filter(&self, ranked: Vec<ScoredResult>) -> Vec<ScoredResult> {
        ranked
            .into_iter()
            .filter(|r| r.score >= self.config.min_score)
            .take(self.config.max_results)
            .collect()
    }

    #[expect(clippy::unused_self, reason = "will use config fields when budget strategy is extended")]
    fn format_within_budget(
        &self,
        results: &[ScoredResult],
        budget: u64,
    ) -> (usize, String, u64) {
        let mut included = Vec::with_capacity(results.len());

        for result in results {
            included.push(result);
            let section = format_section(&included);
            let tokens = estimate_tokens(&section);
            if tokens > budget {
                included.pop();
                break;
            }
        }

        if included.is_empty() {
            return (0, String::new(), 0);
        }

        let section = format_section(&included);
        let tokens = estimate_tokens(&section);
        (included.len(), section, tokens)
    }
}

/// Format scored results as a markdown section.
#[must_use]
pub fn format_section(results: &[&ScoredResult]) -> String {
    use std::fmt::Write;

    let mut out = String::from(
        "## Recalled Knowledge\n\nThe following facts were recalled from memory (relevance score in brackets):\n",
    );

    for r in results {
        let _ = write!(out, "\n- [{:.2}] {}", r.score, r.content);
    }

    out
}

/// Estimate token count from text length (~4 chars per token, ceiling).
#[must_use]
pub fn estimate_tokens(text: &str) -> u64 {
    let len = text.len() as u64;
    len.div_ceil(4)
}

#[cfg(test)]
mod tests {
    use aletheia_mneme::embedding::MockEmbeddingProvider;

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

    fn mock_embed() -> MockEmbeddingProvider {
        MockEmbeddingProvider::new(384)
    }

    fn make_knowledge_result(content: &str, distance: f64) -> KnowledgeRecallResult {
        KnowledgeRecallResult {
            content: content.to_owned(),
            distance,
            source_type: "fact".to_owned(),
            source_id: format!("fact-{}", content.len()),
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
        }
    }

    #[test]
    fn recall_disabled_returns_empty() {
        let config = RecallConfig {
            enabled: false,
            ..Default::default()
        };
        let stage = RecallStage::new(config);
        let result = stage
            .run("query", "syn", &mock_embed(), &MockVectorSearch::empty(), 10000)
            .unwrap();
        assert_eq!(result.candidates_found, 0);
        assert_eq!(result.results_injected, 0);
        assert!(result.recall_section.is_none());
    }

    #[test]
    fn recall_empty_candidates_returns_empty() {
        let config = RecallConfig::default();
        let stage = RecallStage::new(config);
        let result = stage
            .run("query", "syn", &mock_embed(), &MockVectorSearch::empty(), 10000)
            .unwrap();
        assert_eq!(result.candidates_found, 0);
        assert!(result.recall_section.is_none());
    }

    #[test]
    fn recall_formats_section_correctly() {
        let a = make_scored("User prefers dark mode", 0.87);
        let b = make_scored("Project deadline is March 15", 0.72);
        let refs: Vec<&ScoredResult> = vec![&a, &b];
        let section = format_section(&refs);

        assert!(section.starts_with("## Recalled Knowledge"));
        assert!(section.contains("[0.87] User prefers dark mode"));
        assert!(section.contains("[0.72] Project deadline is March 15"));
    }

    #[test]
    fn recall_respects_min_score() {
        let results = vec![
            make_knowledge_result("close match", 0.1),
            make_knowledge_result("medium match", 0.8),
            make_knowledge_result("distant match", 1.5),
        ];
        let config = RecallConfig {
            min_score: 0.4,
            ..Default::default()
        };
        let stage = RecallStage::new(config);
        let result = stage
            .run("query", "syn", &mock_embed(), &MockVectorSearch::new(results), 10000)
            .unwrap();

        assert_eq!(result.candidates_found, 3);
        assert!(result.results_injected <= 3);
        if let Some(ref section) = result.recall_section {
            assert!(!section.contains("distant match"));
        }
    }

    #[test]
    fn recall_respects_max_results() {
        let results: Vec<KnowledgeRecallResult> = (0..10)
            .map(|i| make_knowledge_result(&format!("fact {i}"), 0.1 + f64::from(i) * 0.05))
            .collect();
        let config = RecallConfig {
            max_results: 3,
            min_score: 0.0,
            ..Default::default()
        };
        let stage = RecallStage::new(config);
        let result = stage
            .run("query", "syn", &mock_embed(), &MockVectorSearch::new(results), 50000)
            .unwrap();

        assert_eq!(result.candidates_found, 10);
        assert!(result.results_injected <= 3);
    }

    #[test]
    fn recall_respects_token_budget() {
        let long_content = "x".repeat(400);
        let results: Vec<KnowledgeRecallResult> = (0..5)
            .map(|i| make_knowledge_result(&format!("{long_content} {i}"), 0.1))
            .collect();
        let config = RecallConfig {
            max_results: 5,
            min_score: 0.0,
            max_recall_tokens: 200,
            ..Default::default()
        };
        let stage = RecallStage::new(config);
        let result = stage
            .run("query", "syn", &mock_embed(), &MockVectorSearch::new(results), 200)
            .unwrap();

        assert!(result.tokens_consumed <= 200);
        assert!(result.results_injected < 5);
    }

    #[test]
    fn estimate_tokens_heuristic() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
        let text = "x".repeat(400);
        assert_eq!(estimate_tokens(&text), 100);
    }

    #[test]
    fn vector_search_trait_is_object_safe() {
        fn _assert_object_safe(_: &dyn VectorSearch) {}
    }
}
