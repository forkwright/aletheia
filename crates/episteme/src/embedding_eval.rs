//! Embedding evaluation: Recall@K and MRR metrics.
//!
//! Runs a labelled query set through an [`EmbeddingProvider`] and checks
//! retrieval quality against ground-truth result IDs. Baseline measurement and
//! regression gating are separate modes so automation cannot confuse a
//! baseline-only run with a passed model-upgrade gate.
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
//! use std::path::Path;
//! use episteme::embedding::MockEmbeddingProvider;
//! use episteme::embedding_eval::{EvalDataset, measure_baseline};
//!
//! let dataset = EvalDataset::from_jsonl_file(Path::new("eval.jsonl"))
//!     .expect("JSONL must parse for valid test data");
//! let provider = MockEmbeddingProvider::new(384);
//! let corpus: Vec<(String, String)> = vec![("a".into(), "foo bar".into())];
//! let run = measure_baseline(&provider, &dataset, &corpus, 5).unwrap();
//! println!("Recall@5: {}", run.baseline.recall_at_k);
//! ```

use snafu::{ResultExt, Snafu};
use tracing::instrument;

use std::collections::HashMap;

use crate::embedding::{EmbeddingProvider, ModelProvenance};

// ── Error ─────────────────────────────────────────────────────────────────────

/// Errors produced by the embedding evaluation pipeline.
#[derive(Debug, Snafu)]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (message, location) are self-documenting via display format"
)]
// kanon:ignore RUST/non-exhaustive-enum — already #[non_exhaustive] above (linter only inspects the attribute immediately preceding the enum; known false positive when another attribute intervenes).
// kanon:ignore RUST/pub-visibility — EvalError is the public error type for the embedding eval API; callers of measure_baseline and compare_models (in aletheia and mneme) need to inspect error variants
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

    /// A query references corpus IDs that do not exist in the evaluated corpus.
    #[snafu(display(
        "eval dataset validation failed for query {query:?} (line {line:?}): unknown relevant id {id:?}"
    ))]
    UnknownRelevantId {
        query: String,
        line: Option<usize>,
        id: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// A query lists the same relevant ID more than once.
    #[snafu(display(
        "eval dataset validation failed for query {query:?} (line {line:?}): duplicate relevant id {id:?}"
    ))]
    DuplicateRelevantId {
        query: String,
        line: Option<usize>,
        id: String,
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

    /// Reading the JSONL dataset from disk failed.
    #[snafu(display("cannot read eval dataset {}: {source}", path.display()))]
    IoFailed {
        path: std::path::PathBuf,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Result type for eval operations.
// kanon:ignore RUST/pub-visibility — public return type alias for the embedding eval API; paired with the public EvalError type consumed by aletheia and mneme callers
pub type EvalResult<T> = std::result::Result<T, EvalError>;

// ── Dataset ───────────────────────────────────────────────────────────────────

/// Proof type returned by [`EvalDataset::validate_against_corpus`].
///
/// Only constructable via successful validation; callers are forced to validate
/// before an evaluation can proceed.
#[derive(Debug)]
pub(crate) struct CorpusValidated(());

impl CorpusValidated {
    fn new() -> Self {
        Self(())
    }
}

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
    /// Optional slice label for slice-specific quality floors.
    ///
    /// WHY: model upgrades can regress on specific query cohorts (long-tail,
    /// non-English, code, etc.); slice labels let gates enforce per-cohort
    /// Recall@K floors in addition to aggregate metrics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slice: Option<String>,
    /// Source line number in the JSONL file (1-indexed).
    ///
    /// WHY: recorded at load time so validation errors can point operators at
    /// the exact line containing a stale or typo’d `relevant_ids` entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_line: Option<usize>,
}

/// A set of labelled queries for embedding evaluation.
///
/// Loaded from JSONL — one [`EvalQuery`] JSON object per line.
#[derive(Debug, Clone)]
pub struct EvalDataset {
    /// The labelled queries.
    pub queries: Vec<EvalQuery>,
    /// When `true`, unknown or duplicate `relevant_ids` are reported as warnings
    /// instead of failing the evaluation.
    ///
    /// WHY: fail-closed is the default for gates; permissive mode is only for
    /// exploratory dataset inspection.
    pub(crate) permissive: bool,
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
    pub(crate) fn from_jsonl_str(s: &str) -> EvalResult<Self> {
        let mut queries = Vec::new();
        for (idx, raw) in s.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let mut q: EvalQuery = serde_json::from_str(line).map_err(|e| {
                ParseFailedSnafu {
                    line: idx + 1,
                    message: e.to_string(),
                }
                .build()
            })?;
            // WHY: line numbers are metadata for validation failures; they are
            // not part of the persisted query schema.
            q.source_line = Some(idx + 1);
            queries.push(q);
        }
        Ok(Self {
            queries,
            permissive: false,
        })
    }

    /// Set whether unknown/duplicate `relevant_ids` should be treated as warnings
    /// rather than hard failures.
    #[must_use]
    pub fn permissive(mut self, value: bool) -> Self {
        self.permissive = value;
        self
    }

    /// Load a JSONL file from disk into an [`EvalDataset`].
    ///
    /// # Errors
    ///
    /// Returns [`EvalError::IoFailed`] if the file cannot be read, or
    /// [`EvalError::ParseFailed`] if any non-blank line is malformed.
    pub fn from_jsonl_file(path: &std::path::Path) -> EvalResult<Self> {
        let contents = std::fs::read_to_string(path).context(IoFailedSnafu {
            path: path.to_path_buf(),
        })?;
        Self::from_jsonl_str(&contents)
    }

    /// Number of queries in this dataset.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "symmetry with is_empty; exercised from tests")
    )]
    pub(crate) fn len(&self) -> usize {
        self.queries.len()
    }

    /// `true` if the dataset contains no queries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.queries.is_empty()
    }

    /// Verify that every `relevant_ids` entry exists in the corpus and that no
    /// ID is repeated inside a single query.
    ///
    /// Returns [`CorpusValidated`] on success — a proof type that the dataset
    /// is consistent with the given corpus.
    ///
    /// # Errors
    ///
    /// Returns [`EvalError::UnknownRelevantId`] or
    /// [`EvalError::DuplicateRelevantId`] when the dataset is inconsistent with
    /// the corpus. Permissive datasets skip this check.
    pub(crate) fn validate_against_corpus(
        &self,
        corpus: &[(String, String)],
    ) -> EvalResult<CorpusValidated> {
        if self.permissive {
            return Ok(CorpusValidated::new());
        }

        let corpus_ids: std::collections::HashSet<&str> =
            corpus.iter().map(|(id, _)| id.as_str()).collect();

        for q in &self.queries {
            let mut seen = std::collections::HashSet::new();
            for id in &q.relevant_ids {
                if !corpus_ids.contains(id.as_str()) {
                    return UnknownRelevantIdSnafu {
                        query: q.query.clone(),
                        line: q.source_line,
                        id: id.clone(),
                    }
                    .fail();
                }
                if !seen.insert(id.as_str()) {
                    return DuplicateRelevantIdSnafu {
                        query: q.query.clone(),
                        line: q.source_line,
                        id: id.clone(),
                    }
                    .fail();
                }
            }
        }

        Ok(CorpusValidated::new())
    }
}

