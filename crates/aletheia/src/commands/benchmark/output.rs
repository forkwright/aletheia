use std::io::Write as _;
use std::path::Path;

use owo_colors::OwoColorize;
use snafu::prelude::*;

use dokimion::benchmarks::{BenchmarkComparisonReport, BenchmarkComparisonStatus, BenchmarkReport};
use episteme::rl::{LongMemEvalReward, MemoryOutcome, RewardFn};

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct RewardSurface {
    outcome: MemoryOutcome,
    baseline_exact_match_rate: f64,
    reward: f64,
}

pub(super) fn load_reward_surface(
    report: &BenchmarkReport,
    baseline_path: &Path,
) -> Result<RewardSurface> {
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

pub(super) fn write_stdout(output: &str) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(output.as_bytes())
        .whatever_context("failed to write benchmark output to stdout")
}

fn finish_lines(lines: &[String]) -> String {
    let mut output = lines.join("\n");
    output.push('\n');
    output
}

/// Render a human-readable benchmark report with per-category breakdown
/// and peer baseline comparison.
pub(super) fn render_report_human(
    report: &BenchmarkReport,
    reward_surface: Option<&RewardSurface>,
) -> String {
    let use_color = supports_color::on(supports_color::Stream::Stdout).is_some();
    let mut lines = Vec::new();

    render_report_header(report, use_color, &mut lines);
    render_results_table(report, use_color, &mut lines);
    render_statistics(report, &mut lines);
    render_publishability(report, &mut lines);
    render_comparisons(report, &mut lines);
    render_retrieval_metrics(report, &mut lines);
    render_judge_summary(report, &mut lines);

    if let Some(surface) = reward_surface {
        lines.push(String::new());
        lines.push(format_reward_surface(surface));
    }

    render_baselines(report, use_color, &mut lines);
    finish_lines(&lines)
}

fn render_report_header(report: &BenchmarkReport, use_color: bool, lines: &mut Vec<String>) {
    let header = if let Some(ref meta) = report.metadata {
        format!(
            "{} Benchmark \u{2014} {} ({})",
            report.benchmark, meta.nous_id, meta.model
        )
    } else {
        format!("{} Benchmark", report.benchmark)
    };
    if use_color {
        lines.push(header.bold().to_string());
    } else {
        lines.push(header.clone());
    }
    lines.push("\u{2550}".repeat(header.len()));

    if let Some(ref meta) = report.metadata {
        lines.push(format!(
            "Version: {} | Questions: {}/{} | Timeout: {}s",
            meta.aletheia_version,
            meta.evaluated_questions,
            meta.total_questions,
            meta.timeout_secs
        ));
        if meta.dataset_best_effort {
            let warnings = meta
                .dataset_validation
                .as_ref()
                .map_or(0, |validation| validation.warnings.len());
            lines.push(format!(
                "Dataset validation: best-effort ({warnings} warning(s))"
            ));
        }
    } else {
        lines.push(format!("Questions: {}", report.total));
    }
    lines.push(format!(
        "Attempted: {} | Scored: {} | Errors: {} | Timeouts: {} | No answer: {}",
        report.total, report.scored, report.errors, report.timeouts, report.no_answers
    ));
    lines.push(String::new());
}

fn render_results_table(report: &BenchmarkReport, use_color: bool, lines: &mut Vec<String>) {
    lines.push("Results:".to_owned());
    lines.push(format!("  {:<30} {:>6} {:>6}", "Category", "EM%", "F1%"));
    lines.push(format!("  {}", "\u{2500}".repeat(44)));

    let categories = report.per_category();
    for (cat, em, f1) in &categories {
        let em_pct = em * 100.0;
        let f1_pct = f1 * 100.0;
        if use_color {
            lines.push(format!(
                "  {:<30} {:>5.1}  {:>5.1}",
                cat,
                em_pct.yellow(),
                f1_pct.yellow()
            ));
        } else {
            lines.push(format!("  {cat:<30} {em_pct:>5.1}  {f1_pct:>5.1}"));
        }
    }

    let overall_em = report.exact_match_rate() * 100.0;
    let overall_f1 = report.mean_f1() * 100.0;
    lines.push(format!("  {}", "\u{2500}".repeat(44)));
    if use_color {
        lines.push(format!(
            "  {:<30} {:>5.1}  {:>5.1}",
            "Overall".bold(),
            format!("{overall_em:.1}").green().bold(),
            format!("{overall_f1:.1}").green().bold()
        ));
    } else {
        lines.push(format!(
            "  {:<30} {:>5.1}  {:>5.1}",
            "Overall", overall_em, overall_f1
        ));
    }
}

fn render_retrieval_metrics(report: &BenchmarkReport, lines: &mut Vec<String>) {
    if let Some(recall) = report.mean_recall_at_k() {
        let ndcg = report.mean_ndcg_at_k().unwrap_or(0.0);
        lines.push(String::new());
        lines.push(format!(
            "Retrieval metrics (k={}):",
            args_retrieval_k(report)
        ));
        lines.push(format!(
            "  Mean Recall@k: {recall:.3} | Mean NDCG@k: {ndcg:.3}"
        ));
    }
}

