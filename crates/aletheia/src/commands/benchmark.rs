//! `aletheia benchmark`: run memory benchmarks (`LongMemEval`, `LoCoMo`) against a
//! live instance.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use serde::{Deserialize, Serialize};
use snafu::prelude::*;

use dokimion::benchmarks::{
    BenchmarkComparisonReport, BenchmarkComparisonStatus, BenchmarkMetadata, BenchmarkReport,
    BenchmarkRunner, BenchmarkRunnerConfig, BenchmarkValidationOptions, BenchmarkValidationReport,
    EvalClient, MemoryBenchmark,
};
use episteme::rl::{LongMemEvalReward, MemoryOutcome, RewardFn};

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
        BenchmarkAction::Gate(a) => run_gate(a).await,
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
        Some(load_reward_surface(&report, path)?)
    } else {
        None
    };

    if args.json {
        print_report_json(&report).whatever_context("failed to serialize report")?;
    } else {
        print_report_human(&report, reward_surface.as_ref());
    }

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
        let baseline = load_gate_baseline(path).await?;
        let gate_report = benchmark_gate_report(report, &baseline)?;
        print_gate_report(&gate_report, false).whatever_context("failed to print gate report")?;
        require_gate_passed(&gate_report)?;
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

async fn run_gate(args: BenchmarkGateArgs) -> Result<()> {
    let candidate = load_benchmark_report(&args.candidate_report).await?;
    let baseline = load_gate_baseline(&args.baseline).await?;
    let gate_report = benchmark_gate_report(&candidate, &baseline)?;
    print_gate_report(&gate_report, args.json).whatever_context("failed to print gate report")?;
    require_gate_passed(&gate_report)
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

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkGateBaseline {
    version: u32,
    benchmark: String,
    provenance: BenchmarkGateProvenance,
    metrics: BenchmarkGateMetrics,
    allowed_regression: BenchmarkGateMetrics,
    minimums: BenchmarkGateMinimums,
    maximums: BenchmarkGateMaximums,
    require_retrieval: bool,
    require_judge: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkGateProvenance {
    dataset_hash: String,
    dataset_version: String,
    model: String,
    source_report: String,
    reviewed_at: String,
    reviewed_by: String,
    #[serde(default)]
    git_sha: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkGateMetrics {
    exact_match_rate: f64,
    mean_f1: f64,
    error_rate: f64,
    timeout_rate: f64,
    no_answer_rate: f64,
    #[serde(default)]
    recall_at_k: Option<f64>,
    #[serde(default)]
    ndcg_at_k: Option<f64>,
    #[serde(default)]
    judge_accuracy: Option<f64>,
    #[serde(default)]
    judge_error_rate: Option<f64>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkGateMinimums {
    scored_questions: usize,
    exact_match_rate: f64,
    mean_f1: f64,
    #[serde(default)]
    recall_at_k: Option<f64>,
    #[serde(default)]
    ndcg_at_k: Option<f64>,
    #[serde(default)]
    judge_accuracy: Option<f64>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkGateMaximums {
    #[serde(rename = "error_rate")]
    errors: f64,
    #[serde(rename = "timeout_rate")]
    timeouts: f64,
    #[serde(rename = "no_answer_rate")]
    no_answers: f64,
    #[serde(default)]
    #[serde(rename = "judge_error_rate")]
    judge_errors: Option<f64>,
}

#[derive(Debug, Clone, Copy)]
struct CandidateGateMetrics {
    scored_questions: usize,
    exact_match_rate: Option<f64>,
    mean_f1: Option<f64>,
    error_rate: Option<f64>,
    timeout_rate: Option<f64>,
    no_answer_rate: Option<f64>,
    recall_at_k: Option<f64>,
    ndcg_at_k: Option<f64>,
    judge_accuracy: Option<f64>,
    judge_error_rate: Option<f64>,
}

#[derive(Debug, Clone)]
struct CandidateGateContext {
    dataset_hash: Option<String>,
    model: Option<String>,
    git_sha: Option<String>,
    metadata_benchmark: Option<String>,
    metrics: CandidateGateMetrics,
}

#[derive(Debug, Serialize)]
struct BenchmarkGateReport {
    passed: bool,
    benchmark: String,
    baseline_dataset_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate_dataset_hash: Option<String>,
    baseline_model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseline_git_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate_git_sha: Option<String>,
    baseline_source_report: String,
    checks: Vec<BenchmarkGateCheck>,
}

#[derive(Debug, Serialize)]
struct BenchmarkGateCheck {
    metric: String,
    passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    actual: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    threshold: Option<f64>,
    direction: String,
    detail: String,
}

impl BenchmarkGateCheck {
    fn new(
        metric: impl Into<String>,
        passed: bool,
        actual: Option<f64>,
        threshold: Option<f64>,
        direction: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            metric: metric.into(),
            passed,
            actual,
            threshold,
            direction: direction.into(),
            detail: detail.into(),
        }
    }
}

async fn load_gate_baseline(path: &Path) -> Result<BenchmarkGateBaseline> {
    let json = tokio::fs::read_to_string(path)
        .await
        .with_whatever_context(|_| {
            format!(
                "failed to read benchmark regression gate baseline {}",
                path.display()
            )
        })?;
    let baseline: BenchmarkGateBaseline =
        serde_json::from_str(&json).with_whatever_context(|_| {
            format!(
                "failed to parse benchmark regression gate baseline {}",
                path.display()
            )
        })?;
    validate_gate_baseline(&baseline)?;
    Ok(baseline)
}

fn validate_gate_baseline(baseline: &BenchmarkGateBaseline) -> Result<()> {
    if baseline.version != 1 {
        whatever!(
            "benchmark regression gate baseline version must be 1 (got {})",
            baseline.version
        );
    }
    require_non_empty("benchmark", &baseline.benchmark)?;
    require_non_empty("provenance.dataset_hash", &baseline.provenance.dataset_hash)?;
    require_non_empty(
        "provenance.dataset_version",
        &baseline.provenance.dataset_version,
    )?;
    require_non_empty("provenance.model", &baseline.provenance.model)?;
    require_non_empty(
        "provenance.source_report",
        &baseline.provenance.source_report,
    )?;
    require_non_empty("provenance.reviewed_at", &baseline.provenance.reviewed_at)?;
    require_non_empty("provenance.reviewed_by", &baseline.provenance.reviewed_by)?;

    validate_gate_metrics("metrics", &baseline.metrics)?;
    validate_gate_metrics("allowed_regression", &baseline.allowed_regression)?;
    validate_probability(
        "minimums.exact_match_rate",
        baseline.minimums.exact_match_rate,
    )?;
    validate_probability("minimums.mean_f1", baseline.minimums.mean_f1)?;
    validate_optional_probability("minimums.recall_at_k", baseline.minimums.recall_at_k)?;
    validate_optional_probability("minimums.ndcg_at_k", baseline.minimums.ndcg_at_k)?;
    validate_optional_probability("minimums.judge_accuracy", baseline.minimums.judge_accuracy)?;
    validate_probability("maximums.error_rate", baseline.maximums.errors)?;
    validate_probability("maximums.timeout_rate", baseline.maximums.timeouts)?;
    validate_probability("maximums.no_answer_rate", baseline.maximums.no_answers)?;
    validate_optional_probability("maximums.judge_error_rate", baseline.maximums.judge_errors)?;
    Ok(())
}

fn validate_gate_metrics(prefix: &str, metrics: &BenchmarkGateMetrics) -> Result<()> {
    validate_probability(
        &format!("{prefix}.exact_match_rate"),
        metrics.exact_match_rate,
    )?;
    validate_probability(&format!("{prefix}.mean_f1"), metrics.mean_f1)?;
    validate_probability(&format!("{prefix}.error_rate"), metrics.error_rate)?;
    validate_probability(&format!("{prefix}.timeout_rate"), metrics.timeout_rate)?;
    validate_probability(&format!("{prefix}.no_answer_rate"), metrics.no_answer_rate)?;
    validate_optional_probability(&format!("{prefix}.recall_at_k"), metrics.recall_at_k)?;
    validate_optional_probability(&format!("{prefix}.ndcg_at_k"), metrics.ndcg_at_k)?;
    validate_optional_probability(&format!("{prefix}.judge_accuracy"), metrics.judge_accuracy)?;
    validate_optional_probability(
        &format!("{prefix}.judge_error_rate"),
        metrics.judge_error_rate,
    )?;
    Ok(())
}

fn require_non_empty(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        whatever!("benchmark regression gate baseline field {name} must not be empty");
    }
    Ok(())
}

fn validate_optional_probability(name: &str, value: Option<f64>) -> Result<()> {
    if let Some(value) = value {
        validate_probability(name, value)?;
    }
    Ok(())
}

fn validate_probability(name: &str, value: f64) -> Result<()> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        return Ok(());
    }
    whatever!(
        "benchmark regression gate baseline field {name} must be a finite probability in [0, 1] (got {value})"
    );
}

fn benchmark_gate_report(
    candidate: &BenchmarkReport,
    baseline: &BenchmarkGateBaseline,
) -> Result<BenchmarkGateReport> {
    validate_gate_baseline(baseline)?;

    let context = candidate_gate_context(candidate);
    let mut checks = Vec::new();
    push_gate_provenance_checks(&mut checks, candidate, &context, baseline);
    push_required_gate_metric_checks(&mut checks, context.metrics, baseline);
    push_optional_gate_metric_checks(&mut checks, context.metrics, baseline);

    let passed = checks.iter().all(|check| check.passed);
    Ok(BenchmarkGateReport {
        passed,
        benchmark: baseline.benchmark.clone(),
        baseline_dataset_hash: baseline.provenance.dataset_hash.clone(),
        candidate_dataset_hash: context.dataset_hash,
        baseline_model: baseline.provenance.model.clone(),
        candidate_model: context.model,
        baseline_git_sha: baseline.provenance.git_sha.clone(),
        candidate_git_sha: context.git_sha,
        baseline_source_report: baseline.provenance.source_report.clone(),
        checks,
    })
}

fn candidate_gate_context(report: &BenchmarkReport) -> CandidateGateContext {
    let metadata = report.metadata.as_ref();
    CandidateGateContext {
        dataset_hash: metadata.and_then(|meta| meta.dataset_hash.clone()),
        model: metadata.map(|meta| meta.model.clone()),
        git_sha: metadata.and_then(|meta| meta.git_sha.clone()),
        metadata_benchmark: metadata.map(|meta| meta.benchmark.clone()),
        metrics: candidate_gate_metrics(report),
    }
}

fn push_gate_provenance_checks(
    checks: &mut Vec<BenchmarkGateCheck>,
    candidate: &BenchmarkReport,
    context: &CandidateGateContext,
    baseline: &BenchmarkGateBaseline,
) {
    push_text_match_check(
        checks,
        "benchmark",
        Some(candidate.benchmark.as_str()),
        &baseline.benchmark,
    );
    push_text_match_check(
        checks,
        "metadata.benchmark",
        context.metadata_benchmark.as_deref(),
        &baseline.benchmark,
    );
    push_text_match_check(
        checks,
        "dataset_hash",
        context.dataset_hash.as_deref(),
        &baseline.provenance.dataset_hash,
    );
    push_text_match_check(
        checks,
        "model",
        context.model.as_deref(),
        &baseline.provenance.model,
    );
}

fn push_required_gate_metric_checks(
    checks: &mut Vec<BenchmarkGateCheck>,
    metrics: CandidateGateMetrics,
    baseline: &BenchmarkGateBaseline,
) {
    push_min_count_check(
        checks,
        "scored_questions",
        metrics.scored_questions,
        baseline.minimums.scored_questions,
    );
    push_min_metric_check(
        checks,
        "exact_match_rate",
        metrics.exact_match_rate,
        baseline.minimums.exact_match_rate,
        baseline.metrics.exact_match_rate,
        baseline.allowed_regression.exact_match_rate,
    );
    push_min_metric_check(
        checks,
        "mean_f1",
        metrics.mean_f1,
        baseline.minimums.mean_f1,
        baseline.metrics.mean_f1,
        baseline.allowed_regression.mean_f1,
    );
    push_max_metric_check(
        checks,
        "error_rate",
        metrics.error_rate,
        baseline.maximums.errors,
        baseline.metrics.error_rate,
        baseline.allowed_regression.error_rate,
    );
    push_max_metric_check(
        checks,
        "timeout_rate",
        metrics.timeout_rate,
        baseline.maximums.timeouts,
        baseline.metrics.timeout_rate,
        baseline.allowed_regression.timeout_rate,
    );
    push_max_metric_check(
        checks,
        "no_answer_rate",
        metrics.no_answer_rate,
        baseline.maximums.no_answers,
        baseline.metrics.no_answer_rate,
        baseline.allowed_regression.no_answer_rate,
    );
}

fn push_optional_gate_metric_checks(
    checks: &mut Vec<BenchmarkGateCheck>,
    metrics: CandidateGateMetrics,
    baseline: &BenchmarkGateBaseline,
) {
    push_optional_min_metric_check(
        checks,
        OptionalMinMetricGate {
            name: "recall_at_k",
            required: baseline.require_retrieval,
            actual: metrics.recall_at_k,
            floor: baseline.minimums.recall_at_k,
            baseline: baseline.metrics.recall_at_k,
            allowed_drop: baseline.allowed_regression.recall_at_k,
        },
    );
    push_optional_min_metric_check(
        checks,
        OptionalMinMetricGate {
            name: "ndcg_at_k",
            required: baseline.require_retrieval,
            actual: metrics.ndcg_at_k,
            floor: baseline.minimums.ndcg_at_k,
            baseline: baseline.metrics.ndcg_at_k,
            allowed_drop: baseline.allowed_regression.ndcg_at_k,
        },
    );
    push_optional_min_metric_check(
        checks,
        OptionalMinMetricGate {
            name: "judge_accuracy",
            required: baseline.require_judge,
            actual: metrics.judge_accuracy,
            floor: baseline.minimums.judge_accuracy,
            baseline: baseline.metrics.judge_accuracy,
            allowed_drop: baseline.allowed_regression.judge_accuracy,
        },
    );
    push_optional_max_metric_check(
        checks,
        OptionalMaxMetricGate {
            name: "judge_error_rate",
            required: baseline.require_judge,
            actual: metrics.judge_error_rate,
            ceiling: baseline.maximums.judge_errors,
            baseline: baseline.metrics.judge_error_rate,
            allowed_increase: baseline.allowed_regression.judge_error_rate,
        },
    );
}

fn candidate_gate_metrics(report: &BenchmarkReport) -> CandidateGateMetrics {
    CandidateGateMetrics {
        scored_questions: report.scored,
        exact_match_rate: Some(report.exact_match_rate()),
        mean_f1: Some(report.mean_f1()),
        error_rate: rate(report.errors, report.total),
        timeout_rate: rate(report.timeouts, report.total),
        no_answer_rate: rate(report.no_answers, report.total),
        recall_at_k: report.mean_recall_at_k(),
        ndcg_at_k: report.mean_ndcg_at_k(),
        judge_accuracy: report.judge_accuracy(),
        judge_error_rate: report
            .judge_summary
            .and_then(|summary| rate(summary.errors, summary.attempted)),
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "benchmark counts are bounded by dataset sizes and exactly represented at current scales"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64 is required for aggregate benchmark rates"
)]
fn rate(count: usize, total: usize) -> Option<f64> {
    (total > 0).then_some(count as f64 / total as f64)
}

#[expect(
    clippy::cast_precision_loss,
    reason = "benchmark counts are bounded by dataset sizes and exactly represented at current scales"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64 is required for machine-readable gate output"
)]
fn count_as_f64(value: usize) -> f64 {
    value as f64
}

