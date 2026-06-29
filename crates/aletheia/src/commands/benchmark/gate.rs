use std::path::Path;

use serde::{Deserialize, Serialize};
use snafu::prelude::*;

use dokimion::benchmarks::BenchmarkReport;

use crate::error::Result;

use super::{BenchmarkGateArgs, load_benchmark_report};

pub(super) async fn run(args: BenchmarkGateArgs) -> Result<()> {
    let candidate = load_benchmark_report(&args.candidate_report).await?;
    let baseline = load_gate_baseline(&args.baseline).await?;
    let gate_report = benchmark_gate_report(&candidate, &baseline)?;
    print_gate_report(&gate_report, args.json).whatever_context("failed to print gate report")?;
    require_gate_passed(&gate_report)
}

pub(super) async fn enforce_report(report: &BenchmarkReport, baseline_path: &Path) -> Result<()> {
    let baseline = load_gate_baseline(baseline_path).await?;
    let gate_report = benchmark_gate_report(report, &baseline)?;
    print_gate_report(&gate_report, false).whatever_context("failed to print gate report")?;
    require_gate_passed(&gate_report)
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

#[cfg(test)]
mod tests;