fn render_judge_summary(report: &BenchmarkReport, lines: &mut Vec<String>) {
    if let (Some(judge_acc), Some(summary)) = (report.judge_accuracy(), report.judge_summary) {
        lines.push(String::new());
        lines.push(format!(
            "LLM-as-judge accuracy: {:.1}% ({} correct / {} attempted; {} parsed, {} errors)",
            judge_acc * 100.0,
            summary.correct,
            summary.attempted,
            summary.scored,
            summary.errors
        ));
    }
}

fn render_statistics(report: &BenchmarkReport, lines: &mut Vec<String>) {
    if let Some(statistics) = &report.statistics {
        lines.push(String::new());
        lines.push(format!(
            "Statistics (95% bootstrap CI, {} resamples):",
            statistics.n_resamples
        ));
        lines.push(format!(
            "  EM: {:.1}% [{:.1}, {:.1}]",
            report.exact_match_rate() * 100.0,
            statistics.em_ci_low * 100.0,
            statistics.em_ci_high * 100.0
        ));
        lines.push(format!(
            "  F1: {:.1}% [{:.1}, {:.1}]",
            report.mean_f1() * 100.0,
            statistics.f1_ci_low * 100.0,
            statistics.f1_ci_high * 100.0
        ));
        lines.push(format!("  Method: {}", statistics.method));
    } else {
        lines.push(String::new());
        lines.push(format!(
            "Statistics: unavailable (requires at least {} scored questions)",
            dokimion::benchmarks::MIN_PUBLISHABLE_SCORED_QUESTIONS
        ));
    }
}

fn render_publishability(report: &BenchmarkReport, lines: &mut Vec<String>) {
    let Some(assessment) = &report.publishability else {
        return;
    };
    lines.push(String::new());
    if assessment.publishable {
        lines.push("Publishability: publishable".to_owned());
        return;
    }

    lines.push("Publishability: not publishable".to_owned());
    for reason in &assessment.reasons {
        lines.push(format!("  - {reason}"));
    }
}

fn render_comparisons(report: &BenchmarkReport, lines: &mut Vec<String>) {
    if report.comparisons.is_empty() {
        return;
    }

    lines.push(String::new());
    lines.push("Baseline/candidate statistics:".to_owned());
    for comparison in &report.comparisons {
        render_comparison(comparison, lines);
    }
}

fn render_comparison(comparison: &BenchmarkComparisonReport, lines: &mut Vec<String>) {
    if let (BenchmarkComparisonStatus::Complete, Some(statistics)) =
        (&comparison.status, &comparison.statistics)
    {
        lines.push(format!(
            "  {}: baseline {:.3}, candidate {:.3}, d={} ({})",
            comparison.metric,
            statistics.mean_a,
            statistics.mean_b,
            format_float(statistics.effect.d),
            statistics.effect.interpretation
        ));
        lines.push(format!(
            "      baseline CI [{}, {}] | candidate CI [{}, {}] | p_raw={} | p_fdr={}",
            format_float(statistics.ci_a.ci_low),
            format_float(statistics.ci_a.ci_high),
            format_float(statistics.ci_b.ci_low),
            format_float(statistics.ci_b.ci_high),
            format_float(statistics.p_raw),
            statistics
                .p_adjusted
                .map_or_else(|| "n/a".to_owned(), format_float)
        ));
    } else {
        let reason = comparison
            .reason
            .as_deref()
            .unwrap_or("comparison statistics are incomplete");
        lines.push(format!(
            "  {}: {} ({reason})",
            comparison.metric, comparison.status
        ));
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
    report
        .questions
        .iter()
        .find(|q| q.recall_at_k.is_some())
        .and_then(|q| q.retrieved_facts.as_ref().map(Vec::len))
        .unwrap_or(0)
}

/// Render peer baseline comparison table.
fn render_baselines(report: &BenchmarkReport, use_color: bool, lines: &mut Vec<String>) {
    let baselines = match report.benchmark.as_str() {
        "LongMemEval" => dokimion::benchmarks::baselines::longmemeval_baselines(),
        "LoCoMo" => dokimion::benchmarks::baselines::locomo_baselines(),
        _ => return,
    };

    lines.push(String::new());
    lines.push("Peer baselines:".to_owned());
    lines.push(format!("  {:<28} {:>8} {:>8}", "System", "EM%", "F1%"));
    lines.push(format!("  {}", "\u{2500}".repeat(46)));

    for baseline in &baselines {
        let em_str = baseline
            .exact_match_rate
            .map_or_else(|| "-".to_owned(), |v| format!("{:.1}", v * 100.0));
        let f1_str = baseline
            .mean_f1
            .map_or_else(|| "-".to_owned(), |v| format!("{:.1}", v * 100.0));
        if use_color {
            lines.push(format!(
                "  {:<28} {:>8} {:>8}  {}",
                baseline.system.dimmed(),
                em_str.dimmed(),
                f1_str.dimmed(),
                baseline.note.dimmed()
            ));
        } else {
            lines.push(format!(
                "  {:<28} {:>8} {:>8}  {}",
                baseline.system, em_str, f1_str, baseline.note
            ));
        }
    }
}

/// Render the benchmark report as JSON for machine consumption.
pub(super) fn render_report_json(
    report: &BenchmarkReport,
) -> std::result::Result<String, serde_json::Error> {
    let mut json = serde_json::to_string_pretty(report)?;
    json.push('\n');
    Ok(json)
}

#[cfg(test)]
mod tests;
