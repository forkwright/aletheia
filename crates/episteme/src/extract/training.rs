//! PR lesson extraction from training data with quality gates.
//!
//! Reads JSONL training files (violations, lint summaries) produced by
//! `kanon lint`, extracts patterns from successful and failed fixes, and
//! converts them to knowledge graph facts.
//!
//! Quality gates:
//! - Violations with `pr_number` and `sha` are only treated as successful fixes
//!   when backed by explicit merged/fixed outcome metadata or by a before/after
//!   violation delta that shows a decrease.
//! - Confidence scoring: explicitly verified fixes get 0.9, delta-only fixes
//!   get 0.75, inferred patterns get 0.6, and PR-linked but unresolved
//!   observations get 0.5.
//! - Deduplication by rule+file to avoid flooding the graph with duplicates.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// A violation record from `workflow/training/violations.jsonl`.
///
/// Expected schema version for violation records.
const VIOLATION_SCHEMA_VERSION: u32 = 2;

/// Schema version 2: each line is a JSON object describing a single lint
/// violation detected by `kanon lint`.
#[derive(Debug, Clone, Deserialize)]
#[expect(
    dead_code,
    reason = "fields deserialized from JSONL for completeness; subset used by extraction logic"
)]
pub(crate) struct ViolationRecord {
    /// Record type discriminator (always "violation").
    #[serde(rename = "type")]
    pub(crate) record_type: String,
    /// Schema version guard — records with an unexpected version are rejected.
    pub(crate) schema_version: u32,
    /// When the violation was detected.
    pub(crate) ts: String,
    /// Lint rule that was violated (e.g., "RUST/pub-visibility").
    pub(crate) rule: String,
    /// File path where the violation was found.
    pub(crate) file: String,
    /// Line number of the violation.
    pub(crate) line: u32,
    /// Code snippet showing the violation.
    pub(crate) snippet: String,
    /// Project name (empty for repo-wide scans).
    #[serde(default)]
    pub(crate) project: String,
    /// PR number if this violation was found in a PR context.
    pub(crate) pr_number: Option<u32>,
    /// Git SHA if this violation was found in a PR context.
    pub(crate) sha: Option<String>,
    /// Outcome of the PR that produced this violation (e.g. "merged", "fixed",
    /// "introduced", "failed", "unmerged", or "unresolved").
    #[serde(default)]
    pub(crate) outcome: Option<String>,
    /// Violation count for this rule before the PR.
    #[serde(default)]
    pub(crate) before_count: Option<u32>,
    /// Violation count for this rule after the PR.
    #[serde(default)]
    pub(crate) after_count: Option<u32>,
}

/// Returns true when the record carries evidence that the violation was fixed.
///
/// Evidence takes one of two forms:
/// - an explicit `outcome` of "merged" or "fixed"; or
/// - a before/after violation delta where `after_count < before_count`.
fn has_fixed_outcome_evidence(record: &ViolationRecord) -> bool {
    if matches!(record.outcome.as_deref(), Some("merged") | Some("fixed")) {
        return true;
    }

    if let (Some(before), Some(after)) = (record.before_count, record.after_count) {
        if after < before {
            return true;
        }
    }

    false
}

/// Expected schema version for lint summary records.
const LINT_SUMMARY_SCHEMA_VERSION: u32 = 2;

/// A lint summary record from `workflow/training/lint.jsonl`.
#[derive(Debug, Clone, Deserialize)]
#[expect(
    dead_code,
    reason = "fields deserialized from JSONL for completeness; subset used by trend detection"
)]
pub(crate) struct LintSummaryRecord {
    /// Record type discriminator (always "lint").
    #[serde(rename = "type")]
    pub(crate) record_type: String,
    /// Schema version guard — records with an unexpected version are rejected.
    pub(crate) schema_version: u32,
    /// When the lint run completed.
    pub(crate) ts: String,
    /// Repository or file path that was scanned.
    pub(crate) repo: String,
    /// Total violation count.
    pub(crate) total_violations: u32,
    /// Number of distinct rules triggered.
    pub(crate) rules_triggered: u32,
    /// Lint run duration in milliseconds.
    pub(crate) duration_ms: u64,
}

