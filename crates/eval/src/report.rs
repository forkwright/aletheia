//! Terminal output formatting for eval reports.

use owo_colors::OwoColorize;
use serde::Serialize;
use tracing::info;

use crate::provenance::EvalProvenance;
use crate::runner::RunReport;
use crate::scenario::{ScenarioClassification, ScenarioOutcome, ScenarioSubResult};

/// Print a human-readable eval report to stdout.
#[tracing::instrument(skip_all)]
pub fn print_report(report: &RunReport, base_url: &str) {
    let use_color = supports_color::on(supports_color::Stream::Stdout).is_some();

    if use_color {
        info!("{} — {}", "Behavioral Eval".bold(), base_url.dimmed());
    } else {
        info!("Behavioral Eval — {base_url}");
    }
    info!("{}", "\u{2501}".repeat(39));
    info!("");

    let mut current_category = "";

    for result in &report.results {
        if result.meta.category != current_category {
            current_category = result.meta.category;
            if use_color {
                info!("  {}:", current_category.bold());
            } else {
                info!("  {current_category}:");
            }
        }

        match &result.outcome {
            ScenarioOutcome::Passed { duration } => {
                let ms = duration.as_millis();
                if use_color {
                    info!(
                        "    {}  {:<40} {}",
                        "PASS".green(),
                        result.meta.id,
                        format!("{ms}ms").dimmed()
                    );
                } else {
                    info!("    PASS  {:<40} {ms}ms", result.meta.id);
                }
            }
            ScenarioOutcome::Failed { duration, error } => {
                let ms = duration.as_millis();
                if use_color {
                    info!(
                        "    {}  {:<40} {}",
                        "FAIL".red(),
                        result.meta.id,
                        format!("{ms}ms").dimmed()
                    );
                    info!("          {}", error.to_string().red());
                } else {
                    info!("    FAIL  {:<40} {ms}ms", result.meta.id);
                    info!("          {error}");
                }
            }
            ScenarioOutcome::Skipped { reason } => {
                if use_color {
                    info!("    {}  {}", "SKIP".yellow(), result.meta.id,);
                    info!("          {}", reason.dimmed());
                } else {
                    info!("    SKIP  {}", result.meta.id);
                    info!("          {reason}");
                }
            }
        }

        for sub in &result.sub_results {
            let sub_icon = if sub.passed { "  ✓" } else { "  ✗" };
            let sub_label = format_sub_result(sub);
            info!("      {sub_icon} {sub_label}");
        }
    }

    info!("");
    info!("{}", "\u{2501}".repeat(39));

    let total_secs = report.total_duration.as_secs_f64();
    let summary = format!(
        "{} passed, {} failed, {} skipped ({total_secs:.1}s)",
        report.passed, report.failed, report.skipped
    );

    if use_color {
        if report.failed > 0 {
            info!("{}", summary.red().bold());
        } else {
            info!("{}", summary.green().bold());
        }
    } else {
        info!("{summary}");
    }
}

fn format_sub_result(sub: &ScenarioSubResult) -> String {
    let class = match sub.classification {
        ScenarioClassification::Assertive => "assertive",
        ScenarioClassification::Smoke => "smoke",
        ScenarioClassification::Informational => "informational",
    };
    if let Some(criteria) = &sub.criteria {
        format!("{} ({class}): {criteria}", sub.sub_id)
    } else {
        format!("{} ({class})", sub.sub_id)
    }
}

/// Print the report as JSON for machine consumption.
#[tracing::instrument(skip_all)]
pub fn print_report_json(report: &RunReport) {
    let json_report = build_json_report(report);

    match serde_json::to_string_pretty(&json_report) {
        Ok(json) => info!("{json}"),
        Err(e) => tracing::error!(error = %e, "failed to serialize eval report as JSON"),
    }
}

/// Render an eval report to PDF via poiesis-typst using the `eval-report` template.
///
/// Transforms the `RunReport` into a JSON schema suitable for the Typst template
/// and returns PDF bytes.
///
/// # Errors
///
/// Returns an error if JSON serialization fails or if the Typst render fails.
#[tracing::instrument(skip_all)]
pub fn emit_eval_report(report: &RunReport) -> crate::error::Result<Vec<u8>> {
    let json_report = build_json_report(report);

    let data = serde_json::json!({
        "summary": {
            "passed": json_report.passed,
            "failed": json_report.failed,
            "skipped": json_report.skipped,
            "total_duration_ms": json_report.total_duration_ms,
        },
        "benchmarks": json_report.results
    });

    poiesis_typst::render_template("eval-report", &data).map_err(|e| {
        crate::error::BenchmarkSnafu {
            message: format!("eval report render failed: {e}"),
        }
        .build()
    })
}

fn build_json_report(report: &RunReport) -> JsonReport {
    JsonReport {
        eval_run_id: report.provenance.eval_run_id.clone(),
        provenance: report.provenance.clone(),
        passed: report.passed,
        failed: report.failed,
        skipped: report.skipped,
        total_duration_ms: u64::try_from(report.total_duration.as_millis()).unwrap_or(u64::MAX),
        results: report
            .results
            .iter()
            .map(|r| JsonScenarioResult {
                id: r.meta.id.to_owned(),
                category: r.meta.category.to_owned(),
                classification: r.meta.classification,
                criteria: r.meta.criteria_summary(),
                outcome: match &r.outcome {
                    ScenarioOutcome::Passed { .. } => OutcomeKind::Passed,
                    ScenarioOutcome::Failed { .. } => OutcomeKind::Failed,
                    ScenarioOutcome::Skipped { .. } => OutcomeKind::Skipped,
                },
                duration_ms: match &r.outcome {
                    ScenarioOutcome::Passed { duration }
                    | ScenarioOutcome::Failed { duration, .. } => {
                        Some(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
                    }
                    ScenarioOutcome::Skipped { .. } => None,
                },
                error: match &r.outcome {
                    ScenarioOutcome::Failed { error, .. } => Some(error.to_string()),
                    _ => None,
                },
                skip_reason: match &r.outcome {
                    ScenarioOutcome::Skipped { reason } => Some(reason.clone()),
                    _ => None,
                },
                sub_results: r.sub_results.clone(),
            })
            .collect(),
    }
}

/// Typed outcome kind for JSON serialization: avoids bare "passed"/"failed"/"skipped" strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub(crate) enum OutcomeKind {
    Passed,
    Failed,
    Skipped,
}