// ── Per-query result ──────────────────────────────────────────────────────────

/// Per-query evaluation outcome.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct QueryResult {
    /// The query text.
    pub query: String,
    /// Optional slice label carried over from the input query.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slice: Option<String>,
    /// Whether any ground-truth ID appeared in the top-K results.
    pub hit: bool,
    /// 1/rank of the first hit, or 0.0 if no hit found.
    pub reciprocal_rank: f64,
    /// IDs returned in top-K order by the model.
    pub top_k_ids: Vec<String>,
}

// ── Aggregate result ──────────────────────────────────────────────────────────

/// Minimum allowed candidate Recall@K delta relative to baseline.
pub(crate) const DEFAULT_MIN_RECALL_AT_K_DELTA: f64 = 0.0;

/// Slice-specific floor for Recall@K.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct SliceFloor {
    /// Slice label that must match [`EvalQuery::slice`].
    pub slice: String,
    /// Minimum candidate Recall@K for this slice.
    pub min_recall_at_k: f64,
}

/// Operational mode for an embedding evaluation run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EvalRunMode {
    /// Baseline-only measurement. This records metrics but is not a gate.
    Measurement,
    /// Regression gate. Requires candidate metrics to pass.
    Gate,
}

/// Thresholds applied by the embedding regression gate.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct EvalGateThresholds {
    /// Required candidate Recall@K delta relative to baseline.
    pub min_recall_at_k_delta: f64,
    /// Absolute minimum candidate Recall@K.
    pub min_recall_at_k: f64,
    /// Absolute minimum candidate Recall@5.
    pub min_recall_at_5: f64,
    /// Absolute minimum candidate Recall@10.
    pub min_recall_at_10: f64,
    /// Required candidate MRR delta relative to baseline.
    pub min_mrr_delta: f64,
    /// Absolute minimum candidate MRR.
    pub min_mrr: f64,
    /// Per-slice Recall@K floors.
    pub slice_floors: Vec<SliceFloor>,
}

