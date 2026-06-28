//! Optional reranker stage for the recall pipeline.
//!
//! Provides a [`Reranker`] trait and a lightweight [`NaiveReranker`]
//! implementation based on a simplified BM25 keyword-match score.
//!
//! Also provides an [`HttpReranker`] that forwards candidates to an external
//! HTTP endpoint for cross-encoder or model-based scoring.
//!
//! Gated behind the `reranker` feature (enabled by default).

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use snafu::Snafu;
use tracing::{Instrument, instrument};

use super::RecallCandidate;

/// Errors that can occur in the episteme recall pipeline.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[expect(
    missing_docs,
    reason = "snafu error variant fields are self-documenting via display format"
)]
#[non_exhaustive]
pub enum EpistemeError {
    /// Reranker operation failed.
    #[snafu(display("reranker failed: {message}"))]
    RerankerFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Future returned by object-safe reranker implementations.
pub type RerankFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<RecallCandidate>, EpistemeError>> + Send + 'a>>;

/// Trait for reranking recall candidates.
///
/// Implementations receive the top-K candidates from the baseline 11-factor
/// ranking and may return them in a refined order.
pub trait Reranker: Send + Sync {
    /// Reorder `candidates` for the given `query`.
    ///
    /// # Errors
    ///
    /// Returns [`EpistemeError::RerankerFailed`] when the reranker cannot
    /// complete its scoring (e.g. network timeout for a remote cross-encoder).
    fn rerank<'a>(&'a self, query: &'a str, candidates: Vec<RecallCandidate>) -> RerankFuture<'a>;

    /// Human-readable name for diagnostics and metrics.
    fn name(&self) -> &'static str;

    /// Optional call-site timeout override for this reranker.
    fn timeout(&self) -> Option<Duration> {
        None
    }
}

/// Timeout configuration for HTTP reranking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RerankerTimeouts {
    /// TCP connect timeout for the HTTP client.
    pub connect_timeout: Duration,
    /// End-to-end request timeout configured on the HTTP client.
    pub request_timeout: Duration,
    /// Upper bound applied by [`RecallEngine`](super::RecallEngine) around the reranker call.
    pub call_timeout: Duration,
}

impl RerankerTimeouts {
    /// Defaults requested for remote cross-encoder backends.
    pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
    /// Defaults requested for remote cross-encoder backends.
    pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
    /// Defaults requested for remote cross-encoder backends.
    pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(30);
}

impl Default for RerankerTimeouts {
    fn default() -> Self {
        Self {
            connect_timeout: Self::DEFAULT_CONNECT_TIMEOUT,
            request_timeout: Self::DEFAULT_REQUEST_TIMEOUT,
            call_timeout: Self::DEFAULT_CALL_TIMEOUT,
        }
    }
}

/// A lightweight BM25-ish keyword-match reranker.
///
/// Tokenises the query and each candidate's content by whitespace, computes a
/// simplified BM25 score per candidate, and returns them sorted by that score.
/// No external dependencies or network calls are required.
#[derive(Debug, Clone, Copy)]
pub struct NaiveReranker;

impl Reranker for NaiveReranker {
    #[instrument(skip(self, candidates))]
    fn rerank<'a>(
        &'a self,
        query: &'a str,
        mut candidates: Vec<RecallCandidate>,
    ) -> RerankFuture<'a> {
        Box::pin(
            async move {
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
            .instrument(tracing::Span::current()),
        )
    }

    fn name(&self) -> &'static str {
        "naive-bm25"
    }
}

/// A lightweight DTO for a recall candidate sent to the HTTP reranker.
#[derive(Debug, Serialize)]
struct RerankCandidate<'a> {
    content: &'a str,
    source_id: &'a str,
    source_type: &'a str,
    nous_id: &'a str,
    score: f64,
}

/// Request body sent to the HTTP reranker endpoint.
#[derive(Debug, Serialize)]
struct RerankRequest<'a> {
    query: &'a str,
    candidates: Vec<RerankCandidate<'a>>,
}

/// Response body from the HTTP reranker endpoint.
#[derive(Debug, Deserialize)]
struct RerankResponse {
    scores: Vec<f64>,
}

