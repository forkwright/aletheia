//! `aletheia benchmark`: run memory benchmarks (`LongMemEval`, `LoCoMo`) against a
//! live instance.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Subcommand};
use owo_colors::OwoColorize;
use serde::Serialize;
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

    // Collect system metadata before running.
    let metadata = collect_metadata(&client, benchmark, args, validation).await;
    let config_hash = dokimion::provenance::sha256_hex_str(&format!(
        "benchmark={}\ndataset={}\nurl={}\nnous_id={}\nmax_questions={:?}\ntimeout={}\njson={}\nretrieval_k={:?}\nbest_effort_dataset={}\nbaseline_report={:?}\npublishable={}\njudge_endpoint_present={}\njudge_model={}\njudge_api_key_present={}",
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
        args.publishable,
        args.judge_endpoint.is_some(),
        args.judge_model,
        args.judge_api_key.is_some(),
    ));
    let cli_args: Vec<String> = std::env::args().collect();
    let mut provenance = dokimion::provenance::EvalProvenance::new(
        dokimion::provenance::generate_eval_run_id(),
        args.url.clone(),
    )
    .with_redacted_args(&cli_args)
    .with_config_hash(config_hash)
    .with_target_identity(metadata.aletheia_version.clone())
    .with_audit_refs(Some(metadata.model.clone()), None, None, None, None);
    if let Some(git_sha) = metadata.git_sha.clone() {
        provenance = provenance.with_git_sha(git_sha);
    }
    if let Some(dataset_hash) = metadata.dataset_hash.clone() {
        provenance = provenance.with_scenario_suite_hash(dataset_hash);
    }

    let judge =
        args.judge_endpoint
            .as_ref()
            .map(|endpoint| dokimion::benchmarks::judge::LlmJudgeConfig {
                endpoint: endpoint.clone(),
                model: args.judge_model.clone(),
                api_key: args.judge_api_key.clone(),
                max_tokens: 256,
                temperature: 0.0,
                timeout: Duration::from_secs(args.timeout),
            });

    let config = BenchmarkRunnerConfig {
        nous_id: args.nous_id.clone(),
        session_key_prefix: format!("bench-{}", benchmark.name().to_lowercase()),
        question_timeout: Duration::from_secs(args.timeout),
        max_questions: args.max_questions,
        close_between_questions: true,
        judge,
        retrieval_k: args.retrieval_k,
        provenance,
    };
    let runner = BenchmarkRunner::new(client, config);
    let mut report = runner
        .run(benchmark)
        .await
        .whatever_context("benchmark run failed")?;
    report.metadata = Some(metadata);
    report = report.with_standard_statistics();

    if let Some(ref path) = args.baseline_report {
        let baseline_report = load_benchmark_report(path).await?;
        report = report.with_comparisons_against(&baseline_report, "baseline_vs_candidate");
    }

    // Write to file if --output was provided.
    if let Some(ref path) = args.output {
        let json =
            serde_json::to_string_pretty(&report).whatever_context("failed to serialize report")?;
        tokio::fs::write(path, json)
            .await
            .whatever_context("failed to write report file")?;
        println!("Report written to {}", path.display());
    }

    if args.publishable {
        require_publishable_report(&report)?;
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

async fn load_benchmark_report(path: &Path) -> Result<BenchmarkReport> {
    let json = tokio::fs::read_to_string(path)
        .await
        .whatever_context("failed to read baseline benchmark report")?;
    serde_json::from_str(&json).whatever_context("failed to parse baseline benchmark report")
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