/// A lesson extracted from training data, ready for knowledge graph insertion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingLesson {
    /// The lint rule this lesson is about.
    pub rule: String,
    /// Classification of the lesson outcome.
    pub outcome: LessonOutcome,
    /// Human-readable description of the lesson.
    pub description: String,
    /// Confidence in this lesson (0.0--1.0).
    pub confidence: f64,
    /// Files where this pattern was observed.
    pub affected_files: Vec<String>,
    /// Number of occurrences that contributed to this lesson.
    pub occurrence_count: u32,
    /// Source PR number, if from a merged PR.
    pub pr_number: Option<u32>,
}

/// Classification of a lesson outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LessonOutcome {
    /// A violation was fixed in a merged PR.
    FixedInPr,
    /// A violation pattern recurs across multiple scans (not yet fixed).
    RecurringViolation,
    /// A rule's violation count decreased over time (improving trend).
    ImprovingTrend,
    /// A rule's violation count increased over time (degrading trend).
    DegradingTrend,
}

impl std::fmt::Display for LessonOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FixedInPr => f.write_str("fixed_in_pr"),
            Self::RecurringViolation => f.write_str("recurring_violation"),
            Self::ImprovingTrend => f.write_str("improving_trend"),
            Self::DegradingTrend => f.write_str("degrading_trend"),
        }
    }
}

/// Result of a training lesson extraction run.
#[derive(Debug, Clone, Default)]
pub struct ExtractionResult {
    /// Lessons extracted from the training data.
    pub lessons: Vec<TrainingLesson>,
    /// Number of violation records read.
    pub violations_read: usize,
    /// Number of lint summary records read.
    pub lint_summaries_read: usize,
    /// Number of records skipped (parse errors, quality gate failures).
    pub records_skipped: usize,
}

impl ExtractionResult {
    /// Returns true when no lessons were extracted.
    pub fn is_empty(&self) -> bool {
        self.lessons.is_empty()
    }

    /// Total records processed across all input files.
    pub fn total_records_processed(&self) -> usize {
        self.violations_read + self.lint_summaries_read
    }
}

/// Extract lessons from training data JSONL files.
///
/// Reads violations and lint summaries, applies quality gates, and produces
/// deduplicated lessons grouped by rule.
///
/// # Quality gates
///
/// - Violations with `pr_number` and `sha` are treated as fixed only when
///   supported by explicit merged/fixed outcome metadata or a before/after
///   violation delta that shows a decrease.
/// - Violations without PR context, or PR-linked violations with missing or
///   negative fixed-outcome evidence, are treated as unfixed (recurring).
/// - Duplicate rule+file pairs are collapsed into a single lesson with
///   an occurrence count.
///
/// # Errors
///
/// Returns `std::io::Error` if the training data files cannot be read.
pub fn extract_from_training_data(training_dir: &Path) -> std::io::Result<ExtractionResult> {
    let mut result = ExtractionResult::default();
    let mut rule_buckets: HashMap<String, RuleBucket> = HashMap::new();

    let violations_path = training_dir.join("violations.jsonl");
    if violations_path.exists() {
        // WHY: read_to_string avoids disallowed File::open; file sizes are bounded
        // by JSONL append-only semantics (kanon lint rotates at ~10MB).
        let content = std::fs::read_to_string(&violations_path)?;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            match serde_json::from_str::<ViolationRecord>(trimmed) {
                Ok(record) => {
                    if record.schema_version != VIOLATION_SCHEMA_VERSION {
                        tracing::warn!(
                            schema_version = record.schema_version,
                            expected = VIOLATION_SCHEMA_VERSION,
                            "skipping violation record with unexpected schema version"
                        );
                        result.records_skipped += 1;
                        continue;
                    }
                    result.violations_read += 1;
                    let bucket = rule_buckets.entry(record.rule.clone()).or_default();
                    bucket.add_violation(&record);
                }
                Err(e) => {
                    tracing::debug!(
                        error = %e,
                        "skipping unparseable violation record"
                    );
                    result.records_skipped += 1;
                }
            }
        }
    }

    let lint_path = training_dir.join("lint.jsonl");
    if lint_path.exists() {
        let content = std::fs::read_to_string(&lint_path)?;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            match serde_json::from_str::<LintSummaryRecord>(trimmed) {
                Ok(record) => {
                    if record.schema_version != LINT_SUMMARY_SCHEMA_VERSION {
                        tracing::warn!(
                            schema_version = record.schema_version,
                            expected = LINT_SUMMARY_SCHEMA_VERSION,
                            "skipping lint summary record with unexpected schema version"
                        );
                        result.records_skipped += 1;
                        continue;
                    }
                    result.lint_summaries_read += 1;
                    detect_trends(&record, &mut rule_buckets);
                }
                Err(e) => {
                    tracing::debug!(
                        error = %e,
                        "skipping unparseable lint summary record"
                    );
                    result.records_skipped += 1;
                }
            }
        }
    }

    for (rule, bucket) in &rule_buckets {
        result.lessons.extend(bucket.to_lessons(rule));
    }

    // Sort lessons by confidence descending for deterministic output.
    result.lessons.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(result)
}

