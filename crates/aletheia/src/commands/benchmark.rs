//! `aletheia benchmark`: run memory benchmarks (`LongMemEval`, `LoCoMo`) against a
//! live instance.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use serde::Serialize;
use snafu::prelude::*;

use dokimion::benchmarks::{
    BenchmarkMetadata, BenchmarkReport, BenchmarkRunner, BenchmarkRunnerConfig, EvalClient,
    MemoryBenchmark,
};

use crate::error::Result;

#[derive(Debug, Clone, Args)]
pub(crate) struct BenchmarkArgs {
    #[command(subcommand)]
    pub action: BenchmarkAction,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum BenchmarkAction {
    /// Run the `LongMemEval` benchmark (arxiv 2410.10813)
    Longmemeval(RunArgs),
    /// Run the `LoCoMo` benchmark (arxiv 2402.17753)
    Locomo(RunArgs),
    /// List available benchmarks and download instructions
    List,
}

#[derive(Debug, Clone, Args)]
pub(crate) struct RunArgs {
    /// Path to the benchmark dataset JSON file
    #[arg(long)]
    pub dataset: PathBuf,
    /// Server URL to benchmark against
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    pub url: String,
    /// Bearer token for authenticated endpoints
    #[arg(long, env = "ALETHEIA_EVAL_TOKEN")]
    pub token: Option<String>,
    /// Nous agent ID to test
    #[arg(long, default_value = "benchmark")]
    pub nous_id: String,
    /// Maximum number of questions to evaluate (useful for smoke tests)
    #[arg(long)]
    pub max_questions: Option<usize>,
    /// Per-question timeout in seconds
    #[arg(long, default_value_t = 120)]
    pub timeout: u64,
    /// Output results as JSON instead of human-readable table
    #[arg(long)]
    pub json: bool,
    /// Write the full JSON report to a file for reproducibility and publishing
    #[arg(long)]
    pub output: Option<PathBuf>,
    /// Write a compact baseline summary for training reward loaders
    #[arg(long)]
    pub baseline_out: Option<PathBuf>,
    /// Query the knowledge store after ingestion and compute Recall@k / NDCG@k
    #[arg(long)]
    pub retrieval_k: Option<usize>,
    /// LLM-as-judge endpoint (OpenAI-compatible). If set, each answer is judged.
    #[arg(long, env = "ALETHEIA_JUDGE_ENDPOINT")]
    pub judge_endpoint: Option<String>,
    /// LLM-as-judge model identifier
    #[arg(long, default_value = dokimion::benchmarks::judge::DEFAULT_JUDGE_MODEL, env = "ALETHEIA_JUDGE_MODEL")]
    pub judge_model: String,
    /// LLM-as-judge API key
    #[arg(long, env = "ALETHEIA_JUDGE_API_KEY")]
    pub judge_api_key: Option<String>,
}

pub(crate) async fn run(args: BenchmarkArgs) -> Result<()> {
    match args.action {
        BenchmarkAction::List => {
            println!("Available benchmarks:\n");
            println!(
                "  longmemeval   LongMemEval (arxiv 2410.10813) — 500 questions, 5 memory abilities"
            );
            println!(
                "  locomo        LoCoMo (arxiv 2402.17753) — 50 conversations, ~200 QA each\n"
            );
            println!("{}", dokimion::benchmarks::download_instructions());
            Ok(())
        }
        BenchmarkAction::Longmemeval(a) => run_longmemeval(a).await,
        BenchmarkAction::Locomo(a) => run_locomo(a).await,
    }
}

async fn run_longmemeval(args: RunArgs) -> Result<()> {
    let dataset = dokimion::benchmarks::load_longmemeval(&args.dataset)
        .await
        .whatever_context("failed to load LongMemEval dataset")?;
    run_benchmark(&dataset, &args).await
}

async fn run_locomo(args: RunArgs) -> Result<()> {
    let dataset = dokimion::benchmarks::load_locomo(&args.dataset)
        .await
        .whatever_context("failed to load LoCoMo dataset")?;
    run_benchmark(&dataset, &args).await
}

async fn run_benchmark(benchmark: &dyn MemoryBenchmark, args: &RunArgs) -> Result<()> {
    let client = EvalClient::new(&args.url, args.token.clone());

    // Collect system metadata before running.
    let metadata = collect_metadata(&client, benchmark, args).await;

    let judge =
        args.judge_endpoint
            .as_ref()
            .map(|endpoint| dokimion::benchmarks::judge::LlmJudgeConfig {
                endpoint: endpoint.clone(),
                model: args.judge_model.clone(),
                api_key: args.judge_api_key.clone(),
                max_tokens: 256,
                temperature: 0.0,
            });

    let config = BenchmarkRunnerConfig {
        nous_id: args.nous_id.clone(),
        session_key_prefix: format!("bench-{}", benchmark.name().to_lowercase()),
        question_timeout: Duration::from_secs(args.timeout),
        max_questions: args.max_questions,
        close_between_questions: true,
        judge,
        retrieval_k: args.retrieval_k,
    };
    let runner = BenchmarkRunner::new(client, config);
    let mut report = runner
        .run(benchmark)
        .await
        .whatever_context("benchmark run failed")?;
    report.metadata = Some(metadata);

    // Write to file if --output was provided.
    if let Some(ref path) = args.output {
        let json =
            serde_json::to_string_pretty(&report).whatever_context("failed to serialize report")?;
        tokio::fs::write(path, json)
            .await
            .whatever_context("failed to write report file")?;
        println!("Report written to {}", path.display());
    }

    if let Some(ref path) = args.baseline_out {
        let summary = BenchmarkBaselineSummary::from_report(&report);
        let json = serde_json::to_string_pretty(&summary)
            .whatever_context("failed to serialize baseline summary")?;
        tokio::fs::write(path, json)
            .await
            .whatever_context("failed to write baseline summary file")?;
        println!("Baseline summary written to {}", path.display());
    }

    if args.json {
        print_report_json(&report).whatever_context("failed to serialize report")?;
    } else {
        print_report_human(&report);
    }

    Ok(())
}

#[derive(Debug, Serialize)]
struct BenchmarkBaselineSummary {
    benchmark: String,
    total_questions: usize,
    exact_match_rate: f64,
    mean_f1: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    judge_accuracy: Option<f64>,
}

impl BenchmarkBaselineSummary {
    fn from_report(report: &BenchmarkReport) -> Self {
        Self {
            benchmark: report.benchmark.clone(),
            total_questions: report.total,
            exact_match_rate: report.exact_match_rate(),
            mean_f1: report.mean_f1(),
            judge_accuracy: report.judge_accuracy(),
        }
    }
}

async fn collect_metadata(
    client: &EvalClient,
    benchmark: &dyn MemoryBenchmark,
    args: &RunArgs,
) -> BenchmarkMetadata {
    let version = client
        .health()
        .await
        .map_or_else(|_| "unknown".to_owned(), |h| h.version);

    let model = client
        .get_nous(&args.nous_id)
        .await
        .map_or_else(|_| "unknown".to_owned(), |n| n.model);

    BenchmarkMetadata {
        timestamp: jiff::Timestamp::now().to_string(),
        aletheia_version: version,
        nous_id: args.nous_id.clone(),
        model,
        benchmark: benchmark.name().to_owned(),
        total_questions: benchmark.len(),
        evaluated_questions: args.max_questions.unwrap_or(benchmark.len()),
        timeout_secs: args.timeout,
    }
}

/// Print a human-readable benchmark report with per-category breakdown
/// and peer baseline comparison.
fn print_report_human(report: &BenchmarkReport) {
    let use_color = supports_color::on(supports_color::Stream::Stdout).is_some();

    let header = if let Some(ref meta) = report.metadata {
        format!(
            "{} Benchmark \u{2014} {} ({})",
            report.benchmark, meta.nous_id, meta.model
        )
    } else {
        format!("{} Benchmark", report.benchmark)
    };
    if use_color {
        println!("{}", header.bold());
    } else {
        println!("{header}");
    }
    println!("{}", "\u{2550}".repeat(header.len()));

    if let Some(ref meta) = report.metadata {
        println!(
            "Version: {} | Questions: {}/{} | Timeout: {}s\n",
            meta.aletheia_version,
            meta.evaluated_questions,
            meta.total_questions,
            meta.timeout_secs
        );
    } else {
        println!("Questions: {}\n", report.total);
    }

    // Table header
    println!("Results:");
    println!("  {:<30} {:>6} {:>6}", "Category", "EM%", "F1%");
    println!("  {}", "\u{2500}".repeat(44));

    // Per-category rows
    let categories = report.per_category();
    for (cat, em, f1) in &categories {
        let em_pct = em * 100.0;
        let f1_pct = f1 * 100.0;
        if use_color {
            println!(
                "  {:<30} {:>5.1}  {:>5.1}",
                cat,
                em_pct.yellow(),
                f1_pct.yellow()
            );
        } else {
            println!("  {cat:<30} {em_pct:>5.1}  {f1_pct:>5.1}");
        }
    }

    // Overall row
    let overall_em = report.exact_match_rate() * 100.0;
    let overall_f1 = report.mean_f1() * 100.0;
    println!("  {}", "\u{2500}".repeat(44));
    if use_color {
        println!(
            "  {:<30} {:>5.1}  {:>5.1}",
            "Overall".bold(),
            format!("{overall_em:.1}").green().bold(),
            format!("{overall_f1:.1}").green().bold()
        );
    } else {
        println!(
            "  {:<30} {:>5.1}  {:>5.1}",
            "Overall", overall_em, overall_f1
        );
    }

    // Optional retrieval metrics
    if let Some(recall) = report.mean_recall_at_k() {
        let ndcg = report.mean_ndcg_at_k().unwrap_or(0.0);
        println!("\nRetrieval metrics (k={}):", args_retrieval_k(report));
        println!("  Mean Recall@k: {recall:.3} | Mean NDCG@k: {ndcg:.3}");
    }

    // Optional judge accuracy
    if let Some(judge_acc) = report.judge_accuracy() {
        println!("\nLLM-as-judge accuracy: {:.1}%", judge_acc * 100.0);
    }

    // Peer baseline comparison
    print_baselines(report, use_color);
}

fn args_retrieval_k(report: &BenchmarkReport) -> usize {
    // Infer k from the first question that has retrieval metrics.
    report
        .questions
        .iter()
        .find(|q| q.recall_at_k.is_some())
        .and_then(|q| q.retrieved_facts.as_ref().map(Vec::len))
        .unwrap_or(0)
}

/// Print peer baseline comparison table.
fn print_baselines(report: &BenchmarkReport, use_color: bool) {
    let baselines = match report.benchmark.as_str() {
        "LongMemEval" => dokimion::benchmarks::baselines::longmemeval_baselines(),
        "LoCoMo" => dokimion::benchmarks::baselines::locomo_baselines(),
        _ => return,
    };

    println!("\nPeer baselines:");
    println!("  {:<28} {:>8} {:>8}", "System", "EM%", "F1%");
    println!("  {}", "\u{2500}".repeat(46));

    for baseline in &baselines {
        let em_str = baseline
            .exact_match_rate
            .map_or_else(|| "-".to_owned(), |v| format!("{:.1}", v * 100.0));
        let f1_str = baseline
            .mean_f1
            .map_or_else(|| "-".to_owned(), |v| format!("{:.1}", v * 100.0));
        if use_color {
            println!(
                "  {:<28} {:>8} {:>8}  {}",
                baseline.system.dimmed(),
                em_str.dimmed(),
                f1_str.dimmed(),
                baseline.note.dimmed()
            );
        } else {
            println!(
                "  {:<28} {:>8} {:>8}  {}",
                baseline.system, em_str, f1_str, baseline.note
            );
        }
    }
}

/// Print the benchmark report as JSON for machine consumption.
fn print_report_json(report: &BenchmarkReport) -> std::result::Result<(), serde_json::Error> {
    let json = serde_json::to_string_pretty(report)?;
    println!("{json}");
    Ok(())
}