impl EvalGateThresholds {
    /// Build gate thresholds from an explicit Recall@K delta.
    ///
    /// All absolute floors default to zero (no absolute gate) and no slice
    /// floors are configured.
    #[must_use]
    pub fn new(min_recall_at_k_delta: f64) -> Self {
        Self {
            min_recall_at_k_delta,
            min_recall_at_k: 0.0,
            min_recall_at_5: 0.0,
            min_recall_at_10: 0.0,
            min_mrr_delta: 0.0,
            min_mrr: 0.0,
            slice_floors: Vec::new(),
        }
    }

    /// Set the absolute minimum candidate Recall@K.
    #[must_use]
    pub fn min_recall_at_k(mut self, value: f64) -> Self {
        self.min_recall_at_k = value;
        self
    }

    /// Set the absolute minimum candidate Recall@5.
    #[must_use]
    pub fn min_recall_at_5(mut self, value: f64) -> Self {
        self.min_recall_at_5 = value;
        self
    }

    /// Set the absolute minimum candidate Recall@10.
    #[must_use]
    pub fn min_recall_at_10(mut self, value: f64) -> Self {
        self.min_recall_at_10 = value;
        self
    }

    /// Set the required candidate MRR delta relative to baseline.
    #[must_use]
    pub fn min_mrr_delta(mut self, value: f64) -> Self {
        self.min_mrr_delta = value;
        self
    }

    /// Set the absolute minimum candidate MRR.
    #[must_use]
    pub fn min_mrr(mut self, value: f64) -> Self {
        self.min_mrr = value;
        self
    }

    /// Add a slice-specific Recall@K floor.
    #[must_use]
    pub fn slice_floor(mut self, slice: impl Into<String>, value: f64) -> Self {
        self.slice_floors.push(SliceFloor {
            slice: slice.into(),
            min_recall_at_k: value,
        });
        self
    }
}

impl Default for EvalGateThresholds {
    fn default() -> Self {
        Self::new(DEFAULT_MIN_RECALL_AT_K_DELTA)
    }
}

/// Aggregate evaluation metrics for one model.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModelMetrics {
    /// The embedding model name as reported by the provider.
    pub model_name: String,
    /// Non-secret configuration provenance for the model that produced these
    /// metrics (provider, explicit model, base URL, dimension).
    pub provenance: ModelProvenance,
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
    /// Per-slice Recall@K, keyed by [`EvalQuery::slice`].
    ///
    /// WHY: slice-level gates need per-cohort recall; unlabelled queries are
    /// not represented here because they have no slice to gate on.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub slice_recall_at_k: HashMap<String, f64>,
}

