//! `aletheia eval-embeddings`: embedding quality measurement and upgrade gate.
//!
//! Loads a labelled query set (JSONL), embeds each query against a corpus
//! of known facts, computes Recall@K and MRR, and compares a candidate model
//! against a baseline unless explicit measurement mode is requested.
//!
//! Exits non-zero when gate mode lacks a candidate or the candidate model's
//! Recall@K regresses below baseline.

use std::path::PathBuf;

use clap::Args;
use snafu::prelude::*;

use crate::error::Result;

/// Arguments for the `eval-embeddings` subcommand.
#[derive(Debug, Clone, Args)]
pub(crate) struct EvalEmbeddingsArgs {
    /// Path to the JSONL evaluation dataset.
    ///
    /// Each line must be a JSON object with fields:
    ///   `query`        — natural-language query string
    ///   `relevant_ids` — array of corpus IDs that should appear in top-K
    ///   `description`  — optional human-readable label (ignored during eval)
    #[arg(short = 'd', long)]
    pub dataset: PathBuf,

    /// Path to a JSONL corpus file (one `{"id":"…","text":"…"}` per line).
    ///
    /// When omitted, a small built-in synthetic corpus is used for smoke testing.
    #[arg(short = 'c', long)]
    pub corpus: Option<PathBuf>,

    /// Number of top results to retrieve per query (Recall@K).
    #[arg(short = 'k', long, default_value_t = 5)]
    pub top_k: usize,

    /// Baseline (current) embedding provider.
    ///
    /// Accepted values mirror the `embedding.provider` config key:
    /// `candle` (default, local pure-Rust), `voyage` (cloud, API key required).
    #[arg(long, default_value = "candle")]
    pub baseline_provider: String,

    /// Candidate model provider for side-by-side comparison.
    ///
    /// Required for gate mode. The candidate must match or exceed baseline
    /// Recall@K to pass. Accepted values: `candle`, `voyage`.
    ///
    /// Omit only with `--measure`.
    #[arg(long)]
    pub candidate_provider: Option<String>,

    /// Run explicit baseline measurement mode instead of the regression gate.
    ///
    /// Measurement mode evaluates only the baseline model and exits zero when
    /// metrics are produced. It does not report or imply a gate pass.
    #[arg(long)]
    pub measure: bool,

    /// Output full results as JSON instead of a human-readable table.
    #[arg(long)]
    pub json: bool,
}

/// A corpus entry in the JSONL corpus file format.
#[derive(Debug, Clone, serde::Deserialize)]
struct CorpusEntry {
    id: String,
    text: String,
}

#[derive(Debug, serde::Serialize)]
struct EvalEmbeddingsReport<'a> {
    #[serde(flatten)]
    run: &'a episteme::embedding_eval::EvalRunResult,
    dataset: EvalInputReport,
    corpus: EvalInputReport,
    providers: EvalProviderReport<'a>,
    models: EvalModelReport<'a>,
}

#[derive(Debug, serde::Serialize)]
struct EvalInputReport {
    reference: String,
    records: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    provenance: Option<BuiltinCorpusMetadata>,
}

/// Metadata for the built-in synthetic smoke corpus.
///
/// WHY: Synthetic corpora must be self-describing so operators and reports can
/// distinguish fixture data from production datasets and verify it contains no
/// real personal/private information.
#[derive(Debug, serde::Serialize)]
struct BuiltinCorpusMetadata {
    dataset_id: &'static str,
    schema_version: u32,
    provenance: &'static str,
    fixture_safety: &'static str,
}

const BUILTIN_CORPUS_META: BuiltinCorpusMetadata = BuiltinCorpusMetadata {
    dataset_id: "aletheia:eval-embeddings:builtin-smoke-corpus",
    schema_version: 1,
    provenance: "Built into the aletheia eval-embeddings command for smoke testing; generated from obviously synthetic identities and example domains only.",
    fixture_safety: "Contains no real people, networks, or medical information. Names are conventional test identities (alice, bob, ...) and domains use example.local.",
};

