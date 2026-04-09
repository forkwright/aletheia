//! Embedding evaluation gate: Recall@K and MRR metrics.
//!
//! Runs a labelled query set through an [`EmbeddingProvider`] and checks
//! retrieval quality against ground-truth result IDs. Designed to run
//! before every model upgrade so regressions are caught automatically.
//!
//! # Metrics
//!
//! - **Recall@K** — fraction of queries where at least one ground-truth ID
//!   appears in the top-K results.
//! - **MRR** (Mean Reciprocal Rank) — mean of 1/rank across queries, where
//!   rank is the 1-indexed position of the first hit. 0.0 if no hit.
//!
//! # Usage
//!
//! ```ignore
//! // WHY: MockEmbeddingProvider is gated behind the `test-support` feature.
//! // This example shows the API but cannot compile in doctest mode.
//! use aletheia_episteme::embedding::MockEmbeddingProvider;
//! use aletheia_episteme::embedding_eval::{EvalDataset, evaluate_model};
//!
//! let dataset = EvalDataset::from_jsonl_str(r#"{"query":"foo","relevant_ids":["a"]}"#)
//!     .expect("JSONL must parse for valid test data");
//! let provider = MockEmbeddingProvider::new(384);
//! let corpus: Vec<(String, String)> = vec![("a".into(), "foo bar".into())];
//! let result = evaluate_model(&provider, &dataset, &corpus, 5).unwrap();
//! println!("Recall@5: {}", result.recall_at_k);
//! ```

use snafu::Snafu;
use tracing::instrument;

use crate::embedding::EmbeddingProvider;

// ── Error ─────────────────────────────────────────────────────────────────────

/// Errors produced by the embedding evaluation pipeline.
#[derive(Debug, Snafu)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (message, location) are self-documenting via display format"
)]
pub enum EvalError {
    /// A JSONL line could not be parsed as an [`EvalQuery`].
    #[snafu(display("failed to parse eval dataset line {line}: {message}"))]
    ParseFailed {
        line: usize,
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The corpus is empty — nothing to rank against.
    #[snafu(display("eval corpus is empty: provide at least one (id, text) pair"))]
    EmptyCorpus {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// The dataset contains no queries.
    #[snafu(display("eval dataset is empty: provide at least one query"))]
    EmptyDataset {
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Embedding a text failed.
    #[snafu(display("embedding failed during eval: {message}"))]
    EmbedFailed {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Result type for eval operations.
pub type EvalResult<T> = std::result::Result<T, EvalError>;

// ── Dataset ───────────────────────────────────────────────────────────────────

/// A single labelled evaluation query.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvalQuery {
    /// The natural-language query text.
    pub query: String,
    /// Ground-truth corpus IDs that should rank in the top K for this query.
    pub relevant_ids: Vec<String>,
    /// Optional human-readable description (ignored during evaluation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A set of labelled queries for embedding evaluation.
///
/// Loaded from JSONL — one [`EvalQuery`] JSON object per line.
#[derive(Debug, Clone)]
pub struct EvalDataset {
    /// The labelled queries.
    pub queries: Vec<EvalQuery>,
}

impl EvalDataset {
    /// Parse a JSONL string into an [`EvalDataset`].
    ///
    /// Blank lines are skipped. Returns an error on the first malformed line.
    ///
    /// # Errors
    ///
    /// Returns [`EvalError::ParseFailed`] if any non-blank line is invalid JSON
    /// or missing required fields.
    pub fn from_jsonl_str(s: &str) -> EvalResult<Self> {
        let mut queries = Vec::new();
        for (idx, raw) in s.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let q: EvalQuery = serde_json::from_str(line).map_err(|e| {
                ParseFailedSnafu {
                    line: idx + 1,
                    message: e.to_string(),
                }
                .build()
            })?;
            queries.push(q);
        }
        Ok(Self { queries })
    }

    /// Load a JSONL file from disk into an [`EvalDataset`].
    ///
    /// # Errors
    ///
    /// Returns [`EvalError::ParseFailed`] if reading or parsing fails.
    pub fn from_jsonl_file(path: &std::path::Path) -> EvalResult<Self> {
        let contents = std::fs::read_to_string(path).map_err(|e| {
            ParseFailedSnafu {
                line: 0_usize,
                message: format!("cannot read {}: {e}", path.display()),
            }
            .build()
        })?;
        Self::from_jsonl_str(&contents)
    }

    /// Number of queries in this dataset.
    #[must_use]
    pub fn len(&self) -> usize {
        self.queries.len()
    }

    /// `true` if the dataset contains no queries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queries.is_empty()
    }
}

// ── Per-query result ──────────────────────────────────────────────────────────

/// Per-query evaluation outcome.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QueryResult {
    /// The query text.
    pub query: String,
    /// Whether any ground-truth ID appeared in the top-K results.
    pub hit: bool,
    /// 1/rank of the first hit, or 0.0 if no hit found.
    pub reciprocal_rank: f64,
    /// IDs returned in top-K order by the model.
    pub top_k_ids: Vec<String>,
}

// ── Aggregate result ──────────────────────────────────────────────────────────

/// Aggregate evaluation metrics for one model.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelMetrics {
    /// The embedding model name as reported by the provider.
    pub model_name: String,
    /// K used during this evaluation run.
    pub k: usize,
    /// Recall@K: fraction of queries with at least one ground-truth hit in top K.
    pub recall_at_k: f64,
    /// Recall@5 (re-computed at K=5 regardless of the run K, or the run K if K<5).
    pub recall_at_5: f64,
    /// Recall@10 (re-computed at K=10 regardless of the run K, or the run K if K<10).
    pub recall_at_10: f64,
    /// Mean Reciprocal Rank across all queries.
    pub mrr: f64,
    /// Per-query detail.
    pub per_query: Vec<QueryResult>,
}

/// Result of a full evaluation run, optionally including a candidate model
/// evaluated side-by-side against a baseline.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvalRunResult {
    /// Metrics for the baseline (current) model.
    pub baseline: ModelMetrics,
    /// Metrics for the candidate model, if one was evaluated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate: Option<ModelMetrics>,
    /// `true` if the candidate is at least as good as baseline (or no candidate).
    pub passed: bool,
}