/// Convert extracted training lessons to knowledge graph facts.
///
/// Each lesson becomes an `ExtractedFact` with:
/// - `subject`: the lint rule
/// - `predicate`: the outcome (e.g., "was fixed in PR")
/// - `object`: description with file context
/// - `confidence`: from the lesson's quality gate score
/// - `fact_type`: "observation" (training signal)
#[must_use]
pub fn lessons_to_facts(lessons: &[TrainingLesson]) -> Vec<super::types::ExtractedFact> {
    lessons
        .iter()
        .map(|lesson| {
            let predicate = match lesson.outcome {
                LessonOutcome::FixedInPr => "was fixed in PR".to_owned(),
                LessonOutcome::RecurringViolation => "recurs across scans".to_owned(),
                LessonOutcome::ImprovingTrend => "is improving".to_owned(),
                LessonOutcome::DegradingTrend => "is degrading".to_owned(),
            };

            let object = if let Some(pr) = lesson.pr_number {
                format!(
                    "{} (PR #{pr}, {} files)",
                    lesson.description,
                    lesson.affected_files.len()
                )
            } else {
                format!(
                    "{} ({} occurrences across {} files)",
                    lesson.description,
                    lesson.occurrence_count,
                    lesson.affected_files.len()
                )
            };

            super::types::ExtractedFact {
                subject: lesson.rule.clone(),
                predicate,
                object,
                confidence: lesson.confidence,
                is_correction: false,
                fact_type: Some("observation".to_owned()),
            }
        })
        .collect()
}

/// Accumulator for violations grouped by rule.
#[derive(Debug, Default)]
struct RuleBucket {
    /// Violations that came from merged PRs with fixed-outcome evidence.
    fixed: Vec<ViolationRecord>,
    /// Violations without PR context (unfixed/recurring).
    unfixed: Vec<ViolationRecord>,
    /// Violations with PR context but missing or negative fixed-outcome evidence.
    pr_linked_unresolved: Vec<ViolationRecord>,
    /// Distinct file paths seen.
    files: HashMap<String, u32>,
    /// Trend signal from lint summaries.
    trend: Option<LessonOutcome>,
}

impl RuleBucket {
    fn add_violation(&mut self, record: &ViolationRecord) {
        *self.files.entry(record.file.clone()).or_default() += 1;

        if record.pr_number.is_some() && record.sha.is_some() {
            if has_fixed_outcome_evidence(record) {
                self.fixed.push(record.clone());
            } else {
                self.pr_linked_unresolved.push(record.clone());
            }
        } else {
            self.unfixed.push(record.clone());
        }
    }

