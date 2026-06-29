//! `aletheia benchmark`: run memory benchmarks (`LongMemEval`, `LoCoMo`) against a
//! live instance.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Subcommand};
use serde::Serialize;
use snafu::prelude::*;

use dokimion::benchmarks::{
    BenchmarkMetadata, BenchmarkReport, BenchmarkRunner, BenchmarkRunnerConfig,
    BenchmarkValidationOptions, BenchmarkValidationReport, EvalClient, MemoryBenchmark,
};

use crate::error::Result;

mod gate;
mod output;

#[cfg(test)]
mod tests;

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
    /// Validate a saved benchmark report against a reviewed regression baseline
    Gate(BenchmarkGateArgs),
    /// List available benchmarks and download instructions
    List,
}

// kanon:ignore RUST/struct-too-many-fields — CLI args struct for benchmark runner; each field is a distinct command-line option
#[derive(Debug, Clone, Args)]
pub(crate) struct RunArgs {
    /// Path to the benchmark dataset JSON file
    #[arg(long)]
    pub dataset: PathBuf,
    /// Server URL to benchmark against
    #[arg(long, default_value = "http://127.0.0.1:18789")]
    // kanon:ignore SECURITY/hardcoded-loopback-url -- CLI default, user-overridable at runtime via --url flag
    pub url: String,
    /// Bearer token for authenticated endpoints
    #[arg(long, env = "ALETHEIA_EVAL_TOKEN")]
    pub token: Option<String>,
    /// Nous agent ID to test
    #[arg(long, default_value = "benchmark")]
    // kanon:ignore RUST/primitive-for-domain-id — CLI arg struct field; clap parses from string, newtype would require custom FromStr
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
    /// Compare the run against a saved compact baseline summary and surface the reward
    #[arg(long)]
    pub baseline_in: Option<PathBuf>,
    /// Compare against a prior full `BenchmarkReport` JSON as baseline/candidate statistics
    #[arg(long)]
    pub baseline_report: Option<PathBuf>,
    /// Enforce a reviewed benchmark regression gate baseline against this run
    #[arg(long)]
    pub gate_baseline: Option<PathBuf>,
    /// Require publishable statistical context and complete provenance
    #[arg(long)]
    pub publishable: bool,
    /// Query the knowledge store after ingestion and compute Recall@k / NDCG@k
    #[arg(long)]
    pub retrieval_k: Option<usize>,
    /// Allow incomplete benchmark records and report validation warnings.
    #[arg(long)]
    pub best_effort_dataset: bool,
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

#[derive(Debug, Clone, Args)]
pub(crate) struct BenchmarkGateArgs {
    /// Saved candidate `BenchmarkReport` JSON to validate
    #[arg(long)]
    pub candidate_report: PathBuf,
    /// Reviewed regression gate baseline artifact
    #[arg(long)]
    pub baseline: PathBuf,
    /// Output the gate result as JSON
    #[arg(long)]
    pub json: bool,
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
        BenchmarkAction::Gate(a) => gate::run(a).await,
    }
}

/// Reject obviously-broken inputs before loading datasets or talking to the
/// server. Otherwise `--timeout 0` / `--max-questions 0` / `--retrieval-k 0`
/// quietly exit with an empty report (looking like a passing run), and a
/// malformed `--url` or empty `--nous-id` only surfaces via downstream HTTP
/// errors that read like a server-down or missing-agent problem.
fn validate_args(args: &RunArgs) -> Result<()> {
    if args.timeout == 0 {
        whatever!(
            "--timeout must be greater than 0 seconds (got 0; a zero timeout fails every question instantly)"
        );
    }
    if args.max_questions == Some(0) {
        whatever!("--max-questions must be greater than 0 when set (got 0; nothing would run)");
    }
    if args.retrieval_k == Some(0) {
        whatever!(
            "--retrieval-k must be greater than 0 when set (got 0; recall@0 / NDCG@0 are not meaningful)"
        );
    }
    if args.publishable && args.gate_baseline.is_none() {
        whatever!(
            "--publishable requires --gate-baseline so publishable benchmark runs cannot bypass regression thresholds"
        );
    }
    if args.nous_id.trim().is_empty() {
        whatever!("--nous-id must not be empty");
    }
    if let Err(e) = reqwest::Url::parse(&args.url) {
        whatever!("--url is not a valid URL: {e} (got {:?})", args.url);
    }
    Ok(())
}

async fn run_longmemeval(args: RunArgs) -> Result<()> {
    validate_args(&args)?;
    let (dataset, validation) = dokimion::benchmarks::load_longmemeval_with_options(
        &args.dataset,
        validation_options(&args),
    )
    .await
    .whatever_context("failed to load LongMemEval dataset")?;
    run_benchmark(&dataset, &args, validation).await
}

