//! Recall pipeline stage: retrieves relevant knowledge and injects into context.

mod reranking;
mod scoring;
mod search;

use std::collections::HashSet;
#[cfg(feature = "knowledge-store")]
use std::sync::Arc;

use tracing::{debug, instrument};

use aletheia_mneme::embedding::EmbeddingProvider;
use aletheia_mneme::knowledge::RecallResult as KnowledgeRecallResult;
use aletheia_mneme::recall::{FactorScores, RecallEngine, ScoredResult};

use crate::error;

pub use scoring::{RecallConfig, RecallWeights};
pub(crate) use scoring::{estimate_tokens, format_section};
#[cfg(feature = "knowledge-store")]
pub(crate) use search::KnowledgeTextSearch;
#[cfg(feature = "knowledge-store")]
pub use search::KnowledgeVectorSearch;
pub(crate) use search::TextSearch;
pub use search::VectorSearch;

#[cfg(test)]
use reranking::is_stopword;
use reranking::{detect_gaps, discover_terminology};
use search::{embed, vector_search};

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

/// Recall stage: scores and formats knowledge for injection into the system prompt.
pub struct RecallStage {
    engine: RecallEngine,
    config: RecallConfig,
}

impl RecallStage {
    /// Create a recall stage, wiring operator-configured engine weights.
    #[must_use]
    pub fn new(config: RecallConfig) -> Self {
        let ew = &config.engine_weights;
        let engine_weights = aletheia_mneme::recall::RecallWeights {
            vector_similarity: ew.vector_similarity,
            decay: ew.decay,
            relevance: ew.relevance,
            epistemic_tier: ew.epistemic_tier,
            relationship_proximity: ew.relationship_proximity,
            access_frequency: ew.access_frequency,
        };
        Self {
            engine: RecallEngine::with_weights(engine_weights),
            config,
        }
    }

    /// Run recall using BM25 text search only (no vector embeddings required).
    ///
    /// Used as a fallback when the embedding provider is in mock mode.
    /// Scores, ranks, and formats results the same way as [`run`](Self::run).
    pub(crate) fn run_bm25(
        &self,
        query: &str,
        nous_id: &str,
        text_search: &dyn TextSearch,
        remaining_budget: u64,
    ) -> error::Result<RecallStageResult> {
        if !self.config.enabled {
            debug!("recall disabled");
            return Ok(RecallStageResult::empty());
        }

        let k = self.config.max_results * 3;
        let raw = text_search.search_text(query, k)?;

        if raw.is_empty() {
            debug!("no BM25 recall candidates found");
            return Ok(RecallStageResult::empty());
        }

        let candidates = self.build_candidates(raw, nous_id);
        let ranked = self.engine.rank(candidates);
        Ok(self.finalize_results(ranked, remaining_budget))
    }

    /// Run the recall stage.
    ///
    /// Embeds the query, searches for nearest vectors, scores and ranks results,
    /// then formats the top results as a markdown section for the system prompt.
    /// When `iterative` is enabled, runs a second cycle with terminology-refined queries.
    ///
    /// Non-fatal errors are returned as `Err`: the caller should catch and continue.
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