/// An HTTP-based reranker that delegates scoring to a remote endpoint.
///
/// POSTs the query and candidate list as JSON and expects a JSON response
/// containing a parallel `scores` array.  On any HTTP or parse error the
/// trait method returns [`EpistemeError::RerankerFailed`]; callers such as
/// [`RecallEngine::rank_and_rerank`] fall back to the baseline ranking.
#[derive(Debug, Clone)]
pub struct HttpReranker {
    client: reqwest::Client,
    url: String,
    timeouts: RerankerTimeouts,
}

impl HttpReranker {
    /// Create a new HTTP reranker pointing at `url`.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self::with_timeouts(url, RerankerTimeouts::default())
    }

    /// Create a new HTTP reranker with explicit timeout configuration.
    #[must_use]
    pub fn with_timeouts(url: impl Into<String>, timeouts: RerankerTimeouts) -> Self {
        // WHY: workspace reqwest uses rustls-no-provider, so callers must install
        // the crypto provider before constructing an HTTP client.
        if let Err(err) = rustls::crypto::ring::default_provider().install_default() {
            tracing::trace!(?err, "rustls crypto provider already installed");
        }

        // WHY: without a timeout, a slow or hung cross-encoder backend stalls
        // the recall actor unboundedly; match the pattern in openai.rs.
        let client = reqwest::ClientBuilder::new()
            .connect_timeout(timeouts.connect_timeout)
            .timeout(timeouts.request_timeout)
            .build()
            .unwrap_or_default();

        Self {
            client,
            url: url.into(),
            timeouts,
        }
    }

    /// Return the timeout values used to build this reranker.
    #[must_use]
    pub fn timeouts(&self) -> RerankerTimeouts {
        self.timeouts
    }
}