#[derive(Debug, serde::Serialize)]
struct EvalProviderReport<'a> {
    baseline: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate: Option<&'a str>,
}

#[derive(Debug, serde::Serialize)]
struct EvalModelReport<'a> {
    baseline: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate: Option<&'a str>,
}

/// Parse a corpus JSONL file into `(id, text)` pairs.
fn load_corpus(path: &std::path::Path) -> Result<Vec<(String, String)>> {
    let contents = std::fs::read_to_string(path)
        .whatever_context(format!("cannot read corpus file: {}", path.display()))?;

    let mut pairs = Vec::new();
    for (idx, line) in contents.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let entry: CorpusEntry = serde_json::from_str(trimmed)
            .whatever_context(format!("corpus line {}: invalid JSON", idx + 1))?;
        pairs.push((entry.id, entry.text));
    }
    Ok(pairs)
}

/// Built-in synthetic corpus used when no corpus file is provided.
///
/// WHY: Smoke-test fixtures must be obviously synthetic. They should not
/// contain real network addresses, medical details, or private-looking facts
/// unless the test is explicitly exercising redaction/privacy behavior.
fn builtin_corpus() -> Vec<(String, String)> {
    vec![
        (
            "fact-001".into(),
            "alice prefers tea over coffee in the mornings".into(),
        ),
        (
            "fact-002".into(),
            "the example.corp staging deployment runs on imaginary orchestrator version 0.0.0".into(),
        ),
        (
            "fact-003".into(),
            "bob commutes by bicycle from the north side of town".into(),
        ),
        (
            "fact-004".into(),
            "carol leads the distributed systems research team at university".into(),
        ),
        (
            "fact-005".into(),
            "the primary database replica runs on host db-00.example.local".into(),
        ),
        (
            "fact-006".into(),
            "the project milestone is scheduled for the next planning cycle".into(),
        ),
        (
            "fact-007".into(),
            "dave wrote the initial implementation of the recall pipeline".into(),
        ),
        (
            "fact-008".into(),
            "the knowledge store uses cosine similarity for vector search".into(),
        ),
        (
            "fact-009".into(),
            "eve prefers asynchronous communication via email over meetings".into(),
        ),
        (
            "fact-010".into(),
            "the backup task runs daily at a configurable hour and retains a fixed number of snapshots".into(),
        ),
        (
            "fact-011".into(),
            "frank dislikes the fictional snack 'quzzle' used only in fixture data".into(),
        ),
        (
            "fact-012".into(),
            "grace manages the on-call rotation for the infrastructure team".into(),
        ),
        (
            "fact-013".into(),
            "the HNSW index uses m=16 and ef_construction=200 by default".into(),
        ),
        (
            "fact-014".into(),
            "heidi finished the onboarding process on the first day of the month".into(),
        ),
        (
            "fact-015".into(),
            "the staging environment mirrors production except for test certificates".into(),
        ),
    ]
}