// ── Core evaluation ───────────────────────────────────────────────────────────

/// Evaluate one embedding provider against a labelled dataset.
///
/// # Arguments
///
/// * `provider` — the model under test.
/// * `dataset`  — labelled queries.
/// * `corpus`   — `(id, text)` pairs representing the retrieval pool.
/// * `k`        — how many top results to retrieve per query.
///
/// # Algorithm
///
/// 1. Embed every corpus text once (batch).
/// 2. For each query, embed the query text, compute cosine similarities
///    against all corpus vectors, take the top-K IDs.
/// 3. Hit = any ground-truth ID is in the top-K.
/// 4. Recall@K = hits / queries. MRR = mean(1/rank) across queries.
///
/// # Errors
///
/// Returns [`EvalError::EmptyCorpus`] or [`EvalError::EmptyDataset`] for
/// empty inputs, and [`EvalError::EmbedFailed`] if the provider errors.
#[instrument(skip(provider, dataset, corpus), fields(k = k, queries = dataset.queries.len(), corpus = corpus.len()))]
pub fn evaluate_model(
    provider: &dyn EmbeddingProvider,
    dataset: &EvalDataset,
    corpus: &[(String, String)],
    k: usize,
) -> EvalResult<ModelMetrics> {
    if corpus.is_empty() {
        return EmptyCorpusSnafu.fail();
    }
    if dataset.is_empty() {
        return EmptyDatasetSnafu.fail();
    }

    // Embed corpus in one batch.
    let corpus_texts: Vec<&str> = corpus.iter().map(|(_, t)| t.as_str()).collect();
    let corpus_vecs = provider.embed_batch(&corpus_texts).map_err(|e| {
        EmbedFailedSnafu {
            message: e.to_string(),
        }
        .build()
    })?;

    // Effective K capped at corpus size.
    let eff_k = k.min(corpus.len());
    let eff_k5 = 5_usize.min(corpus.len());
    let eff_k10 = 10_usize.min(corpus.len());

    let mut per_query: Vec<QueryResult> = Vec::with_capacity(dataset.queries.len());
    let mut hit_count_k = 0usize;
    let mut hit_count_5 = 0usize;
    let mut hit_count_10 = 0usize;
    let mut rr_sum = 0.0_f64;

    for eq in &dataset.queries {
        let outcome = score_one_query(
            provider,
            eq,
            corpus,
            &corpus_vecs,
            [eff_k, eff_k5, eff_k10],
        )?;
        if outcome.hit_k {
            hit_count_k += 1;
        }
        if outcome.hit_5 {
            hit_count_5 += 1;
        }
        if outcome.hit_10 {
            hit_count_10 += 1;
        }
        rr_sum += outcome.reciprocal_rank;
        per_query.push(QueryResult {
            query: eq.query.clone(),
            hit: outcome.hit_k,
            reciprocal_rank: outcome.reciprocal_rank,
            top_k_ids: outcome.top_k_ids,
        });
    }

    let n = dataset.queries.len();
    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "n and the hit counts are small dataset sizes that fit exactly in f64"
    )]
    let (queries_f, hits_at_k, hits_at_five, hits_at_ten) = (
        n as f64,
        hit_count_k as f64,
        hit_count_5 as f64,
        hit_count_10 as f64,
    );

    Ok(ModelMetrics {
        model_name: provider.model_name().to_owned(),
        k: eff_k,
        recall_at_k: hits_at_k / queries_f,
        recall_at_5: hits_at_five / queries_f,
        recall_at_10: hits_at_ten / queries_f,
        mrr: rr_sum / queries_f,
        per_query,
    })
}