fn push_text_match_check(
    checks: &mut Vec<BenchmarkGateCheck>,
    metric: &'static str,
    actual: Option<&str>,
    expected: &str,
) {
    let passed = actual == Some(expected);
    let detail = match actual {
        Some(actual) => format!("candidate {actual:?} must match reviewed baseline {expected:?}"),
        None => format!("candidate is missing {metric}; reviewed baseline expects {expected:?}"),
    };
    checks.push(BenchmarkGateCheck::new(
        metric, passed, None, None, "eq", detail,
    ));
}

fn push_min_count_check(
    checks: &mut Vec<BenchmarkGateCheck>,
    metric: &'static str,
    actual: usize,
    minimum: usize,
) {
    let actual_value = count_as_f64(actual);
    let minimum_value = count_as_f64(minimum);
    checks.push(BenchmarkGateCheck::new(
        metric,
        actual >= minimum,
        Some(actual_value),
        Some(minimum_value),
        "min",
        format!("actual {actual} must be at least reviewed minimum {minimum}"),
    ));
}

fn push_min_metric_check(
    checks: &mut Vec<BenchmarkGateCheck>,
    metric: &'static str,
    actual: Option<f64>,
    floor: f64,
    baseline: f64,
    allowed_drop: f64,
) {
    let regression_threshold = (baseline - allowed_drop).max(0.0);
    let threshold = floor.max(regression_threshold);
    let passed = actual.is_some_and(|value| value >= threshold);
    checks.push(BenchmarkGateCheck::new(
        metric,
        passed,
        actual,
        Some(threshold),
        "min",
        format!(
            "actual {} must be >= {} (floor {}, baseline {}, allowed drop {})",
            format_optional_metric(actual),
            format_metric(threshold),
            format_metric(floor),
            format_metric(baseline),
            format_metric(allowed_drop)
        ),
    ));
}