/// Print a human-readable comparison table to stdout.
fn print_table(run: &episteme::embedding_eval::EvalRunResult) {
    use episteme::embedding_eval::EvalRunMode;
    use owo_colors::OwoColorize;

    println!();
    println!("┌─────────────────────────────────────────────────────────┐");
    if run.mode == EvalRunMode::Measurement {
        println!("│              Embedding Baseline Measurement              │");
    } else {
        println!("│              Embedding Regression Gate                   │");
    }
    println!("└─────────────────────────────────────────────────────────┘");
    println!();

    let b = &run.baseline;
    println!("  Baseline  : {}", b.model_name);
    println!("  K         : {}", b.k);
    println!("  Recall@K  : {:.1}%", b.recall_at_k * 100.0);
    println!("  Recall@5  : {:.1}%", b.recall_at_5 * 100.0);
    println!("  Recall@10 : {:.1}%", b.recall_at_10 * 100.0);
    println!("  MRR       : {:.3}", b.mrr);
    println!();

    if let Some(c) = &run.candidate {
        println!("  Candidate : {}", c.model_name);
        println!("  K         : {}", c.k);
        let diff_rk = c.recall_at_k - b.recall_at_k;
        let diff_mrr = c.mrr - b.mrr;
        let rk_str = format!(
            "{:.1}%  (Δ {:+.1}%)",
            c.recall_at_k * 100.0,
            diff_rk * 100.0
        );
        if diff_rk >= 0.0 {
            println!("  Recall@K  : {}", rk_str.green());
        } else {
            println!("  Recall@K  : {}", rk_str.red());
        }
        println!("  Recall@5  : {:.1}%", c.recall_at_5 * 100.0);
        println!("  Recall@10 : {:.1}%", c.recall_at_10 * 100.0);
        let mrr_str = format!("{:.3}  (Δ {:+.3})", c.mrr, diff_mrr);
        if diff_mrr >= 0.0 {
            println!("  MRR       : {}", mrr_str.green());
        } else {
            println!("  MRR       : {}", mrr_str.red());
        }
        println!();
    }

    if run.mode == EvalRunMode::Measurement {
        println!("  {}", "BASELINE MEASUREMENT COMPLETE".green().bold());
    } else if run.passed {
        println!("  {}", "GATE PASSED".green().bold());
    } else if run.candidate.is_none() {
        println!(
            "  {}",
            "GATE FAILED — candidate provider missing".red().bold()
        );
    } else {
        println!(
            "  {}",
            "GATE FAILED — candidate regresses Recall@K".red().bold()
        );
    }
    println!();
}

fn build_report<'a>(
    run: &'a episteme::embedding_eval::EvalRunResult,
    args: &'a EvalEmbeddingsArgs,
    dataset_records: usize,
    corpus_records: usize,
) -> EvalEmbeddingsReport<'a> {
    let (corpus_reference, corpus_provenance) = args.corpus.as_ref().map_or_else(
        || {
            let reference = format!(
                "builtin:{}@v{}",
                BUILTIN_CORPUS_META.dataset_id, BUILTIN_CORPUS_META.schema_version
            );
            (reference, Some(BUILTIN_CORPUS_META))
        },
        |path| (path.display().to_string(), None),
    );

    EvalEmbeddingsReport {
        run,
        dataset: EvalInputReport {
            reference: args.dataset.display().to_string(),
            records: dataset_records,
            provenance: None,
        },
        corpus: EvalInputReport {
            reference: corpus_reference,
            records: corpus_records,
            provenance: corpus_provenance,
        },
        providers: EvalProviderReport {
            baseline: args.baseline_provider.as_str(),
            candidate: args.candidate_provider.as_deref(),
        },
        models: EvalModelReport {
            baseline: run.baseline.model_name.as_str(),
            candidate: run.candidate.as_ref().map(|c| c.model_name.as_str()),
        },
    }
}

/// Reject obviously-broken inputs up-front so operators don't get a
/// meaningless table (e.g. `Recall@0: 0.0%`) instead of an error.
fn validate_args(args: &EvalEmbeddingsArgs) -> Result<()> {
    if args.top_k == 0 {
        snafu::whatever!("--top-k must be greater than 0 (got 0; Recall@0 is undefined)");
    }
    if args.measure && args.candidate_provider.is_some() {
        snafu::whatever!("--measure cannot be combined with --candidate-provider");
    }
    if !args.measure && args.candidate_provider.is_none() {
        snafu::whatever!(
            "embedding regression gate requires --candidate-provider; use --measure for baseline-only measurement"
        );
    }
    Ok(())
}