/// Result of a full evaluation run, optionally including a candidate model
/// evaluated side-by-side against a baseline.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EvalRunResult {
    /// Whether this run measured a baseline or enforced a regression gate.
    pub mode: EvalRunMode,
    /// Metrics for the baseline (current) model.
    pub baseline: ModelMetrics,
    /// Metrics for the candidate model, if one was evaluated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate: Option<ModelMetrics>,
    /// Thresholds used by the regression gate.
    pub gate_thresholds: EvalGateThresholds,
    /// Human-readable reason when a gate run does not pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    /// `true` if this run completed successfully for its mode.
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
pub(crate) fn evaluate_model(
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
    // WHY: dataset/corpus inconsistency must fail closed so typos and stale
    // labels surface as dataset errors instead of silent model misses.
    let _validated = dataset.validate_against_corpus(corpus)?;

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
    // WHY: track slice-level (hits, total) so we can compute per-slice recall
    // for slice-specific quality floors.
    let mut slice_counts: HashMap<String, (usize, usize)> = HashMap::new();

    for eq in &dataset.queries {
        let outcome =
            score_one_query(provider, eq, corpus, &corpus_vecs, [eff_k, eff_k5, eff_k10])?;
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
            slice: eq.slice.clone(),
            hit: outcome.hit_k,
            reciprocal_rank: outcome.reciprocal_rank,
            top_k_ids: outcome.top_k_ids,
        });
        if let Some(slice) = eq.slice.as_ref() {
            let entry = slice_counts.entry(slice.clone()).or_insert((0, 0));
            entry.1 += 1;
            if outcome.hit_k {
                entry.0 += 1;
            }
        }
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

    let slice_recall_at_k = slice_counts
        .into_iter()
        .map(|(slice, (hits, total))| {
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "slice query counts are small dataset sizes that fit exactly in f64"
            )]
            let recall = hits as f64 / total as f64;
            (slice, recall)
        })
        .collect();

    Ok(ModelMetrics {
        model_name: provider.model_name().to_owned(),
        provenance: provider.provenance(),
        k: eff_k,
        recall_at_k: hits_at_k / queries_f,
        recall_at_5: hits_at_five / queries_f,
        recall_at_10: hits_at_ten / queries_f,
        mrr: rr_sum / queries_f,
        per_query,
        slice_recall_at_k,
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

/// Evaluate a baseline provider without enforcing a regression gate.
///
/// Use this for explicit measurement runs only. Automation that needs a model
/// upgrade gate should call [`compare_models`].
///
/// # Errors
///
/// Propagates errors from the crate-private `evaluate_model` helper.
#[instrument(skip(baseline, dataset, corpus), fields(k = k))]
pub fn measure_baseline(
    baseline: &dyn EmbeddingProvider,
    dataset: &EvalDataset,
    corpus: &[(String, String)],
    k: usize,
) -> EvalResult<EvalRunResult> {
    measure_baseline_with_thresholds(baseline, dataset, corpus, k, EvalGateThresholds::default())
}

/// Evaluate a baseline provider without enforcing a regression gate, recording
/// the supplied thresholds in the result for audit purposes.
///
/// # Errors
///
/// Propagates errors from the crate-private `evaluate_model` helper.
#[instrument(skip(baseline, dataset, corpus), fields(k = k))]
pub fn measure_baseline_with_thresholds(
    baseline: &dyn EmbeddingProvider,
    dataset: &EvalDataset,
    corpus: &[(String, String)],
    k: usize,
    gate_thresholds: EvalGateThresholds,
) -> EvalResult<EvalRunResult> {
    let baseline_metrics = evaluate_model(baseline, dataset, corpus, k)?;

    Ok(EvalRunResult {
        mode: EvalRunMode::Measurement,
        baseline: baseline_metrics,
        candidate: None,
        gate_thresholds,
        failure_reason: None,
        passed: true,
    })
}

/// Evaluate baseline and candidate providers side-by-side as a regression gate.
///
/// Returns [`EvalRunResult::passed = false`] when candidate metrics are absent
/// or candidate Recall@K is lower than the configured baseline threshold.
///
/// # Errors
///
/// Propagates errors from the crate-private `evaluate_model` helper.
#[instrument(skip(baseline, candidate, dataset, corpus), fields(k = k))]
pub fn compare_models(
    baseline: &dyn EmbeddingProvider,
    candidate: Option<&dyn EmbeddingProvider>,
    dataset: &EvalDataset,
    corpus: &[(String, String)],
    k: usize,
) -> EvalResult<EvalRunResult> {
    compare_models_with_thresholds(
        baseline,
        candidate,
        dataset,
        corpus,
        k,
        EvalGateThresholds::default(),
    )
}

/// Evaluate baseline and candidate providers side-by-side with explicit gate
/// thresholds, including absolute floors, MRR gates, and slice-specific floors.
///
/// # Errors
///
/// Propagates errors from the crate-private `evaluate_model` helper.
#[instrument(skip(baseline, candidate, dataset, corpus), fields(k = k))]
pub fn compare_models_with_thresholds(
    baseline: &dyn EmbeddingProvider,
    candidate: Option<&dyn EmbeddingProvider>,
    dataset: &EvalDataset,
    corpus: &[(String, String)],
    k: usize,
    gate_thresholds: EvalGateThresholds,
) -> EvalResult<EvalRunResult> {
    let baseline_metrics = evaluate_model(baseline, dataset, corpus, k)?;

    let (candidate_metrics, passed) = if let Some(cand) = candidate {
        let cm = evaluate_model(cand, dataset, corpus, k)?;
        let ok = check_gate(&cm, &baseline_metrics, &gate_thresholds).is_ok();
        (Some(cm), ok)
    } else {
        (None, false)
    };
    let failure_reason = gate_failure_reason(
        candidate_metrics.as_ref(),
        &baseline_metrics,
        &gate_thresholds,
        passed,
    );

    Ok(EvalRunResult {
        mode: EvalRunMode::Gate,
        baseline: baseline_metrics,
        candidate: candidate_metrics,
        gate_thresholds,
        failure_reason,
        passed,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Gate check result: `Ok(())` when the candidate passes all configured
/// thresholds, or `Err` with a list of human-readable failure messages.
type GateCheck = std::result::Result<(), Vec<String>>;

/// Check all configured gate thresholds for a candidate against a baseline.
///
/// Enforces both regression checks (candidate >= baseline + delta) and absolute
/// minimums. A poor baseline can no longer let a poor candidate pass when
/// absolute floors are configured.
fn check_gate(
    candidate: &ModelMetrics,
    baseline: &ModelMetrics,
    thresholds: &EvalGateThresholds,
) -> GateCheck {
    let mut failures = Vec::new();

    let required_recall_k = baseline.recall_at_k + thresholds.min_recall_at_k_delta;
    if candidate.recall_at_k < required_recall_k {
        failures.push(format!(
            "candidate Recall@{} ({:.1}%) is below baseline regression threshold ({:.1}%)",
            candidate.k,
            candidate.recall_at_k * 100.0,
            required_recall_k * 100.0,
        ));
    }
    if candidate.recall_at_k < thresholds.min_recall_at_k {
        failures.push(format!(
            "candidate Recall@{} ({:.1}%) is below absolute floor ({:.1}%)",
            candidate.k,
            candidate.recall_at_k * 100.0,
            thresholds.min_recall_at_k * 100.0,
        ));
    }
    if candidate.recall_at_5 < thresholds.min_recall_at_5 {
        failures.push(format!(
            "candidate Recall@5 ({:.1}%) is below absolute floor ({:.1}%)",
            candidate.recall_at_5 * 100.0,
            thresholds.min_recall_at_5 * 100.0,
        ));
    }
    if candidate.recall_at_10 < thresholds.min_recall_at_10 {
        failures.push(format!(
            "candidate Recall@10 ({:.1}%) is below absolute floor ({:.1}%)",
            candidate.recall_at_10 * 100.0,
            thresholds.min_recall_at_10 * 100.0,
        ));
    }

    let required_mrr = baseline.mrr + thresholds.min_mrr_delta;
    if candidate.mrr < required_mrr {
        failures.push(format!(
            "candidate MRR ({:.3}) is below baseline regression threshold ({:.3})",
            candidate.mrr, required_mrr,
        ));
    }
    if candidate.mrr < thresholds.min_mrr {
        failures.push(format!(
            "candidate MRR ({:.3}) is below absolute floor ({:.3})",
            candidate.mrr, thresholds.min_mrr,
        ));
    }

    for floor in &thresholds.slice_floors {
        let slice_recall = candidate
            .slice_recall_at_k
            .get(&floor.slice)
            .copied()
            .unwrap_or(0.0);
        if slice_recall < floor.min_recall_at_k {
            failures.push(format!(
                "candidate Recall@{} for slice {:?} ({:.1}%) is below floor ({:.1}%)",
                candidate.k,
                floor.slice,
                slice_recall * 100.0,
                floor.min_recall_at_k * 100.0,
            ));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

fn gate_failure_reason(
    candidate: Option<&ModelMetrics>,
    baseline: &ModelMetrics,
    gate_thresholds: &EvalGateThresholds,
    passed: bool,
) -> Option<String> {
    if passed {
        return None;
    }

    let Some(candidate) = candidate else {
        return Some("candidate provider missing for embedding regression gate".to_owned());
    };

    match check_gate(candidate, baseline, gate_thresholds) {
        Ok(()) => Some("embedding regression gate failed for an unspecified reason".to_owned()),
        Err(failures) => Some(failures.join("; ")),
    }
}

/// Cosine similarity between two L2-normalized f32 vectors (dot product).
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
#[path = "embedding_eval_tests.rs"]
mod tests;