        if self.config.iterative && self.config.max_cycles > 1 {
            self.run_iterative(
                query,
                nous_id,
                embedding_provider,
                vector_search,
                remaining_budget,
            )
        } else {
            self.run_single(
                query,
                nous_id,
                embedding_provider,
                vector_search,
                remaining_budget,
            )
        }
    }

    fn run_single(
        &self,
        query: &str,
        nous_id: &str,
        embedding_provider: &dyn EmbeddingProvider,
        vs: &dyn VectorSearch,
        remaining_budget: u64,
    ) -> error::Result<RecallStageResult> {
        let k = self.config.max_results * 3;
        let query_vec = embed(query, embedding_provider)?;
        let raw = vector_search(vs, query_vec, k)?;

        if raw.is_empty() {
            debug!("no recall candidates found");
            return Ok(RecallStageResult::empty());
        }

        let candidates = self.build_candidates(raw, nous_id);
        let ranked = self.engine.rank(candidates);
        Ok(self.finalize_results(ranked, remaining_budget))
    }

    fn run_iterative(
        &self,
        query: &str,
        nous_id: &str,
        embedding_provider: &dyn EmbeddingProvider,
        vs: &dyn VectorSearch,
        remaining_budget: u64,
    ) -> error::Result<RecallStageResult> {
        let k = self.config.max_results * 3;

        let query_vec = embed(query, embedding_provider)?;
        let raw_cycle1 = vector_search(vs, query_vec, k)?;

        if raw_cycle1.is_empty() {
            debug!("no recall candidates in cycle 1");
            return Ok(RecallStageResult::empty());
        }

        let candidates_c1 = self.build_candidates(raw_cycle1.clone(), nous_id);
        let ranked_c1 = self.engine.rank(candidates_c1);

        let terms = discover_terminology(&ranked_c1, query);
        let gaps = detect_gaps(&ranked_c1);

        if terms.is_empty() && gaps.is_empty() {
            debug!("no novel terms or gaps discovered, skipping cycle 2");
            return Ok(self.finalize_results(ranked_c1, remaining_budget));
        }

        let mut refined = String::from(query);
        for term in &terms {
            refined.push(' ');
            refined.push_str(term);
        }
        for gap in &gaps {
            refined.push(' ');
            refined.push_str(gap);
        }

        debug!(
            new_terms = terms.len(),
            gaps = gaps.len(),
            refined = refined.as_str(),
            "cycle 2 with refined query"
        );

        let refined_vec = embed(&refined, embedding_provider)?;
        let raw_cycle2 = vector_search(vs, refined_vec, k)?;

        let mut seen: HashSet<String> = HashSet::new();
        let mut merged: Vec<KnowledgeRecallResult> = Vec::new();
        for r in raw_cycle1 {
            if seen.insert(r.source_id.clone()) {
                merged.push(r);
            }
        }
        for r in raw_cycle2 {
            if seen.insert(r.source_id.clone()) {
                merged.push(r);
            }
        }

        debug!(
            unique_candidates = merged.len(),
            "merged results from 2 cycles"
        );

        let candidates = self.build_candidates(merged, nous_id);
        let ranked = self.engine.rank(candidates);
        Ok(self.finalize_results(ranked, remaining_budget))
    }

    fn finalize_results(
        &self,
        ranked: Vec<ScoredResult>,
        remaining_budget: u64,
    ) -> RecallStageResult {
        let candidates_found = ranked.len();
        let filtered = self.filter(ranked);

        if filtered.is_empty() {
            debug!(candidates_found, "all candidates below min_score");
            return RecallStageResult {
                candidates_found,
                ..RecallStageResult::empty()
            };
        }

        let budget = remaining_budget.min(self.config.max_recall_tokens);
        let (results_injected, section, tokens) = self.format_within_budget(&filtered, budget);

        debug!(
            candidates_found,
            results_injected,
            tokens_consumed = tokens,
            "recall complete"
        );

        RecallStageResult {
            candidates_found,
            results_injected,
            tokens_consumed: tokens,
            recall_section: if section.is_empty() {
                None
            } else {
                Some(section)
            },
        }
    }

    fn build_candidates(
        &self,
        raw: Vec<KnowledgeRecallResult>,
        _nous_id: &str,
    ) -> Vec<ScoredResult> {
        let w = &self.config.weights;
        raw.into_iter()
            .map(|r| ScoredResult {
                content: r.content,
                source_type: r.source_type,
                source_id: r.source_id,
                nous_id: String::new(),
                factors: FactorScores {
                    vector_similarity: self.engine.score_vector_similarity(r.distance),
                    decay: w.decay,
                    relevance: w.relevance,
                    epistemic_tier: w.epistemic_tier,
                    relationship_proximity: w.relationship_proximity,
                    access_frequency: w.access_frequency,
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

    fn format_within_budget(&self, results: &[ScoredResult], budget: u64) -> (usize, String, u64) {
        let cpt = self.config.chars_per_token;
        let mut included = Vec::with_capacity(results.len());

        for result in results {
            included.push(result);
            let section = format_section(&included);
            let tokens = estimate_tokens(&section, cpt);
            if tokens > budget {
                included.pop();
                break;
            }
        }

        if included.is_empty() {
            return (0, String::new(), 0);
        }

        let section = format_section(&included);
        let tokens = estimate_tokens(&section, cpt);
        (included.len(), section, tokens)
    }
}

#[cfg(test)]
#[path = "../recall_tests.rs"]
mod tests;
