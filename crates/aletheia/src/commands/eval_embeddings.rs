//! `aletheia eval-embeddings`: embedding quality gate for model upgrades.
//!
//! Loads a labelled query set (JSONL), embeds each query against a corpus
//! of known facts, computes Recall@K and MRR, and optionally compares a
//! candidate model against a baseline.
//!
//! Exits non-zero when a candidate model's Recall@K regresses below baseline.

use std::path::PathBuf;

use aletheia_koina::color::{supports_color, AnsiColorize};
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
    /// When set, the candidate must match or exceed baseline Recall@K to pass.
    /// Accepted values: `candle`, `voyage`.
    #[arg(long)]
    pub candidate_provider: Option<String>,

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

/// Parse a corpus JSONL file into `(id, text)` pairs.
fn load_corpus(path: &std::path::Path) -> Result<Vec<(String, String)>> {
    #[expect(
        clippy::disallowed_methods,
        reason = "synchronous filesystem I/O is correct here; eval-embeddings runs outside the async runtime"
    )]
    let contents = std::fs::read_to_string(path)
        .whatever_context(format!("cannot read corpus file: {}", path.display()))?;

    let mut pairs = Vec::new();
    for (idx, line) in contents.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let entry: CorpusEntry = serde_json::from_str(trimmed).whatever_context(format!(
            "corpus line {}: invalid JSON",
            idx + 1
        ))?;
        pairs.push((entry.id, entry.text));
    }
    Ok(pairs)
}

/// Built-in synthetic corpus used when no corpus file is provided.
///
/// Covers the same domains as the seed eval dataset so smoke-test runs
/// work out of the box without an instance.
fn builtin_corpus() -> Vec<(String, String)> {
    vec![
        (
            "fact-001".into(),
            "alice prefers tea over coffee in the mornings".into(),
        ),
        (
            "fact-002".into(),
            "the acme.corp deployment runs on kubernetes version 1.28".into(),
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
            "the 192.168.1.100 server hosts the primary database replica".into(),
        ),
        (
            "fact-006".into(),
            "the project deadline is set for the end of the fiscal quarter".into(),
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
            "the backup job runs daily at 03:00 and retains seven snapshots".into(),
        ),
        (
            "fact-011".into(),
            "frank is allergic to peanuts and carries an epinephrine injector".into(),
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
            "the staging environment mirrors production except for TLS certificates".into(),
        ),
    ]
}

/// Print a human-readable comparison table to stdout.
fn print_table(run: &aletheia_mneme::embedding_eval::EvalRunResult) {
    let color = supports_color();

    println!();
    println!("┌─────────────────────────────────────────────────────────┐");
    println!("│              Embedding Evaluation Results                │");
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
        let rk_str = format!("{:.1}%  (Δ {:+.1}%)", c.recall_at_k * 100.0, diff_rk * 100.0);
        if diff_rk >= 0.0 {
            println!("  Recall@K  : {}", rk_str.green(color));
        } else {
            println!("  Recall@K  : {}", rk_str.red(color));
        }
        println!("  Recall@5  : {:.1}%", c.recall_at_5 * 100.0);
        println!("  Recall@10 : {:.1}%", c.recall_at_10 * 100.0);
        let mrr_str = format!("{:.3}  (Δ {:+.3})", c.mrr, diff_mrr);
        if diff_mrr >= 0.0 {
            println!("  MRR       : {}", mrr_str.green(color));
        } else {
            println!("  MRR       : {}", mrr_str.red(color));
        }
        println!();
    }

    if run.passed {
        println!("  {}", "GATE PASSED".green(color).bold(color));
    } else {
        println!("  {}", "GATE FAILED — candidate regresses Recall@K".red(color).bold(color));
    }
    println!();
}

/// Entry point for the `eval-embeddings` subcommand.
pub(crate) fn run(args: EvalEmbeddingsArgs) -> Result<()> {
    use aletheia_mneme::embedding::{EmbeddingConfig, create_provider};
    use aletheia_mneme::embedding_eval::{EvalDataset, compare_models};

    // Load dataset.
    let dataset = EvalDataset::from_jsonl_file(&args.dataset)
        .whatever_context("failed to load eval dataset")?;

    if dataset.is_empty() {
        snafu::whatever!("eval dataset is empty: add at least one query to {}", args.dataset.display());
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
    let candidate: Option<&dyn aletheia_mneme::embedding::EmbeddingProvider> =
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
    let run = compare_models(baseline.as_ref(), candidate, &dataset, &corpus, args.top_k)
        .whatever_context("evaluation failed")?;

    // Output.
    if args.json {
        let json = serde_json::to_string_pretty(&run)
            .whatever_context("failed to serialize eval result as JSON")?;
        println!("{json}");
    } else {
        print_table(&run);
    }

    if run.passed {
        Ok(())
    } else {
        snafu::whatever!(
            "embedding evaluation gate failed: candidate Recall@{} ({:.1}%) regresses below baseline ({:.1}%)",
            run.candidate.as_ref().map_or(0, |c| c.k),
            run.candidate.as_ref().map_or(0.0, |c| c.recall_at_k * 100.0),
            run.baseline.recall_at_k * 100.0,
        )
    }
}