/// Per-query outcome of [`score_one_query`].
struct QueryOutcome {
    hit_k: bool,
    hit_5: bool,
    hit_10: bool,
    reciprocal_rank: f64,
    top_k_ids: Vec<String>,
}

/// Score one evaluation query against the embedded corpus.
///
/// Embeds the query, ranks the corpus by cosine similarity, then computes
/// hit indicators for K, 5, 10 and the reciprocal rank.
///
/// `cutoffs` is `[eff_k, eff_k_5, eff_k_10]` — the effective top-K cutoffs
/// already capped at corpus length.
fn score_one_query(
    provider: &dyn EmbeddingProvider,
    eq: &EvalQuery,
    corpus: &[(String, String)],
    corpus_vecs: &[Vec<f32>],
    cutoffs: [usize; 3],
) -> EvalResult<QueryOutcome> {
    let [eff_k, eff_k5, eff_k10] = cutoffs;

    let q_vec = provider.embed(&eq.query).map_err(|e| {
        EmbedFailedSnafu {
            message: format!("query {:?}: {e}", eq.query),
        }
        .build()
    })?;

    // Rank corpus by cosine similarity (dot product of L2-normalized vectors).
    let mut ranked: Vec<(usize, f32)> = corpus_vecs
        .iter()
        .enumerate()
        .map(|(i, cv)| (i, cosine_similarity(&q_vec, cv)))
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // WHY: ranked is collected from enumerate over corpus_vecs, so each (i, _)
    // pair has `i < corpus.len()`. Use `.get()` to avoid the indexing/slicing
    // lints; missing entries (impossible by construction) are silently skipped
    // via filter_map rather than panicking.
    let top_k_ids: Vec<String> = ranked
        .iter()
        .take(eff_k)
        .filter_map(|(i, _)| corpus.get(*i).map(|(id, _)| id.clone()))
        .collect();

    let relevant: std::collections::HashSet<&str> =
        eq.relevant_ids.iter().map(String::as_str).collect();

    let hit_k = top_k_ids.iter().any(|id| relevant.contains(id.as_str()));

    let hit_5 = ranked
        .iter()
        .take(eff_k5)
        .filter_map(|(i, _)| corpus.get(*i).map(|(id, _)| id.as_str()))
        .any(|id| relevant.contains(id));

    let hit_10 = ranked
        .iter()
        .take(eff_k10)
        .filter_map(|(i, _)| corpus.get(*i).map(|(id, _)| id.as_str()))
        .any(|id| relevant.contains(id));

    // MRR: first hit position across top-K (1-indexed).
    let first_hit_rank = top_k_ids
        .iter()
        .position(|id| relevant.contains(id.as_str()))
        .map(|pos| pos + 1);
    let reciprocal_rank = first_hit_rank.map_or(0.0, |r| {
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "rank fits in f64 exactly for any realistic K"
        )]
        let rf = r as f64;
        1.0 / rf
    });

    Ok(QueryOutcome {
        hit_k,
        hit_5,
        hit_10,
        reciprocal_rank,
        top_k_ids,
    })
}