fn push_max_metric_check(
    checks: &mut Vec<BenchmarkGateCheck>,
    metric: &'static str,
    actual: Option<f64>,
    ceiling: f64,
    baseline: f64,
    allowed_increase: f64,
) {
    let regression_threshold = (baseline + allowed_increase).min(1.0);
    let threshold = ceiling.min(regression_threshold);
    let passed = actual.is_some_and(|value| value <= threshold);
    checks.push(BenchmarkGateCheck::new(
        metric,
        passed,
        actual,
        Some(threshold),
        "max",
        format!(
            "actual {} must be <= {} (ceiling {}, baseline {}, allowed increase {})",
            format_optional_metric(actual),
            format_metric(threshold),
            format_metric(ceiling),
            format_metric(baseline),
            format_metric(allowed_increase)
        ),
    ));
}

#[derive(Debug, Clone, Copy)]
struct OptionalMinMetricGate {
    name: &'static str,
    required: bool,
    actual: Option<f64>,
    floor: Option<f64>,
    baseline: Option<f64>,
    allowed_drop: Option<f64>,
}

fn push_optional_min_metric_check(
    checks: &mut Vec<BenchmarkGateCheck>,
    gate: OptionalMinMetricGate,
) {
    if !optional_metric_required(gate.required, gate.floor, gate.baseline, gate.allowed_drop) {
        return;
    }

    let missing_baseline = gate.required && gate.baseline.is_none();
    let floor = gate.floor.unwrap_or(0.0);
    let baseline = gate.baseline.unwrap_or(0.0);
    let allowed_drop = gate.allowed_drop.unwrap_or(0.0);
    let threshold = floor.max((baseline - allowed_drop).max(0.0));
    let passed = !missing_baseline && gate.actual.is_some_and(|value| value >= threshold);
    let mut detail = format!(
        "actual {} must be >= {} (floor {}, baseline {}, allowed drop {})",
        format_optional_metric(gate.actual),
        format_metric(threshold),
        format_optional_metric(gate.floor),
        format_optional_metric(gate.baseline),
        format_optional_metric(gate.allowed_drop)
    );
    if missing_baseline {
        detail.push_str("; reviewed baseline is missing this required metric");
    }
    checks.push(BenchmarkGateCheck::new(
        gate.name,
        passed,
        gate.actual,
        Some(threshold),
        "min",
        detail,
    ));
}