/// Entry point for the `eval-embeddings` subcommand.
pub(crate) fn run(args: &EvalEmbeddingsArgs) -> Result<()> {
    use episteme::embedding_eval::{EvalDataset, EvalRunMode, compare_models, measure_baseline};
    use mneme::embedding::{EmbeddingConfig, create_provider};

    validate_args(args)?;

    // Load dataset.
    let dataset = EvalDataset::from_jsonl_file(&args.dataset)
        .whatever_context("failed to load eval dataset")?;

    if dataset.is_empty() {
        snafu::whatever!(
            "eval dataset is empty: add at least one query to {}",
            args.dataset.display()
        );
    }

    // Load or use built-in corpus.
    let corpus: Vec<(String, String)> = if let Some(ref p) = args.corpus {
        load_corpus(p)?
    } else {
        builtin_corpus()
    };

    if corpus.is_empty() {
        snafu::whatever!("corpus is empty — provide at least one (id, text) entry");
    }

    // Build baseline provider.
    let baseline_config = EmbeddingConfig {
        provider: args.baseline_provider.clone(),
        ..EmbeddingConfig::default()
    };
    let baseline = create_provider(&baseline_config).whatever_context(format!(
        "failed to create baseline embedding provider '{}'",
        args.baseline_provider
    ))?;

    // Build optional candidate provider.
    let candidate_config = args.candidate_provider.as_deref().map(|p| EmbeddingConfig {
        provider: p.to_owned(),
        ..EmbeddingConfig::default()
    });
    let candidate_box;
    let candidate: Option<&dyn mneme::embedding::EmbeddingProvider> =
        if let Some(cfg) = &candidate_config {
            candidate_box = create_provider(cfg).whatever_context(format!(
                "failed to create candidate embedding provider '{}'",
                cfg.provider
            ))?;
            Some(candidate_box.as_ref())
        } else {
            None
        };

    // Run evaluation.
    let run = if args.measure {
        measure_baseline(baseline.as_ref(), &dataset, &corpus, args.top_k)
            .whatever_context("evaluation failed")?
    } else {
        compare_models(baseline.as_ref(), candidate, &dataset, &corpus, args.top_k)
            .whatever_context("evaluation failed")?
    };

    // Output.
    if args.json {
        let report = build_report(&run, args, dataset.queries.len(), corpus.len());
        let json = serde_json::to_string_pretty(&report)
            .whatever_context("failed to serialize eval result as JSON")?;
        println!("{json}");
    } else {
        print_table(&run);
    }

    if run.passed {
        Ok(())
    } else if run.mode == EvalRunMode::Gate {
        snafu::whatever!(
            "embedding evaluation gate failed: {}",
            run.failure_reason
                .as_deref()
                .unwrap_or("candidate Recall@K regresses below baseline")
        )
    } else {
        snafu::whatever!("embedding baseline measurement failed")
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn args_with(top_k: usize) -> EvalEmbeddingsArgs {
        EvalEmbeddingsArgs {
            dataset: PathBuf::from("/dev/null"),
            corpus: None,
            top_k,
            baseline_provider: "candle".to_owned(),
            candidate_provider: None,
            measure: false,
            json: false,
        }
    }

    #[test]
    fn validate_rejects_top_k_zero() {
        let err = validate_args(&args_with(0)).unwrap_err();
        assert!(
            err.to_string().contains("--top-k must be greater than 0"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_accepts_positive_top_k() {
        let mut args = args_with(1);
        args.measure = true;
        validate_args(&args).unwrap();

        let mut args = args_with(5);
        args.candidate_provider = Some("voyage".to_owned());
        validate_args(&args).unwrap();

        let mut args = args_with(100);
        args.measure = true;
        validate_args(&args).unwrap();
    }

    #[test]
    fn validate_rejects_gate_without_candidate_provider() {
        let err = validate_args(&args_with(5)).unwrap_err();
        assert!(
            err.to_string().contains("requires --candidate-provider"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_rejects_measure_with_candidate_provider() {
        let mut args = args_with(5);
        args.measure = true;
        args.candidate_provider = Some("voyage".to_owned());
        let err = validate_args(&args).unwrap_err();
        assert!(
            err.to_string()
                .contains("--measure cannot be combined with --candidate-provider"),
            "got: {err}"
        );
    }
}