/// Evaluate baseline and optional candidate providers side-by-side.
///
/// Returns [`EvalRunResult::passed = false`] when a candidate is present and
/// its Recall@K is strictly lower than the baseline (tolerance = 0.0).
///
/// # Errors
///
/// Propagates errors from [`evaluate_model`].
#[instrument(skip(baseline, candidate, dataset, corpus), fields(k = k))]
pub fn compare_models(
    baseline: &dyn EmbeddingProvider,
    candidate: Option<&dyn EmbeddingProvider>,
    dataset: &EvalDataset,
    corpus: &[(String, String)],
    k: usize,
) -> EvalResult<EvalRunResult> {
    let baseline_metrics = evaluate_model(baseline, dataset, corpus, k)?;

    let (candidate_metrics, passed) = if let Some(cand) = candidate {
        let cm = evaluate_model(cand, dataset, corpus, k)?;
        // Candidate passes when it does not regress Recall@K.
        let ok = cm.recall_at_k >= baseline_metrics.recall_at_k;
        (Some(cm), ok)
    } else {
        (None, true)
    };

    Ok(EvalRunResult {
        baseline: baseline_metrics,
        candidate: candidate_metrics,
        passed,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Cosine similarity between two L2-normalized f32 vectors (dot product).
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::MockEmbeddingProvider;

    fn mock() -> MockEmbeddingProvider {
        MockEmbeddingProvider::new(64)
    }

    fn simple_corpus() -> Vec<(String, String)> {
        vec![
            ("id-alice".into(), "alice prefers tea over coffee".into()),
            ("id-bob".into(), "bob enjoys cycling on weekends".into()),
            ("id-carol".into(), "carol studies distributed systems".into()),
        ]
    }

    fn simple_dataset() -> EvalDataset {
        EvalDataset::from_jsonl_str(
            r#"{"query":"alice tea coffee","relevant_ids":["id-alice"]}
{"query":"distributed systems","relevant_ids":["id-carol"]}
"#,
        )
        .expect("simple dataset must parse")
    }

    #[test]
    fn parse_valid_jsonl() {
        let ds = simple_dataset();
        assert_eq!(ds.len(), 2, "parse valid jsonl: values should be equal");
        assert_eq!(
            ds.queries[0].query, "alice tea coffee",
            "parse valid jsonl: values should be equal"
        );
        assert_eq!(
            ds.queries[0].relevant_ids,
            ["id-alice"],
            "parse valid jsonl: values should be equal"
        );
    }

    #[test]
    fn parse_skips_blank_lines() {
        let ds = EvalDataset::from_jsonl_str(
            "\n{\"query\":\"x\",\"relevant_ids\":[\"y\"]}\n\n",
        )
        .expect("blank lines should be skipped");
        assert_eq!(ds.len(), 1, "parse skips blank lines: values should be equal");
    }

    #[test]
    fn parse_bad_json_returns_error() {
        let result = EvalDataset::from_jsonl_str("not json at all");
        assert!(result.is_err(), "bad json should return error");
    }

    #[test]
    fn evaluate_returns_recall_and_mrr() {
        let provider = mock();
        let corpus = simple_corpus();
        let dataset = simple_dataset();
        let metrics = evaluate_model(&provider, &dataset, &corpus, 3)
            .expect("evaluate_model must succeed on valid inputs");
        // recall_at_k is in [0, 1]
        assert!(
            metrics.recall_at_k >= 0.0 && metrics.recall_at_k <= 1.0,
            "recall must be in [0,1]"
        );
        assert!(
            metrics.mrr >= 0.0 && metrics.mrr <= 1.0,
            "mrr must be in [0,1]"
        );
        assert_eq!(
            metrics.per_query.len(),
            2,
            "per_query count must match dataset size"
        );
    }

    #[test]
    fn evaluate_empty_corpus_errors() {
        let provider = mock();
        let dataset = simple_dataset();
        let err = evaluate_model(&provider, &dataset, &[], 5)
            .expect_err("empty corpus must error");
        assert!(
            matches!(err, EvalError::EmptyCorpus { .. }),
            "expected EmptyCorpus, got {err:?}"
        );
    }

    #[test]
    fn evaluate_empty_dataset_errors() {
        let provider = mock();
        let corpus = simple_corpus();
        let dataset = EvalDataset { queries: vec![] };
        let err = evaluate_model(&provider, &dataset, &corpus, 5)
            .expect_err("empty dataset must error");
        assert!(
            matches!(err, EvalError::EmptyDataset { .. }),
            "expected EmptyDataset, got {err:?}"
        );
    }

    #[test]
    fn compare_no_candidate_passes() {
        let provider = mock();
        let corpus = simple_corpus();
        let dataset = simple_dataset();
        let run = compare_models(&provider, None, &dataset, &corpus, 3)
            .expect("compare_models with no candidate must succeed");
        assert!(run.passed, "no candidate always passes");
        assert!(
            run.candidate.is_none(),
            "no candidate means None in result"
        );
    }

    #[test]
    fn compare_same_model_passes() {
        let a = mock();
        let b = mock();
        let corpus = simple_corpus();
        let dataset = simple_dataset();
        let run = compare_models(&a, Some(&b), &dataset, &corpus, 3)
            .expect("compare_models same model must succeed");
        // Same model: candidate recall == baseline recall, so it passes.
        assert!(run.passed, "identical models must pass");
        assert!(run.candidate.is_some(), "candidate metrics must be present");
    }

    #[test]
    fn query_result_fields_populated() {
        let provider = mock();
        let corpus = simple_corpus();
        let dataset = simple_dataset();
        let metrics = evaluate_model(&provider, &dataset, &corpus, 2)
            .expect("evaluate_model must succeed");
        for qr in &metrics.per_query {
            assert!(!qr.query.is_empty(), "query must not be empty");
            assert!(
                qr.top_k_ids.len() <= 2,
                "top_k_ids capped at k=2"
            );
            assert!(
                qr.reciprocal_rank >= 0.0 && qr.reciprocal_rank <= 1.0,
                "rr must be in [0,1]"
            );
        }
    }

    #[test]
    fn k_larger_than_corpus_clamps_to_corpus_size() {
        let provider = mock();
        let corpus = simple_corpus(); // 3 items
        let dataset = simple_dataset();
        let metrics = evaluate_model(&provider, &dataset, &corpus, 100)
            .expect("evaluate_model with large k must succeed");
        // Effective k = min(100, 3) = 3
        assert_eq!(metrics.k, 3, "k must be clamped to corpus size");
        for qr in &metrics.per_query {
            assert!(
                qr.top_k_ids.len() <= 3,
                "top_k_ids cannot exceed corpus size"
            );
        }
    }

    #[test]
    fn cosine_similarity_unit_vectors() {
        let a = vec![1.0_f32, 0.0];
        let b = vec![0.0_f32, 1.0];
        let sim = cosine_similarity(&a, &b);
        assert!((sim - 0.0).abs() < 1e-6, "orthogonal unit vectors => similarity 0");

        let c = vec![1.0_f32, 0.0];
        let same = cosine_similarity(&a, &c);
        assert!((same - 1.0).abs() < 1e-6, "identical unit vectors => similarity 1");
    }

    #[test]
    fn parse_with_optional_description() {
        let ds = EvalDataset::from_jsonl_str(
            r#"{"query":"foo","relevant_ids":["bar"],"description":"a test query"}"#,
        )
        .expect("parse with optional description must succeed");
        assert_eq!(
            ds.queries[0].description.as_deref(),
            Some("a test query"),
            "description must round-trip"
        );
    }
}
