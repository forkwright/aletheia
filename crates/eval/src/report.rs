//! Terminal output formatting for eval reports.

use owo_colors::OwoColorize;
use serde::Serialize;

use crate::runner::RunReport;
use crate::scenario::ScenarioOutcome;

/// Print a human-readable eval report to stdout.
#[tracing::instrument(skip_all)]
pub fn print_report(report: &RunReport, base_url: &str) {
    let use_color = supports_color::on(supports_color::Stream::Stdout).is_some();

    if use_color {
        println!("{} — {}", "Behavioral Eval".bold(), base_url.dimmed());
    } else {
        println!("Behavioral Eval — {base_url}");
    }
    println!("{}", "\u{2501}".repeat(39));
    println!();

    let mut current_category = "";

    for result in &report.results {
        if result.meta.category != current_category {
            current_category = result.meta.category;
            if use_color {
                println!("  {}:", current_category.bold());
            } else {
                println!("  {current_category}:");
            }
        }

        match &result.outcome {
            ScenarioOutcome::Passed { duration } => {
                let ms = duration.as_millis();
                if use_color {
                    println!(
                        "    {}  {:<40} {}",
                        "PASS".green(),
                        result.meta.id,
                        format!("{ms}ms").dimmed()
                    );
                } else {
                    println!("    PASS  {:<40} {ms}ms", result.meta.id);
                }
            }
            ScenarioOutcome::Failed { duration, error } => {
                let ms = duration.as_millis();
                if use_color {
                    println!(
                        "    {}  {:<40} {}",
                        "FAIL".red(),
                        result.meta.id,
                        format!("{ms}ms").dimmed()
                    );
                    println!("          {}", error.to_string().red());
                } else {
                    println!("    FAIL  {:<40} {ms}ms", result.meta.id);
                    println!("          {error}");
                }
            }
            ScenarioOutcome::Skipped { reason } => {
                if use_color {
                    println!("    {}  {}", "SKIP".yellow(), result.meta.id,);
                    println!("          {}", reason.dimmed());
                } else {
                    println!("    SKIP  {}", result.meta.id);
                    println!("          {reason}");
                }
            }
        }
    }

    println!();
    println!("{}", "\u{2501}".repeat(39));

    let total_secs = report.total_duration.as_secs_f64();
    let summary = format!(
        "{} passed, {} failed, {} skipped ({total_secs:.1}s)",
        report.passed, report.failed, report.skipped
    );

    if use_color {
        if report.failed > 0 {
            println!("{}", summary.red().bold());
        } else {
            println!("{}", summary.green().bold());
        }
    } else {
        println!("{summary}");
    }
}

/// Print the report as JSON for machine consumption.
#[tracing::instrument(skip_all)]
pub fn print_report_json(report: &RunReport) {
    let json_report = JsonReport {
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
            })
            .collect(),
    };

    match serde_json::to_string_pretty(&json_report) {
        Ok(json) => println!("{json}"),
        Err(e) => tracing::error!(error = %e, "failed to serialize eval report as JSON"),
    }
}

/// Typed outcome kind for JSON serialization: avoids bare "passed"/"failed"/"skipped" strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum OutcomeKind {
    Passed,
    Failed,
    Skipped,
}

#[derive(Serialize)]
struct JsonReport {
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
    outcome: OutcomeKind,
    duration_ms: Option<u64>,
    error: Option<String>,
    skip_reason: Option<String>,
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::time::Duration;

    use crate::runner::RunReport;
    use crate::scenario::{ScenarioMeta, ScenarioOutcome, ScenarioResult};

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
                    },
                    outcome: ScenarioOutcome::Passed {
                        duration: Duration::from_millis(50),
                    },
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
                    },
                    outcome: ScenarioOutcome::Failed {
                        duration: Duration::from_millis(200),
                        error: crate::error::AssertionSnafu {
                            message: "status mismatch",
                        }
                        .build(),
                    },
                },
            ],
        }
    }

    #[test]
    fn json_report_serializes() {
        let report = sample_report();
        let json_report = JsonReport {
            passed: report.passed,
            failed: report.failed,
            skipped: report.skipped,
            total_duration_ms: u64::try_from(report.total_duration.as_millis()).unwrap_or(u64::MAX),
            results: vec![],
        };
        let json = serde_json::to_string(&json_report).expect("serialization should succeed");
        assert!(!json.is_empty());
    }

    #[test]
    fn json_report_contains_expected_fields() {
        let report = sample_report();
        let json_report = JsonReport {
            passed: report.passed,
            failed: report.failed,
            skipped: report.skipped,
            total_duration_ms: u64::try_from(report.total_duration.as_millis()).unwrap_or(u64::MAX),
            results: vec![JsonScenarioResult {
                id: "health-ok".to_owned(),
                category: "health".to_owned(),
                outcome: OutcomeKind::Passed,
                duration_ms: Some(50),
                error: None,
                skip_reason: None,
            }],
        };
        let json = serde_json::to_string_pretty(&json_report).expect("serialize");
        assert!(json.contains("\"passed\""));
        assert!(json.contains("\"failed\""));
        assert!(json.contains("\"skipped\""));
        assert!(json.contains("\"total_duration_ms\""));
        assert!(json.contains("\"results\""));
        assert!(json.contains("health-ok"));
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
}