#[derive(Debug, Clone, Copy)]
struct OptionalMaxMetricGate {
    name: &'static str,
    required: bool,
    actual: Option<f64>,
    ceiling: Option<f64>,
    baseline: Option<f64>,
    allowed_increase: Option<f64>,
}

fn push_optional_max_metric_check(
    checks: &mut Vec<BenchmarkGateCheck>,
    gate: OptionalMaxMetricGate,
) {
    if !optional_metric_required(
        gate.required,
        gate.ceiling,
        gate.baseline,
        gate.allowed_increase,
    ) {
        return;
    }

    let missing_baseline = gate.required && gate.baseline.is_none();
    let ceiling = gate.ceiling.unwrap_or(1.0);
    let baseline = gate.baseline.unwrap_or(1.0);
    let allowed_increase = gate.allowed_increase.unwrap_or(0.0);
    let threshold = ceiling.min((baseline + allowed_increase).min(1.0));
    let passed = !missing_baseline && gate.actual.is_some_and(|value| value <= threshold);
    let mut detail = format!(
        "actual {} must be <= {} (ceiling {}, baseline {}, allowed increase {})",
        format_optional_metric(gate.actual),
        format_metric(threshold),
        format_optional_metric(gate.ceiling),
        format_optional_metric(gate.baseline),
        format_optional_metric(gate.allowed_increase)
    );
    if missing_baseline {
        detail.push_str("; reviewed baseline is missing this required metric");
    }
    checks.push(BenchmarkGateCheck::new(
        gate.name,
        passed,
        gate.actual,
        Some(threshold),
        "max",
        detail,
    ));
}