#[derive(Serialize)]
struct JsonReport {
    eval_run_id: String,
    provenance: EvalProvenance,
    passed: usize,
    failed: usize,
    skipped: usize,
    total_duration_ms: u64,
    results: Vec<JsonScenarioResult>,
}

#[derive(Serialize)]
struct JsonScenarioResult {
    id: String,
    category: String,
    classification: ScenarioClassification,
    #[serde(skip_serializing_if = "Option::is_none")]
    criteria: Option<String>,
    outcome: OutcomeKind,
    duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    skip_reason: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    sub_results: Vec<ScenarioSubResult>,
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::time::Duration;

    use crate::runner::RunReport;
    use crate::scenario::{ScenarioClassification, ScenarioMeta, ScenarioOutcome, ScenarioResult};

    use super::*;

    fn sample_report() -> RunReport {
        RunReport {
            passed: 2,
            failed: 1,
            skipped: 1,
            total_duration: Duration::from_millis(1234),
            results: vec![
                ScenarioResult {
                    meta: ScenarioMeta {
                        id: "health-ok",
                        description: "health endpoint returns ok",
                        category: "health",
                        requires_auth: false,
                        requires_nous: false,
                        expected_contains: None,
                        expected_pattern: None,
                        classification: ScenarioClassification::Smoke,
                    },
                    outcome: ScenarioOutcome::Passed {
                        duration: Duration::from_millis(50),
                    },
                    sub_results: vec![],
                },
                ScenarioResult {
                    meta: ScenarioMeta {
                        id: "session-create",
                        description: "session creation works",
                        category: "session",
                        requires_auth: true,
                        requires_nous: true,
                        expected_contains: None,
                        expected_pattern: None,
                        classification: ScenarioClassification::Assertive,
                    },
                    outcome: ScenarioOutcome::Failed {
                        duration: Duration::from_millis(200),
                        error: crate::error::AssertionSnafu {
                            message: "status mismatch",
                        }
                        .build(),
                    },
                    sub_results: vec![],
                },
            ],
            provenance: EvalProvenance::new("er-sample", "http://localhost"),
        }
    }

    #[test]
    fn json_report_serializes() {
        let report = sample_report();
        let json_report = build_json_report(&report);
        let json = serde_json::to_string(&json_report).expect("serialization should succeed");
        assert!(!json.is_empty());
    }

    #[test]
    fn json_report_contains_expected_fields() {
        let report = sample_report();
        let json_report = build_json_report(&report);
        let json = serde_json::to_string_pretty(&json_report).expect("serialize");
        assert!(json.contains("\"passed\""));
        assert!(json.contains("\"failed\""));
        assert!(json.contains("\"skipped\""));
        assert!(json.contains("\"total_duration_ms\""));
        assert!(json.contains("\"results\""));
        assert!(json.contains("health-ok"));
        assert!(json.contains("eval_run_id"));
        assert!(json.contains("classification"));
    }

    #[test]
    fn outcome_kind_serializes_to_lowercase_string() {
        assert_eq!(
            serde_json::to_string(&OutcomeKind::Passed).expect("serialize"),
            "\"passed\""
        );
        assert_eq!(
            serde_json::to_string(&OutcomeKind::Failed).expect("serialize"),
            "\"failed\""
        );
        assert_eq!(
            serde_json::to_string(&OutcomeKind::Skipped).expect("serialize"),
            "\"skipped\""
        );
    }

    #[test]
    fn outcome_kind_equality() {
        assert_eq!(OutcomeKind::Passed, OutcomeKind::Passed);
        assert_ne!(OutcomeKind::Passed, OutcomeKind::Failed);
        assert_ne!(OutcomeKind::Failed, OutcomeKind::Skipped);
    }

    #[test]
    fn emit_eval_report_round_trip() {
        let report = sample_report();
        let pdf_bytes = emit_eval_report(&report).expect("emit_eval_report must not fail");
        assert!(pdf_bytes.starts_with(b"%PDF-"), "output must be PDF magic");
        assert!(pdf_bytes.len() > 500, "PDF must be >500 bytes");
        assert!(pdf_bytes.len() < 5_000_000, "PDF must be <5MB");
    }

    #[test]
    fn json_report_includes_sub_results() {
        let mut report = sample_report();
        report
            .results
            .first_mut()
            .expect("sample report has a first result")
            .sub_results
            .push(ScenarioSubResult {
                sub_id: "probe-1".to_owned(),
                classification: ScenarioClassification::Assertive,
                passed: true,
                criteria: Some("forbidden patterns".to_owned()),
                response_excerpt: None,
                violation_ids: vec![],
            });
        let json_report = build_json_report(&report);
        let result = json_report.results.first().expect("first JSON result");
        assert_eq!(result.sub_results.len(), 1);
    }
}
