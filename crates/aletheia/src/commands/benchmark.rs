//! `aletheia benchmark`: run memory benchmarks (`LongMemEval`, `LoCoMo`) against a
//! live instance.

use std::path::PathBuf;
use std::time::Duration;

use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use snafu::prelude::*;

use dokimion::benchmarks::{
    BenchmarkReport, BenchmarkRunner, BenchmarkRunnerConfig, EvalClient, MemoryBenchmark,
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
}

pub(crate) async fn run(args: BenchmarkArgs) -> Result<()> {
    match args.action {
        BenchmarkAction::List => {
            println!("Available benchmarks:\n");
            println!("  longmemeval   LongMemEval (arxiv 2410.10813) — 500 questions, 5 memory abilities");
            println!("  locomo        LoCoMo (arxiv 2402.17753) — 50 conversations, ~200 QA each\n");
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
    let config = BenchmarkRunnerConfig {
        nous_id: args.nous_id.clone(),
        session_key_prefix: format!("bench-{}", benchmark.name().to_lowercase()),
        question_timeout: Duration::from_secs(args.timeout),
        max_questions: args.max_questions,
        close_between_questions: true,
    };
    let runner = BenchmarkRunner::new(client, config);
    let report = runner
        .run(benchmark)
        .await
        .whatever_context("benchmark run failed")?;

    if args.json {
        print_report_json(&report).whatever_context("failed to serialize report")?;
    } else {
        print_report_human(&report, &args.nous_id, args.timeout);
    }

    Ok(())
}

/// Print a human-readable benchmark report with per-category breakdown.
fn print_report_human(report: &BenchmarkReport, nous_id: &str, timeout: u64) {
    let use_color = supports_color::on(supports_color::Stream::Stdout).is_some();

    let header = format!("{} Benchmark \u{2014} agent: {nous_id}", report.benchmark);
    if use_color {
        println!("{}", header.bold());
    } else {
        println!("{header}");
    }
    println!("{}", "\u{2550}".repeat(header.len()));
    println!(
        "Questions: {} | Timeout: {timeout}s\n",
        report.total
    );

    // Table header
    println!("Results:");
    println!(
        "  {:<30} {:>6} {:>6}",
        "Category", "EM%", "F1%"
    );
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
        println!("  {:<30} {:>5.1}  {:>5.1}", "Overall", overall_em, overall_f1);
    }
}

/// Print the benchmark report as JSON for machine consumption.
fn print_report_json(report: &BenchmarkReport) -> std::result::Result<(), serde_json::Error> {
    let categories: Vec<_> = report
        .per_category()
        .into_iter()
        .map(|(cat, em, f1)| {
            serde_json::json!({
                "category": cat,
                "exact_match_rate": em,
                "f1": f1,
            })
        })
        .collect();

    let json = serde_json::json!({
        "benchmark": report.benchmark,
        "total": report.total,
        "exact_match_rate": report.exact_match_rate(),
        "mean_f1": report.mean_f1(),
        "categories": categories,
        "questions": report.questions.iter().map(|q| {
            serde_json::json!({
                "id": q.id,
                "category": q.category,
                "actual_answer": q.actual_answer,
                "expected_answers": q.expected_answers,
                "exact_match": q.score.exact_match,
                "f1": q.score.f1,
                "contains": q.score.contains,
            })
        }).collect::<Vec<_>>(),
    });

    let output = serde_json::to_string_pretty(&json)?;
    println!("{output}");
    Ok(())
}