async fn run_locomo(args: RunArgs) -> Result<()> {
    validate_args(&args)?;
    let (dataset, validation) =
        dokimion::benchmarks::load_locomo_with_options(&args.dataset, validation_options(&args))
            .await
            .whatever_context("failed to load LoCoMo dataset")?;
    run_benchmark(&dataset, &args, validation).await
}

fn validation_options(args: &RunArgs) -> BenchmarkValidationOptions {
    BenchmarkValidationOptions {
        dataset_path: Some(args.dataset.display().to_string()),
        allow_best_effort: args.best_effort_dataset,
        require_retrieval_evidence: args.retrieval_k.is_some(),
    }
}

async fn run_benchmark(
    benchmark: &dyn MemoryBenchmark,
    args: &RunArgs,
    validation: BenchmarkValidationReport,
) -> Result<()> {
    let client = EvalClient::new(&args.url, args.token.clone());
    let metadata = collect_metadata(&client, benchmark, args, validation).await;
    let config = BenchmarkRunnerConfig {
        nous_id: args.nous_id.clone(),
        session_key_prefix: format!("bench-{}", benchmark.name().to_lowercase()),
        question_timeout: Duration::from_secs(args.timeout),
        max_questions: args.max_questions,
        close_between_questions: true,
        judge: benchmark_judge_config(args),
        retrieval_k: args.retrieval_k,
        provenance: benchmark_provenance(benchmark, args, &metadata),
    };
    let runner = BenchmarkRunner::new(client, config);
    let mut report = runner
        .run(benchmark)
        .await
        .whatever_context("benchmark run failed")?;
    report.metadata = Some(metadata);
    report = report.with_standard_statistics();
    report = apply_baseline_report(report, args).await?;
    write_report_if_requested(&report, args).await?;

    if args.publishable {
        require_publishable_report(&report)?;
    }

    enforce_gate_if_requested(&report, args).await?;
    write_baseline_summary_if_requested(&report, args).await?;

    let reward_surface = if let Some(ref path) = args.baseline_in {
        Some(output::load_reward_surface(&report, path)?)
    } else {
        None
    };

    let rendered = if args.json {
        output::render_report_json(&report).whatever_context("failed to serialize report")?
    } else {
        output::render_report_human(&report, reward_surface.as_ref())
    };
    output::write_stdout(&rendered)?;

    Ok(())
}

fn benchmark_provenance(
    benchmark: &dyn MemoryBenchmark,
    args: &RunArgs,
    metadata: &BenchmarkMetadata,
) -> dokimion::provenance::EvalProvenance {
    let cli_args: Vec<String> = std::env::args().collect();
    let mut provenance = dokimion::provenance::EvalProvenance::new(
        dokimion::provenance::generate_eval_run_id(),
        args.url.clone(),
    )
    .with_redacted_args(&cli_args)
    .with_config_hash(benchmark_config_hash(benchmark, args))
    .with_target_identity(metadata.aletheia_version.clone())
    .with_audit_refs(Some(metadata.model.clone()), None, None, None, None);

    if let Some(git_sha) = metadata.git_sha.clone() {
        provenance = provenance.with_git_sha(git_sha);
    }
    if let Some(dataset_hash) = metadata.dataset_hash.clone() {
        provenance = provenance.with_scenario_suite_hash(dataset_hash);
    }
    provenance
}

fn benchmark_config_hash(benchmark: &dyn MemoryBenchmark, args: &RunArgs) -> String {
    dokimion::provenance::sha256_hex_str(&format!(
        "benchmark={}\ndataset={}\nurl={}\nnous_id={}\nmax_questions={:?}\ntimeout={}\njson={}\nretrieval_k={:?}\nbest_effort_dataset={}\nbaseline_report={:?}\ngate_baseline={:?}\npublishable={}\njudge_endpoint_present={}\njudge_model={}\njudge_api_key_present={}",
        benchmark.name(),
        args.dataset.display(),
        args.url,
        args.nous_id,
        args.max_questions,
        args.timeout,
        args.json,
        args.retrieval_k,
        args.best_effort_dataset,
        args.baseline_report.as_deref(),
        args.gate_baseline.as_deref(),
        args.publishable,
        args.judge_endpoint.is_some(),
        args.judge_model,
        args.judge_api_key.is_some(),
    ))
}

fn benchmark_judge_config(args: &RunArgs) -> Option<dokimion::benchmarks::judge::LlmJudgeConfig> {
    args.judge_endpoint
        .as_ref()
        .map(|endpoint| dokimion::benchmarks::judge::LlmJudgeConfig {
            endpoint: endpoint.clone(),
            model: args.judge_model.clone(),
            api_key: args.judge_api_key.clone(),
            max_tokens: 256,
            temperature: 0.0,
            timeout: Duration::from_secs(args.timeout),
        })
}