fn optional_metric_required(
    required: bool,
    threshold: Option<f64>,
    baseline: Option<f64>,
    tolerance: Option<f64>,
) -> bool {
    required || threshold.is_some() || baseline.is_some() || tolerance.is_some()
}

fn format_optional_metric(value: Option<f64>) -> String {
    value.map_or_else(|| "missing".to_owned(), format_metric)
}

fn format_metric(value: f64) -> String {
    format!("{value:.4}")
}

fn print_gate_report(
    report: &BenchmarkGateReport,
    json: bool,
) -> std::result::Result<(), serde_json::Error> {
    if json {
        let json = serde_json::to_string_pretty(report)?;
        println!("{json}");
        return Ok(());
    }

    if report.passed {
        println!(
            "Benchmark regression gate passed ({} checks).",
            report.checks.len()
        );
        return Ok(());
    }

    println!("Benchmark regression gate failed:");
    for check in report.checks.iter().filter(|check| !check.passed) {
        println!("  - {}: {}", check.metric, check.detail);
    }
    Ok(())
}

fn require_gate_passed(report: &BenchmarkGateReport) -> Result<()> {
    if report.passed {
        return Ok(());
    }
    whatever!("{}", gate_failure_message(report));
}

fn gate_failure_message(report: &BenchmarkGateReport) -> String {
    let failures = report
        .checks
        .iter()
        .filter(|check| !check.passed)
        .map(|check| format!("{}: {}", check.metric, check.detail))
        .collect::<Vec<_>>();

    if failures.is_empty() {
        "benchmark regression gate failed".to_owned()
    } else {
        format!(
            "benchmark regression gate failed:\n- {}",
            failures.join("\n- ")
        )
    }
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct RewardSurface {
    outcome: MemoryOutcome,
    baseline_exact_match_rate: f64,
    reward: f64,
}

fn load_reward_surface(report: &BenchmarkReport, baseline_path: &Path) -> Result<RewardSurface> {
    let reward_fn = LongMemEvalReward::from_json_file(baseline_path)
        .whatever_context("failed to load baseline summary for reward calculation")?;
    let outcome = MemoryOutcome {
        exact_match_rate: report.exact_match_rate(),
        mean_f1: Some(report.mean_f1()),
    };
    let reward = reward_fn.reward(&outcome);

    Ok(RewardSurface {
        outcome,
        baseline_exact_match_rate: reward_fn.baseline_exact_match_rate,
        reward,
    })
}

fn format_reward_surface(surface: &RewardSurface) -> String {
    format!(
        "Reward vs baseline: {:+.3} (EM {:.1}% vs baseline {:.1}%)",
        surface.reward,
        surface.outcome.exact_match_rate * 100.0,
        surface.baseline_exact_match_rate * 100.0,
    )
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

/// Print a human-readable benchmark report with per-category breakdown
/// and peer baseline comparison.
fn print_report_human(report: &BenchmarkReport, reward_surface: Option<&RewardSurface>) {
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
            "Version: {} | Questions: {}/{} | Timeout: {}s",
            meta.aletheia_version,
            meta.evaluated_questions,
            meta.total_questions,
            meta.timeout_secs
        );
        if meta.dataset_best_effort {
            let warnings = meta
                .dataset_validation
                .as_ref()
                .map_or(0, |validation| validation.warnings.len());
            println!("Dataset validation: best-effort ({warnings} warning(s))");
        }
    } else {
        println!("Questions: {}", report.total);
    }
    println!(
        "Attempted: {} | Scored: {} | Errors: {} | Timeouts: {} | No answer: {}\n",
        report.total, report.scored, report.errors, report.timeouts, report.no_answers
    );

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

    print_statistics(report);
    print_publishability(report);
    print_comparisons(report);

    // Optional retrieval metrics
    if let Some(recall) = report.mean_recall_at_k() {
        let ndcg = report.mean_ndcg_at_k().unwrap_or(0.0);
        println!("\nRetrieval metrics (k={}):", args_retrieval_k(report));
        println!("  Mean Recall@k: {recall:.3} | Mean NDCG@k: {ndcg:.3}");
    }

    // Optional judge accuracy
    if let (Some(judge_acc), Some(summary)) = (report.judge_accuracy(), report.judge_summary) {
        println!(
            "\nLLM-as-judge accuracy: {:.1}% ({} correct / {} attempted; {} parsed, {} errors)",
            judge_acc * 100.0,
            summary.correct,
            summary.attempted,
            summary.scored,
            summary.errors
        );
    }

    if let Some(surface) = reward_surface {
        println!("\n{}", format_reward_surface(surface));
    }

    // Peer baseline comparison
    print_baselines(report, use_color);
}