impl Reranker for HttpReranker {
    #[instrument(skip(self, candidates))]
    fn rerank<'a>(
        &'a self,
        query: &'a str,
        mut candidates: Vec<RecallCandidate>,
    ) -> RerankFuture<'a> {
        Box::pin(
            async move {
                if candidates.is_empty() {
                    return Ok(candidates);
                }

                let req_body = RerankRequest {
                    query,
                    candidates: candidates
                        .iter()
                        .map(|c| RerankCandidate {
                            content: &c.content,
                            source_id: &c.source_id,
                            source_type: &c.source_type,
                            nous_id: &c.nous_id,
                            score: c.score,
                        })
                        .collect(),
                };

                let response = self
                    .client
                    .post(&self.url)
                    .json(&req_body)
                    .send()
                    .await
                    .map_err(|e| {
                        RerankerFailedSnafu {
                            message: format!("HTTP request failed: {e}"),
                        }
                        .build()
                    })?;

                let status = response.status();
                if !status.is_success() {
                    let body = match response.text().await {
                        Ok(body) => body,
                        Err(err) => format!("<failed to read error body: {err}>"),
                    };
                    return Err(RerankerFailedSnafu {
                        message: format!("HTTP {status}: {body}"),
                    }
                    .build());
                }

                let resp_body: RerankResponse = response.json().await.map_err(|e| {
                    RerankerFailedSnafu {
                        message: format!("failed to parse reranker response: {e}"),
                    }
                    .build()
                })?;

                if resp_body.scores.len() != candidates.len() {
                    return Err(RerankerFailedSnafu {
                        message: format!(
                            "score count mismatch: expected {}, got {}",
                            candidates.len(),
                            resp_body.scores.len()
                        ),
                    }
                    .build());
                }

                for (candidate, score) in candidates.iter_mut().zip(resp_body.scores) {
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
            .instrument(tracing::Span::current()),
        )
    }

    fn name(&self) -> &'static str {
        "http-reranker"
    }

    fn timeout(&self) -> Option<Duration> {
        Some(self.timeouts.call_timeout)
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

    struct PendingReranker;

    impl Reranker for PendingReranker {
        fn rerank<'a>(
            &'a self,
            _query: &'a str,
            _candidates: Vec<RecallCandidate>,
        ) -> RerankFuture<'a> {
            Box::pin(std::future::pending::<
                Result<Vec<RecallCandidate>, EpistemeError>,
            >())
        }

        fn name(&self) -> &'static str {
            "pending-reranker"
        }
    }

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
            visibility: crate::knowledge::Visibility::Private,
            scope: None,
            project_id: None,
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

    #[tokio::test]
    async fn http_reranker_reorders_by_remote_scores() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "scores": [0.1, 0.9]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let reranker = HttpReranker::new(format!("{}/rerank", server.uri()));
        let candidates = vec![
            make_candidate("foo bar baz", 0.5),
            make_candidate("query term exact match", 0.5),
        ];

        let result = reranker
            .rerank("query term", candidates)
            .await
            .expect("HTTP reranker should succeed");

        assert_eq!(result.len(), 2);
        assert_eq!(
            result[0].source_id, "query term exact match",
            "higher remote score should be first"
        );
        assert_eq!(result[1].source_id, "foo bar baz");
    }

    #[tokio::test]
    async fn http_reranker_preserves_order_on_equal_scores() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rerank"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "scores": [0.5, 0.5, 0.5]
            })))
            .mount(&server)
            .await;

        let reranker = HttpReranker::new(format!("{}/rerank", server.uri()));
        let candidates = vec![
            make_candidate("aaa bbb ccc", 0.5),
            make_candidate("ddd eee fff", 0.5),
            make_candidate("ggg hhh iii", 0.5),
        ];

        let result = reranker
            .rerank("xyz unrelated", candidates.clone())
            .await
            .expect("HTTP reranker should not fail");

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].source_id, candidates[0].source_id);
        assert_eq!(result[1].source_id, candidates[1].source_id);
        assert_eq!(result[2].source_id, candidates[2].source_id);
    }

    #[tokio::test]
    async fn http_reranker_returns_err_on_http_error() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let reranker = HttpReranker::new(server.uri());
        let candidates = vec![make_candidate("foo bar baz", 0.5)];

        let result = reranker.rerank("query", candidates).await;
        assert!(
            matches!(result, Err(EpistemeError::RerankerFailed { .. })),
            "HTTP error should yield RerankerFailed"
        );
    }

    #[tokio::test]
    async fn http_reranker_returns_err_on_score_count_mismatch() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "scores": [0.5]
            })))
            .mount(&server)
            .await;

        let reranker = HttpReranker::new(server.uri());
        let candidates = vec![
            make_candidate("foo bar baz", 0.5),
            make_candidate("query term exact match", 0.5),
        ];

        let result = reranker.rerank("query", candidates).await;
        assert!(
            matches!(result, Err(EpistemeError::RerankerFailed { .. })),
            "score count mismatch should yield RerankerFailed"
        );
    }

    #[tokio::test]
    async fn http_reranker_empty_candidates_short_circuits() {
        let reranker = HttpReranker::new("http://localhost:9999/rerank");
        let result = reranker
            .rerank("query", vec![])
            .await
            .expect("empty candidates should short-circuit without network call");
        assert!(result.is_empty());
    }

    #[test]
    fn http_reranker_uses_configurable_timeouts() {
        let timeouts = RerankerTimeouts {
            connect_timeout: std::time::Duration::from_millis(250),
            request_timeout: std::time::Duration::from_millis(500),
            call_timeout: std::time::Duration::from_millis(750),
        };
        let reranker = HttpReranker::with_timeouts("http://localhost:9999/rerank", timeouts);

        assert_eq!(reranker.timeouts(), timeouts);
    }

    #[tokio::test]
    async fn recall_pipeline_with_reranker_http_falls_back_on_error() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let engine = RecallEngine::new()
            .with_reranker(Some(Arc::new(HttpReranker::new(server.uri()))))
            .with_reranker_top_k(20);

        let candidates = vec![
            make_candidate("foo bar baz", 0.9),
            make_candidate("query term exact match", 0.1),
        ];

        let baseline = engine.rank(candidates.clone());
        let reranked = engine.rank_and_rerank("query term", candidates).await;

        assert_eq!(baseline.len(), reranked.len());
        for (b, r) in baseline.iter().zip(reranked.iter()) {
            assert_eq!(b.source_id, r.source_id);
        }
    }

    #[tokio::test]
    async fn recall_pipeline_reranker_timeout_falls_back_to_baseline() {
        let engine = RecallEngine::new()
            .with_reranker(Some(Arc::new(PendingReranker)))
            .with_reranker_timeout(std::time::Duration::from_millis(10))
            .with_reranker_top_k(20);

        let candidates = vec![
            make_candidate("foo bar baz", 0.9),
            make_candidate("query term exact match", 0.1),
        ];

        let baseline = engine.rank(candidates.clone());
        let reranked = engine.rank_and_rerank("query term", candidates).await;

        assert_eq!(baseline.len(), reranked.len());
        for (b, r) in baseline.iter().zip(reranked.iter()) {
            assert_eq!(b.source_id, r.source_id);
        }
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