async fn apply_baseline_report(
    mut report: BenchmarkReport,
    args: &RunArgs,
) -> Result<BenchmarkReport> {
    if let Some(ref path) = args.baseline_report {
        let baseline_report = load_benchmark_report(path).await?;
        report = report.with_comparisons_against(&baseline_report, "baseline_vs_candidate");
    }
    Ok(report)
}

async fn write_report_if_requested(report: &BenchmarkReport, args: &RunArgs) -> Result<()> {
    if let Some(ref path) = args.output {
        let json =
            serde_json::to_string_pretty(report).whatever_context("failed to serialize report")?;
        tokio::fs::write(path, json)
            .await
            .whatever_context("failed to write report file")?;
        println!("Report written to {}", path.display());
    }
    Ok(())
}

async fn enforce_gate_if_requested(report: &BenchmarkReport, args: &RunArgs) -> Result<()> {
    if let Some(ref path) = args.gate_baseline {
        gate::enforce_report(report, path).await?;
    }
    Ok(())
}

async fn write_baseline_summary_if_requested(
    report: &BenchmarkReport,
    args: &RunArgs,
) -> Result<()> {
    if let Some(ref path) = args.baseline_out {
        let summary = BenchmarkBaselineSummary::from_report(report);
        let json = serde_json::to_string_pretty(&summary)
            .whatever_context("failed to serialize baseline summary")?;
        tokio::fs::write(path, json)
            .await
            .whatever_context("failed to write baseline summary file")?;
        println!("Baseline summary written to {}", path.display());
    }
    Ok(())
}

async fn load_benchmark_report(path: &Path) -> Result<BenchmarkReport> {
    let json = tokio::fs::read_to_string(path)
        .await
        .with_whatever_context(|_| format!("failed to read benchmark report {}", path.display()))?;
    serde_json::from_str(&json)
        .with_whatever_context(|_| format!("failed to parse benchmark report {}", path.display()))
}

fn require_publishable_report(report: &BenchmarkReport) -> Result<()> {
    let assessment = report
        .publishability
        .clone()
        .unwrap_or_else(|| report.assess_publishability());
    if assessment.publishable {
        return Ok(());
    }

    whatever!(
        "--publishable requires statistical summaries and complete provenance; report is not publishable:\n- {}",
        assessment.reasons.join("\n- ")
    );
}

#[derive(Debug, Serialize)]
struct BenchmarkBaselineSummary {
    benchmark: String,
    total_questions: usize,
    scored_questions: usize,
    error_questions: usize,
    timeout_questions: usize,
    no_answer_questions: usize,
    exact_match_rate: f64,
    mean_f1: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    judge_accuracy: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    judge_attempted: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    judge_scored: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    judge_errors: Option<usize>,
}

impl BenchmarkBaselineSummary {
    fn from_report(report: &BenchmarkReport) -> Self {
        Self {
            benchmark: report.benchmark.clone(),
            total_questions: report.total,
            scored_questions: report.scored,
            error_questions: report.errors,
            timeout_questions: report.timeouts,
            no_answer_questions: report.no_answers,
            exact_match_rate: report.exact_match_rate(),
            mean_f1: report.mean_f1(),
            judge_accuracy: report.judge_accuracy(),
            judge_attempted: report.judge_summary.map(|summary| summary.attempted),
            judge_scored: report.judge_summary.map(|summary| summary.scored),
            judge_errors: report.judge_summary.map(|summary| summary.errors),
        }
    }
}

async fn collect_metadata(
    client: &EvalClient,
    benchmark: &dyn MemoryBenchmark,
    args: &RunArgs,
    validation: BenchmarkValidationReport,
) -> BenchmarkMetadata {
    let version = client
        .health()
        .await
        .ok()
        .and_then(|h| h.version)
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());

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
        dataset_hash: dataset_hash(&args.dataset).await,
        git_sha: current_git_sha(),
        dataset_best_effort: args.best_effort_dataset,
        dataset_validation: Some(validation),
    }
}

fn current_git_sha() -> Option<String> {
    option_env!("GITHUB_SHA")
        .map(str::trim)
        .filter(|sha| !sha.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            let output = std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            String::from_utf8(output.stdout)
                .ok()
                .map(|sha| sha.trim().to_owned())
                .filter(|sha| !sha.is_empty())
        })
}

async fn dataset_hash(path: &Path) -> Option<String> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Some(format!(
            "sha256:{}",
            dokimion::provenance::sha256_hex(&bytes)
        )),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "failed to hash benchmark dataset");
            None
        }
    }
}