fn print_statistics(report: &BenchmarkReport) {
    if let Some(statistics) = &report.statistics {
        println!(
            "\nStatistics (95% bootstrap CI, {} resamples):",
            statistics.n_resamples
        );
        println!(
            "  EM: {:.1}% [{:.1}, {:.1}]",
            report.exact_match_rate() * 100.0,
            statistics.em_ci_low * 100.0,
            statistics.em_ci_high * 100.0
        );
        println!(
            "  F1: {:.1}% [{:.1}, {:.1}]",
            report.mean_f1() * 100.0,
            statistics.f1_ci_low * 100.0,
            statistics.f1_ci_high * 100.0
        );
        println!("  Method: {}", statistics.method);
    } else {
        println!(
            "\nStatistics: unavailable (requires at least {} scored questions)",
            dokimion::benchmarks::MIN_PUBLISHABLE_SCORED_QUESTIONS
        );
    }
}

fn print_publishability(report: &BenchmarkReport) {
    let Some(assessment) = &report.publishability else {
        return;
    };
    if assessment.publishable {
        println!("\nPublishability: publishable");
        return;
    }

    println!("\nPublishability: not publishable");
    for reason in &assessment.reasons {
        println!("  - {reason}");
    }
}

fn print_comparisons(report: &BenchmarkReport) {
    if report.comparisons.is_empty() {
        return;
    }

    println!("\nBaseline/candidate statistics:");
    for comparison in &report.comparisons {
        print_comparison(comparison);
    }
}

fn print_comparison(comparison: &BenchmarkComparisonReport) {
    if let (BenchmarkComparisonStatus::Complete, Some(statistics)) =
        (&comparison.status, &comparison.statistics)
    {
        println!(
            "  {}: baseline {:.3}, candidate {:.3}, d={} ({})",
            comparison.metric,
            statistics.mean_a,
            statistics.mean_b,
            format_float(statistics.effect.d),
            statistics.effect.interpretation
        );
        println!(
            "      baseline CI [{}, {}] | candidate CI [{}, {}] | p_raw={} | p_fdr={}",
            format_float(statistics.ci_a.ci_low),
            format_float(statistics.ci_a.ci_high),
            format_float(statistics.ci_b.ci_low),
            format_float(statistics.ci_b.ci_high),
            format_float(statistics.p_raw),
            statistics
                .p_adjusted
                .map_or_else(|| "n/a".to_owned(), format_float)
        );
    } else {
        let reason = comparison
            .reason
            .as_deref()
            .unwrap_or("comparison statistics are incomplete");
        println!("  {}: {} ({reason})", comparison.metric, comparison.status);
    }
}

