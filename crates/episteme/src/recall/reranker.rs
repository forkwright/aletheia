//! Optional reranker stage for the recall pipeline.
//!
//! Provides a [`Reranker`] trait and a lightweight [`NaiveReranker`]
//! implementation based on a simplified BM25 keyword-match score.
//!
//! Gated behind the `reranker` feature (enabled by default).

use async_trait::async_trait;
use snafu::Snafu;
use tracing::instrument;

use super::RecallCandidate;

/// Errors that can occur in the episteme recall pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
pub enum EpistemeError {
    /// Reranker operation failed.
    #[snafu(display("reranker failed: {message}"))]
    RerankerFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Trait for reranking recall candidates.
///
/// Implementations receive the top-K candidates from the baseline 6-factor
/// ranking and may return them in a refined order.
#[async_trait]
pub trait Reranker: Send + Sync {
    /// Reorder `candidates` for the given `query`.
    ///
    /// # Errors
    ///
    /// Returns [`EpistemeError::RerankerFailed`] when the reranker cannot
    /// complete its scoring (e.g. network timeout for a remote cross-encoder).
    async fn rerank(
        &self,
        query: &str,
        candidates: Vec<RecallCandidate>,
    ) -> Result<Vec<RecallCandidate>, EpistemeError>;

    /// Human-readable name for diagnostics and metrics.
    fn name(&self) -> &'static str;
}

/// A lightweight BM25-ish keyword-match reranker.
///
/// Tokenises the query and each candidate's content by whitespace, computes a
/// simplified BM25 score per candidate, and returns them sorted by that score.
/// No external dependencies or network calls are required.
#[derive(Debug, Clone, Copy)]
pub struct NaiveReranker;

#[async_trait]
impl Reranker for NaiveReranker {
    #[instrument(skip(self, candidates))]
    async fn rerank(
        &self,
        query: &str,
        mut candidates: Vec<RecallCandidate>,
    ) -> Result<Vec<RecallCandidate>, EpistemeError> {
        let query_tokens: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(String::from)
            .collect();

        if query_tokens.is_empty() || candidates.is_empty() {
            return Ok(candidates);
        }

        let total_len: usize = candidates
            .iter()
            .map(|c| c.content.split_whitespace().count())
            .sum();
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "usize→f64: token counts fit in f64"
        )]
        let avgdl = total_len as f64 / candidates.len().max(1) as f64;
        let k1 = 1.2;
        let b = 0.75;

        for candidate in &mut candidates {
            let doc_tokens: Vec<String> = candidate
                .content
                .to_lowercase()
                .split_whitespace()
                .map(String::from)
                .collect();
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "usize→f64: token counts fit in f64"
            )]
            let dl = doc_tokens.len() as f64;
            let mut score = 0.0;

            for qt in &query_tokens {
                #[expect(
                    clippy::cast_precision_loss,
                    clippy::as_conversions,
                    reason = "usize→f64: token counts fit in f64"
                )]
                let f = doc_tokens.iter().filter(|t| t == &qt).count() as f64;
                if f > 0.0 {
                    // Simplified IDF: assume every term appears in roughly half
                    // the documents.  This avoids needing corpus-level statistics.
                    let idf = 2.0f64.ln();
                    let denom = f + k1 * (1.0 - b + b * dl / avgdl.max(1.0));
                    score += idf * f * (k1 + 1.0) / denom;
                }
            }

            candidate.score = score;
        }

        let mut indexed: Vec<(usize, RecallCandidate)> =
            candidates.into_iter().enumerate().collect();
        indexed.sort_by(|(ia, a), (ib, b)| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| ia.cmp(ib))
        });

        Ok(indexed.into_iter().map(|(_, c)| c).collect())
    }

    fn name(&self) -> &'static str {
        "naive-bm25"
    }
}

#[cfg(test)]
mod tests {
    #![expect(clippy::expect_used, reason = "test assertions")]
    #![expect(
        clippy::indexing_slicing,
        reason = "recall reranker tests: bounded indexing on known-small vecs"
    )]

    use std::sync::Arc;

    use super::*;
    use crate::knowledge::FactSensitivity;
    use crate::recall::{FactorScores, RecallEngine, ScoredResult};

    fn make_candidate(content: &str, vector_similarity: f64) -> ScoredResult {
        ScoredResult {
            content: content.to_owned(),
            source_type: "fact".to_owned(),
            source_id: content.to_owned(),
            nous_id: "syn".to_owned(),
            factors: FactorScores {
                vector_similarity,
                ..FactorScores::default()
            },
            score: 0.0,
            sensitivity: FactSensitivity::Public,
        }
    }

    #[tokio::test]
    async fn naive_reranker_preserves_order_on_identical_scores() {
        let reranker = NaiveReranker;
        let candidates = vec![
            make_candidate("aaa bbb ccc", 0.5),
            make_candidate("ddd eee fff", 0.5),
            make_candidate("ggg hhh iii", 0.5),
        ];

        let result = reranker
            .rerank("xyz unrelated", candidates.clone())
            .await
            .expect("naive reranker should not fail");

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].source_id, candidates[0].source_id);
        assert_eq!(result[1].source_id, candidates[1].source_id);
        assert_eq!(result[2].source_id, candidates[2].source_id);
    }

    #[tokio::test]
    async fn naive_reranker_boosts_exact_keyword_match() {
        let reranker = NaiveReranker;
        let candidates = vec![
            make_candidate("foo bar baz", 0.9),
            make_candidate("query term exact match", 0.1),
        ];

        let result = reranker
            .rerank("query term", candidates)
            .await
            .expect("naive reranker should not fail");

        assert_eq!(
            result[0].source_id, "query term exact match",
            "candidate with exact keyword matches should be boosted to first place"
        );
    }

    #[tokio::test]
    async fn recall_pipeline_with_reranker_none_matches_baseline() {
        let engine = RecallEngine::new();
        let candidates = vec![
            make_candidate("alpha", 0.9),
            make_candidate("beta", 0.5),
            make_candidate("gamma", 0.1),
        ];

        let baseline = engine.rank(candidates.clone());
        let with_reranker = engine.rank_and_rerank("test", candidates).await;

        assert_eq!(baseline.len(), with_reranker.len());
        for (b, w) in baseline.iter().zip(with_reranker.iter()) {
            assert_eq!(b.source_id, w.source_id);
            assert!(
                (b.score - w.score).abs() < f64::EPSILON,
                "score should match baseline: {} vs {}",
                b.score,
                w.score
            );
        }
    }

    #[tokio::test]
    async fn recall_pipeline_with_reranker_naive_reorders_topk() {
        let engine = RecallEngine::new()
            .with_reranker(Some(Arc::new(NaiveReranker)))
            .with_reranker_top_k(20);

        let candidates = vec![
            make_candidate("foo bar baz", 0.9),
            make_candidate("query term exact match", 0.1),
        ];

        let baseline = engine.rank(candidates.clone());
        assert_eq!(
            baseline[0].source_id, "foo bar baz",
            "baseline should rank high-vector candidate first"
        );

        let reranked = engine.rank_and_rerank("query term", candidates).await;
        assert_eq!(
            reranked[0].source_id, "query term exact match",
            "naive reranker should reorder top-k based on keyword match"
        );
    }

    #[test]
    fn episteme_error_display_includes_message() {
        let err = EpistemeError::RerankerFailed {
            message: "test failure".to_owned(),
            location: snafu::location!(),
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("test failure"),
            "error display should contain message: {msg}"
        );
    }
}