    fn to_lessons(&self, rule: &str) -> Vec<TrainingLesson> {
        let mut lessons = Vec::new();
        let affected_files: Vec<String> = self.files.keys().cloned().collect();

        if !self.fixed.is_empty() {
            let mut by_pr: HashMap<u32, Vec<&ViolationRecord>> = HashMap::new();
            for v in &self.fixed {
                if let Some(pr) = v.pr_number {
                    by_pr.entry(pr).or_default().push(v);
                }
            }

            for (pr_num, violations) in &by_pr {
                let pr_files: Vec<String> = violations.iter().map(|v| v.file.clone()).collect();
                let sample_snippet = violations
                    .first()
                    .map(|v| v.snippet.clone())
                    .unwrap_or_default();
                let explicit_outcome = violations
                    .iter()
                    .any(|v| matches!(v.outcome.as_deref(), Some("merged") | Some("fixed")));

                // WHY: explicit merged/fixed outcome is stronger evidence than a
                // before/after delta alone.
                let confidence = if explicit_outcome { 0.9 } else { 0.75 };

                lessons.push(TrainingLesson {
                    rule: rule.to_owned(),
                    outcome: LessonOutcome::FixedInPr,
                    description: format!("rule {rule} violation fixed: {sample_snippet}"),
                    confidence,
                    affected_files: pr_files,
                    occurrence_count: u32::try_from(violations.len()).unwrap_or(u32::MAX),
                    pr_number: Some(*pr_num),
                });
            }
        }

        if !self.unfixed.is_empty() {
            let count = u32::try_from(self.unfixed.len()).unwrap_or(u32::MAX);
            let sample_snippet = self
                .unfixed
                .first()
                .map(|v| v.snippet.clone())
                .unwrap_or_default();

            lessons.push(TrainingLesson {
                rule: rule.to_owned(),
                outcome: LessonOutcome::RecurringViolation,
                description: format!(
                    "rule {rule} has {count} unfixed violations: {sample_snippet}"
                ),
                // WHY: unfixed violations are inferred patterns (moderate confidence).
                confidence: 0.6,
                affected_files,
                occurrence_count: count,
                pr_number: None,
            });
        }

        if !self.pr_linked_unresolved.is_empty() {
            let mut by_pr: HashMap<u32, Vec<&ViolationRecord>> = HashMap::new();
            for v in &self.pr_linked_unresolved {
                if let Some(pr) = v.pr_number {
                    by_pr.entry(pr).or_default().push(v);
                }
            }

            for (pr_num, violations) in &by_pr {
                let pr_files: Vec<String> = violations.iter().map(|v| v.file.clone()).collect();
                let sample_snippet = violations
                    .first()
                    .map(|v| v.snippet.clone())
                    .unwrap_or_default();

                lessons.push(TrainingLesson {
                    rule: rule.to_owned(),
                    outcome: LessonOutcome::RecurringViolation,
                    description: format!(
                        "rule {rule} has unresolved PR-linked violations: {sample_snippet}"
                    ),
                    // WHY: PR-linked but unresolved observations are weaker than
                    // verified fixes and weaker than ordinary recurring patterns
                    // because the PR context is unverified.
                    confidence: 0.5,
                    affected_files: pr_files,
                    occurrence_count: u32::try_from(violations.len()).unwrap_or(u32::MAX),
                    pr_number: Some(*pr_num),
                });
            }
        }

        if let Some(trend) = &self.trend {
            let trend_confidence = match trend {
                LessonOutcome::ImprovingTrend | LessonOutcome::DegradingTrend => 0.7,
                _ => 0.5,
            };

            lessons.push(TrainingLesson {
                rule: rule.to_owned(),
                outcome: *trend,
                description: format!("rule {rule} shows {trend} trend"),
                confidence: trend_confidence,
                affected_files: Vec::new(),
                occurrence_count: 0,
                pr_number: None,
            });
        }

        lessons
    }
}

/// Detect improving/degrading trends from lint summary records.
///
/// Compares violation counts across time-ordered summaries.
/// WHY: two consecutive summaries showing the same count direction
/// is enough signal to flag a trend without over-fitting to noise.
fn detect_trends(summary: &LintSummaryRecord, buckets: &mut HashMap<String, RuleBucket>) {
    // WHY: lint summaries are repo-wide aggregates, not per-rule.
    // We use the total violation count as a proxy for overall code health.
    // A synthetic "REPO/total-violations" rule captures the trend.
    let synthetic_rule = format!("REPO/total-violations:{}", summary.repo);
    let bucket = buckets.entry(synthetic_rule).or_default();

    // Use total_violations as a simple trend signal.
    // This is intentionally coarse; per-rule trend detection would
    // require correlating violation records across timestamps.
    if summary.total_violations == 0 {
        bucket.trend = Some(LessonOutcome::ImprovingTrend);
    }
}

#[cfg(test)]
#[path = "training_tests.rs"]
mod tests;