fn format_float(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.4}")
    } else if value.is_nan() {
        "n/a".to_owned()
    } else if value.is_sign_positive() {
        "inf".to_owned()
    } else {
        "-inf".to_owned()
    }
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

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use dokimion::benchmarks::{BenchmarkScore, QuestionResult, QuestionStatus};
    use std::io::Write as _;

    fn base_args() -> RunArgs {
        RunArgs {
            dataset: PathBuf::from("/tmp/does-not-matter.json"),
            url: "http://127.0.0.1:18789".to_owned(),
            token: None,
            nous_id: "benchmark".to_owned(),
            max_questions: None,
            timeout: 120,
            json: false,
            output: None,
            baseline_out: None,
            baseline_in: None,
            baseline_report: None,
            gate_baseline: None,
            publishable: false,
            retrieval_k: None,
            best_effort_dataset: false,
            judge_endpoint: None,
            judge_model: "gpt-4o".to_owned(),
            judge_api_key: None,
        }
    }

    #[test]
    fn validate_rejects_timeout_zero() {
        let mut a = base_args();
        a.timeout = 0;
        let err = validate_args(&a).unwrap_err();
        assert!(
            err.to_string().contains("--timeout must be greater than 0"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_rejects_max_questions_zero() {
        let mut a = base_args();
        a.max_questions = Some(0);
        let err = validate_args(&a).unwrap_err();
        assert!(err.to_string().contains("--max-questions"), "got: {err}");
    }

    #[test]
    fn validate_rejects_retrieval_k_zero() {
        let mut a = base_args();
        a.retrieval_k = Some(0);
        let err = validate_args(&a).unwrap_err();
        assert!(err.to_string().contains("--retrieval-k"), "got: {err}");
    }

    #[test]
    fn validate_rejects_empty_nous_id() {
        let mut a = base_args();
        a.nous_id = String::new();
        let err = validate_args(&a).unwrap_err();
        assert!(err.to_string().contains("--nous-id"), "got: {err}");
    }

    #[test]
    fn validate_rejects_whitespace_only_nous_id() {
        let mut a = base_args();
        a.nous_id = "   ".to_owned();
        let err = validate_args(&a).unwrap_err();
        assert!(err.to_string().contains("--nous-id"), "got: {err}");
    }

    #[test]
    fn validate_rejects_malformed_url() {
        let mut a = base_args();
        a.url = "not a url".to_owned();
        let err = validate_args(&a).unwrap_err();
        assert!(
            err.to_string().contains("--url is not a valid URL"),
            "got: {err}"
        );
    }

    #[test]
    fn validate_accepts_well_formed_args() {
        validate_args(&base_args()).unwrap();
        let mut a = base_args();
        a.url = "https://example.com:8443/path".to_owned();
        a.max_questions = Some(5);
        a.retrieval_k = Some(10);
        a.timeout = 1;
        validate_args(&a).unwrap();
    }

    #[test]
    fn validate_requires_gate_baseline_for_publishable_runs() {
        let mut a = base_args();
        a.publishable = true;
        let err = validate_args(&a).unwrap_err();
        assert!(
            err.to_string()
                .contains("--publishable requires --gate-baseline"),
            "got: {err}"
        );

        a.gate_baseline = Some(PathBuf::from("reviewed-gate.json"));
        validate_args(&a).unwrap();
    }

    fn sample_report() -> BenchmarkReport {
        BenchmarkReport::new(
            "LongMemEval",
            vec![
                QuestionResult {
                    id: "q1".to_owned(),
                    category: "factual".to_owned(),
                    status: QuestionStatus::Scored,
                    error_message: None,
                    actual_answer: "blue".to_owned(),
                    expected_answers: vec!["blue".to_owned()],
                    expected_evidence_refs: Vec::new(),
                    score: BenchmarkScore {
                        exact_match: true,
                        f1: 1.0,
                        contains: true,
                    },
                    judge_score: None,
                    retrieved_facts: None,
                    retrieval_scoring: None,
                    recall_at_k: None,
                    ndcg_at_k: None,
                },
                QuestionResult {
                    id: "q2".to_owned(),
                    category: "factual".to_owned(),
                    status: QuestionStatus::Scored,
                    error_message: None,
                    actual_answer: "green".to_owned(),
                    expected_answers: vec!["red".to_owned()],
                    expected_evidence_refs: Vec::new(),
                    score: BenchmarkScore {
                        exact_match: false,
                        f1: 0.0,
                        contains: false,
                    },
                    judge_score: None,
                    retrieved_facts: None,
                    retrieval_scoring: None,
                    recall_at_k: None,
                    ndcg_at_k: None,
                },
            ],
        )
    }

    fn first_two_questions_mut(
        report: &mut BenchmarkReport,
    ) -> (&mut QuestionResult, &mut QuestionResult) {
        let Some((first, rest)) = report.questions.split_first_mut() else {
            panic!("sample report must contain a first question");
        };
        let Some(second) = rest.first_mut() else {
            panic!("sample report must contain a second question");
        };
        (first, second)
    }

    fn sample_report_with_gate_metadata() -> BenchmarkReport {
        let mut report = sample_report();
        report.metadata = Some(BenchmarkMetadata {
            timestamp: "2026-06-01T00:00:00Z".to_owned(),
            aletheia_version: "0.1.0".to_owned(),
            nous_id: "benchmark".to_owned(),
            model: "fixture-model".to_owned(),
            benchmark: "LongMemEval".to_owned(),
            total_questions: 2,
            evaluated_questions: 2,
            timeout_secs: 120,
            dataset_hash: Some(
                "sha256:a6ecd7d5dadb9734f9cf28a590b99bfe527d906c08aaf3d356e7861661e74f10"
                    .to_owned(),
            ),
            git_sha: Some("0123456789abcdef".to_owned()),
            dataset_best_effort: false,
            dataset_validation: None,
        });
        let (first, second) = first_two_questions_mut(&mut report);
        first.recall_at_k = Some(1.0);
        second.recall_at_k = Some(0.5);
        first.ndcg_at_k = Some(1.0);
        second.ndcg_at_k = Some(0.5);
        report.judge_summary = Some(dokimion::benchmarks::JudgeSummary {
            attempted: 2,
            scored: 2,
            errors: 0,
            correct: 1,
        });
        report
    }

    fn sample_gate_baseline() -> BenchmarkGateBaseline {
        BenchmarkGateBaseline {
            version: 1,
            benchmark: "LongMemEval".to_owned(),
            provenance: BenchmarkGateProvenance {
                dataset_hash:
                    "sha256:a6ecd7d5dadb9734f9cf28a590b99bfe527d906c08aaf3d356e7861661e74f10"
                        .to_owned(),
                dataset_version: "fixture-smoke-v1".to_owned(),
                model: "fixture-model".to_owned(),
                source_report: "crates/aletheia/testdata/benchmarks/smoke-report.json".to_owned(),
                reviewed_at: "2026-06-01T00:00:00Z".to_owned(),
                reviewed_by: "benchmark-maintainers".to_owned(),
                git_sha: Some("0123456789abcdef".to_owned()),
            },
            metrics: BenchmarkGateMetrics {
                exact_match_rate: 0.5,
                mean_f1: 0.5,
                error_rate: 0.0,
                timeout_rate: 0.0,
                no_answer_rate: 0.0,
                recall_at_k: Some(0.75),
                ndcg_at_k: Some(0.75),
                judge_accuracy: Some(0.5),
                judge_error_rate: Some(0.0),
            },
            allowed_regression: BenchmarkGateMetrics {
                exact_match_rate: 0.0,
                mean_f1: 0.0,
                error_rate: 0.0,
                timeout_rate: 0.0,
                no_answer_rate: 0.0,
                recall_at_k: Some(0.0),
                ndcg_at_k: Some(0.0),
                judge_accuracy: Some(0.0),
                judge_error_rate: Some(0.0),
            },
            minimums: BenchmarkGateMinimums {
                scored_questions: 2,
                exact_match_rate: 0.5,
                mean_f1: 0.5,
                recall_at_k: Some(0.75),
                ndcg_at_k: Some(0.75),
                judge_accuracy: Some(0.5),
            },
            maximums: BenchmarkGateMaximums {
                errors: 0.0,
                timeouts: 0.0,
                no_answers: 0.0,
                judge_errors: Some(0.0),
            },
            require_retrieval: true,
            require_judge: true,
        }
    }

    #[test]
    fn benchmark_gate_passes_reviewed_baseline() {
        let report = sample_report_with_gate_metadata();
        let baseline = sample_gate_baseline();

        let gate = benchmark_gate_report(&report, &baseline).unwrap();

        assert!(gate.passed);
        assert!(gate.checks.iter().all(|check| check.passed));
    }

    #[test]
    fn benchmark_gate_fails_quality_regression() {
        let mut report = sample_report_with_gate_metadata();
        let Some(first) = report.questions.first_mut() else {
            panic!("sample report must contain a first question");
        };
        first.score.exact_match = false;
        first.score.f1 = 0.0;
        let baseline = sample_gate_baseline();

        let gate = benchmark_gate_report(&report, &baseline).unwrap();
        let err = require_gate_passed(&gate).unwrap_err();
        let message = err.to_string();

        assert!(!gate.passed);
        assert!(message.contains("exact_match_rate"), "got: {message}");
        assert!(message.contains("mean_f1"), "got: {message}");
    }

    #[test]
    fn benchmark_gate_fails_stale_dataset_provenance() {
        let mut report = sample_report_with_gate_metadata();
        report.metadata.as_mut().unwrap().dataset_hash = Some("sha256:new-dataset".to_owned());
        let baseline = sample_gate_baseline();

        let gate = benchmark_gate_report(&report, &baseline).unwrap();
        let failed_metrics = gate
            .checks
            .iter()
            .filter(|check| !check.passed)
            .map(|check| check.metric.as_str())
            .collect::<Vec<_>>();

        assert!(!gate.passed);
        assert!(failed_metrics.contains(&"dataset_hash"));
    }

    #[test]
    fn benchmark_gate_fails_when_required_retrieval_missing() {
        let mut report = sample_report_with_gate_metadata();
        for question in &mut report.questions {
            question.recall_at_k = None;
            question.ndcg_at_k = None;
        }
        let baseline = sample_gate_baseline();

        let gate = benchmark_gate_report(&report, &baseline).unwrap();
        let failed_metrics = gate
            .checks
            .iter()
            .filter(|check| !check.passed)
            .map(|check| check.metric.as_str())
            .collect::<Vec<_>>();

        assert!(!gate.passed);
        assert!(failed_metrics.contains(&"recall_at_k"));
        assert!(failed_metrics.contains(&"ndcg_at_k"));
    }

    #[test]
    fn reward_surface_uses_real_report_and_baseline_file() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("baseline.json");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(
            serde_json::json!({
                "benchmark": "LongMemEval",
                "exact_match_rate": 0.35,
                "mean_f1": 0.40
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();

        let report = sample_report();
        let surface = load_reward_surface(&report, &path).unwrap();

        assert!((surface.outcome.exact_match_rate - 0.5).abs() < f64::EPSILON);
        assert!((surface.reward - 0.15).abs() < f64::EPSILON);
        assert_eq!(
            format_reward_surface(&surface),
            "Reward vs baseline: +0.150 (EM 50.0% vs baseline 35.0%)"
        );
    }

    #[test]
    fn publishable_mode_rejects_point_estimate_only_report() {
        let report = sample_report();
        let err = require_publishable_report(&report).unwrap_err();

        assert!(
            err.to_string()
                .contains("missing bootstrap confidence intervals"),
            "got: {err}"
        );
    }
}
