//! PR lesson extraction from training data with quality gates.
//!
//! Reads JSONL training files (violations, lint summaries) produced by
//! `kanon lint`, extracts patterns from successful and failed fixes, and
//! converts them to knowledge graph facts.
//!
//! Quality gates:
//! - A violation is a successful fix candidate only when the record carries
//!   explicit outcome evidence: a merged `pr_state` and `resolved: true`. PR
//!   context alone (`pr_number` + `sha`) records where a violation was seen,
//!   not what became of it.
//! - Confidence scoring: verified fixes get 0.9, inferred patterns get 0.6.
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
    /// PR lifecycle state at record time (`merged`, `open`, `closed`).
    ///
    /// Absent in schema v2 records, which carry no outcome evidence.
    #[serde(default)]
    pub(crate) pr_state: Option<String>,
    /// Whether this violation was absent at `sha` — that is, actually fixed.
    ///
    /// Absent in schema v2 records, which carry no outcome evidence.
    #[serde(default)]
    pub(crate) resolved: Option<bool>,
}

/// The `pr_state` value that marks a PR as landed on the default branch.
const MERGED_PR_STATE: &str = "merged";

impl ViolationRecord {
    /// Whether the record was captured while scanning a pull request.
    fn has_pr_context(&self) -> bool {
        self.pr_number.is_some() && self.sha.is_some()
    }

    /// Whether the record proves the violation was fixed by a landed PR.
    ///
    /// WHY(#5384): a violation record is a record of *presence*. Observing one
    /// inside a PR is equally consistent with the PR having introduced it, or
    /// with the PR having failed, stayed open, or never removed it. Only an
    /// explicit resolved-and-merged outcome distinguishes a fix from a sighting.
    fn has_landed_fix_evidence(&self) -> bool {
        self.has_pr_context()
            && self.resolved == Some(true)
            && self.pr_state.as_deref() == Some(MERGED_PR_STATE)
    }
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
    /// Stable evidence-chain pointers back to the source `ViolationRecord`s
    /// this lesson was derived from (#4863): `"{sha}:{file}:{line}"` when
    /// the record carries a git SHA, else `"{file}:{line}"`. Empty for the
    /// trend-signal lesson variant, which is derived from an aggregate
    /// lint-summary comparison rather than individual violation records.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_violation_ids: Vec<String>,
}

/// Stable evidence-chain id for one violation record (#4863): `sha` when
/// present (a PR-context record), else `file:line` (a repo-wide scan
/// record, which carries no SHA).
fn violation_id(record: &ViolationRecord) -> String {
    record.sha.as_deref().map_or_else(
        || format!("{}:{}", record.file, record.line),
        |sha| format!("{sha}:{}:{}", record.file, record.line),
    )
}

/// Classification of a lesson outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum LessonOutcome {
    /// A violation was fixed in a merged PR, proven by outcome evidence.
    FixedInPr,
    /// A violation was seen in PR context with no outcome evidence either way.
    ObservedInPr,
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
            Self::ObservedInPr => f.write_str("observed_in_pr"),
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

/// Extract lessons from training data JSONL files.
///
/// Reads violations and lint summaries, applies quality gates, and produces
/// deduplicated lessons grouped by rule.
///
/// # Quality gates
///
/// - Violations with a merged `pr_state` and `resolved: true` are treated as
///   fixed; PR context alone yields an unresolved observation instead.
/// - Violations without PR context are treated as unfixed (recurring).
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
                LessonOutcome::ObservedInPr => "was observed in PR, outcome unknown".to_owned(),
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
    /// Violations proven fixed by a landed PR (merged `pr_state` + `resolved`).
    fixed: Vec<ViolationRecord>,
    /// Violations seen in PR context whose outcome is unknown.
    observed: Vec<ViolationRecord>,
    /// Violations without PR context (unfixed/recurring).
    unfixed: Vec<ViolationRecord>,
    /// Distinct file paths seen.
    files: HashMap<String, u32>,
    /// Trend signal from lint summaries.
    trend: Option<LessonOutcome>,
}

impl RuleBucket {
    fn add_violation(&mut self, record: &ViolationRecord) {
        *self.files.entry(record.file.clone()).or_default() += 1;

        if record.has_landed_fix_evidence() {
            self.fixed.push(record.clone());
        } else if record.has_pr_context() {
            self.observed.push(record.clone());
        } else {
            self.unfixed.push(record.clone());
        }
    }

    /// Emit one lesson per PR for `records`, all sharing `outcome`.
    fn extend_pr_lessons(
        lessons: &mut Vec<TrainingLesson>,
        rule: &str,
        records: &[ViolationRecord],
        outcome: LessonOutcome,
        confidence: f64,
    ) {
        let mut by_pr: HashMap<u32, Vec<&ViolationRecord>> = HashMap::new();
        for v in records {
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
            let description = match outcome {
                LessonOutcome::FixedInPr => {
                    format!("rule {rule} violation fixed: {sample_snippet}")
                }
                _ => format!(
                    "rule {rule} violation seen in PR, outcome unrecorded: {sample_snippet}"
                ),
            };

            lessons.push(TrainingLesson {
                rule: rule.to_owned(),
                outcome,
                description,
                confidence,
                affected_files: pr_files,
                occurrence_count: u32::try_from(violations.len()).unwrap_or(u32::MAX),
                pr_number: Some(*pr_num),
                source_violation_ids: violations.iter().map(|v| violation_id(v)).collect(),
            });
        }
    }

    fn to_lessons(&self, rule: &str) -> Vec<TrainingLesson> {
        let mut lessons = Vec::new();
        let affected_files: Vec<String> = self.files.keys().cloned().collect();

        // WHY: outcome-evidenced fixes are verified (high confidence); a PR
        // sighting with no outcome evidence is an unresolved observation and
        // shares the inferred-pattern confidence tier.
        Self::extend_pr_lessons(
            &mut lessons,
            rule,
            &self.fixed,
            LessonOutcome::FixedInPr,
            0.9,
        );
        Self::extend_pr_lessons(
            &mut lessons,
            rule,
            &self.observed,
            LessonOutcome::ObservedInPr,
            0.6,
        );

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
                source_violation_ids: self.unfixed.iter().map(violation_id).collect(),
            });
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
                // WHY empty: a trend is a lint-summary-count comparison
                // across scans, not a claim about specific violation
                // records -- there is no source_violation_ids to cite.
                source_violation_ids: Vec::new(),
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
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on collections with known length"
)]
#[expect(clippy::float_cmp, reason = "test assertions on exact float values")]
#[expect(
    clippy::disallowed_methods,
    reason = "tests use std::fs for synchronous fixture setup"
)]
mod tests {
    use super::*;

    #[test]
    fn parse_violation_record() {
        let json = r#"{"type":"violation","schema_version":2,"ts":"2026-03-25T15:43:30Z","rule":"RUST/pub-visibility","file":"/src/lib.rs","line":28,"snippet":"pub type Result<T>","project":"","pr_number":null,"sha":null}"#;
        let record: ViolationRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.rule, "RUST/pub-visibility");
        assert_eq!(record.line, 28);
        assert!(record.pr_number.is_none());
    }

    #[test]
    fn parse_violation_record_with_pr() {
        let json = r#"{"type":"violation","schema_version":2,"ts":"2026-03-25T15:43:30Z","rule":"RUST/expect","file":"/src/main.rs","line":10,"snippet":".expect(\"msg\")","project":"aletheia","pr_number":42,"sha":"abc123"}"#;
        let record: ViolationRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.pr_number, Some(42));
        assert_eq!(record.sha.as_deref(), Some("abc123"));
        // A schema v2 record carries no outcome evidence, so it cannot be a fix.
        assert!(record.pr_state.is_none());
        assert!(record.resolved.is_none());
        assert!(record.has_pr_context());
        assert!(!record.has_landed_fix_evidence());
    }

    #[test]
    fn parse_lint_summary_record() {
        let json = r#"{"type":"lint","schema_version":2,"ts":"2026-03-25T15:43:30Z","repo":"/repo","total_violations":100,"rules_triggered":10,"duration_ms":5000}"#;
        let record: LintSummaryRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.total_violations, 100);
        assert_eq!(record.rules_triggered, 10);
    }

    #[test]
    fn rule_bucket_classifies_fixed_vs_unfixed() {
        let mut bucket = RuleBucket::default();

        bucket.add_violation(&ViolationRecord {
            record_type: "violation".to_owned(),
            schema_version: 2,
            ts: "2026-01-01T00:00:00Z".to_owned(),
            rule: "RUST/expect".to_owned(),
            file: "/src/lib.rs".to_owned(),
            line: 10,
            snippet: ".expect(\"msg\")".to_owned(),
            project: String::new(),
            pr_number: Some(42),
            sha: Some("abc123".to_owned()),
            pr_state: Some("merged".to_owned()),
            resolved: Some(true),
        });

        bucket.add_violation(&ViolationRecord {
            record_type: "violation".to_owned(),
            schema_version: 2,
            ts: "2026-01-02T00:00:00Z".to_owned(),
            rule: "RUST/expect".to_owned(),
            file: "/src/main.rs".to_owned(),
            line: 20,
            snippet: ".expect(\"other\")".to_owned(),
            project: String::new(),
            pr_number: None,
            sha: None,
            pr_state: None,
            resolved: None,
        });

        assert_eq!(bucket.fixed.len(), 1);
        assert_eq!(bucket.unfixed.len(), 1);
        assert_eq!(bucket.files.len(), 2);

        let lessons = bucket.to_lessons("RUST/expect");
        assert_eq!(lessons.len(), 2, "one fixed + one recurring");
        assert!(
            lessons
                .iter()
                .any(|l| l.outcome == LessonOutcome::FixedInPr)
        );
        assert!(
            lessons
                .iter()
                .any(|l| l.outcome == LessonOutcome::RecurringViolation)
        );
    }

    #[test]
    fn fixed_lessons_have_high_confidence() {
        let mut bucket = RuleBucket::default();
        bucket.add_violation(&ViolationRecord {
            record_type: "violation".to_owned(),
            schema_version: 2,
            ts: "2026-01-01T00:00:00Z".to_owned(),
            rule: "RUST/pub-visibility".to_owned(),
            file: "/src/lib.rs".to_owned(),
            line: 1,
            snippet: "pub fn".to_owned(),
            project: String::new(),
            pr_number: Some(100),
            sha: Some("def456".to_owned()),
            pr_state: Some("merged".to_owned()),
            resolved: Some(true),
        });

        let lessons = bucket.to_lessons("RUST/pub-visibility");
        assert_eq!(lessons.len(), 1);
        assert_eq!(lessons[0].outcome, LessonOutcome::FixedInPr);
        assert_eq!(lessons[0].confidence, 0.9);
        assert_eq!(lessons[0].pr_number, Some(100));
    }

    #[test]
    fn recurring_lessons_have_moderate_confidence() {
        let mut bucket = RuleBucket::default();
        bucket.add_violation(&ViolationRecord {
            record_type: "violation".to_owned(),
            schema_version: 2,
            ts: "2026-01-01T00:00:00Z".to_owned(),
            rule: "RUST/expect".to_owned(),
            file: "/src/lib.rs".to_owned(),
            line: 1,
            snippet: ".expect()".to_owned(),
            project: String::new(),
            pr_number: None,
            sha: None,
            pr_state: None,
            resolved: None,
        });

        let lessons = bucket.to_lessons("RUST/expect");
        assert_eq!(lessons.len(), 1);
        assert_eq!(lessons[0].confidence, 0.6);
        assert!(lessons[0].pr_number.is_none());
    }

    /// Build a PR-context violation with the given outcome evidence.
    fn pr_violation(pr_state: Option<&str>, resolved: Option<bool>) -> ViolationRecord {
        ViolationRecord {
            record_type: "violation".to_owned(),
            schema_version: 2,
            ts: "2026-01-01T00:00:00Z".to_owned(),
            rule: "RUST/expect".to_owned(),
            file: "/src/lib.rs".to_owned(),
            line: 1,
            snippet: ".expect(\"msg\")".to_owned(),
            project: String::new(),
            pr_number: Some(7),
            sha: Some("abc123".to_owned()),
            pr_state: pr_state.map(str::to_owned),
            resolved,
        }
    }

    /// The regression guard for #5384: before the fix every one of these cases
    /// produced `FixedInPr` at confidence 0.9 purely because `pr_number` and
    /// `sha` were present.
    #[test]
    fn pr_context_without_landed_fix_evidence_is_not_a_fix() {
        // (pr_state, resolved, why this is not a landed fix)
        let cases = [
            (None, None, "schema v2 record: no outcome evidence at all"),
            (Some("merged"), None, "merged, but resolution unrecorded"),
            (Some("merged"), Some(false), "merged PR that introduced it"),
            (Some("open"), Some(true), "resolved but never landed"),
            (Some("closed"), Some(true), "resolved but PR was abandoned"),
            (None, Some(true), "resolved, but merge state unknown"),
        ];

        for (pr_state, resolved, why) in cases {
            let record = pr_violation(pr_state, resolved);
            assert!(
                !record.has_landed_fix_evidence(),
                "should not count as a landed fix — {why}"
            );

            let mut bucket = RuleBucket::default();
            bucket.add_violation(&record);
            assert_eq!(bucket.fixed.len(), 0, "{why}");
            assert_eq!(bucket.observed.len(), 1, "{why}");

            let lessons = bucket.to_lessons("RUST/expect");
            assert_eq!(lessons.len(), 1, "{why}");
            assert_eq!(lessons[0].outcome, LessonOutcome::ObservedInPr, "{why}");
            assert_eq!(lessons[0].confidence, 0.6, "{why}");
            assert_eq!(lessons[0].pr_number, Some(7), "{why}");
        }
    }

    #[test]
    fn merged_and_resolved_is_the_only_fix_evidence() {
        let record = pr_violation(Some("merged"), Some(true));
        assert!(record.has_landed_fix_evidence());

        let mut bucket = RuleBucket::default();
        bucket.add_violation(&record);
        assert_eq!(bucket.fixed.len(), 1);
        assert_eq!(bucket.observed.len(), 0);

        let lessons = bucket.to_lessons("RUST/expect");
        assert_eq!(lessons.len(), 1);
        assert_eq!(lessons[0].outcome, LessonOutcome::FixedInPr);
        assert_eq!(lessons[0].confidence, 0.9);
    }

    #[test]
    fn observed_in_pr_lessons_do_not_claim_a_fix() {
        let lessons = vec![TrainingLesson {
            rule: "RUST/expect".to_owned(),
            outcome: LessonOutcome::ObservedInPr,
            description: "seen in PR".to_owned(),
            confidence: 0.6,
            affected_files: vec!["/src/lib.rs".to_owned()],
            occurrence_count: 1,
            pr_number: Some(7),
            source_violation_ids: vec!["abc123:/src/lib.rs:10".to_owned()],
        }];

        let facts = lessons_to_facts(&lessons);
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].predicate, "was observed in PR, outcome unknown");
        assert!(!facts[0].predicate.contains("fixed"));
        assert_eq!(facts[0].confidence, 0.6);
    }

    #[test]
    fn lessons_to_facts_produces_correct_types() {
        let lessons = vec![
            TrainingLesson {
                rule: "RUST/expect".to_owned(),
                outcome: LessonOutcome::FixedInPr,
                description: "expect replaced with context".to_owned(),
                confidence: 0.9,
                affected_files: vec!["/src/lib.rs".to_owned()],
                occurrence_count: 1,
                pr_number: Some(42),
                source_violation_ids: vec!["def456:/src/lib.rs:20".to_owned()],
            },
            TrainingLesson {
                rule: "RUST/pub-visibility".to_owned(),
                outcome: LessonOutcome::RecurringViolation,
                description: "pub items not narrowed".to_owned(),
                confidence: 0.6,
                affected_files: vec!["/src/a.rs".to_owned(), "/src/b.rs".to_owned()],
                occurrence_count: 5,
                pr_number: None,
                source_violation_ids: vec![
                    "/src/a.rs:5".to_owned(),
                    "/src/b.rs:12".to_owned(),
                ],
            },
        ];

        let facts = lessons_to_facts(&lessons);
        assert_eq!(facts.len(), 2);

        assert_eq!(facts[0].subject, "RUST/expect");
        assert_eq!(facts[0].predicate, "was fixed in PR");
        assert!(facts[0].object.contains("PR #42"));
        assert_eq!(facts[0].confidence, 0.9);

        assert_eq!(facts[1].subject, "RUST/pub-visibility");
        assert_eq!(facts[1].predicate, "recurs across scans");
        assert!(facts[1].object.contains("5 occurrences"));
        assert_eq!(facts[1].confidence, 0.6);
    }

    #[test]
    fn extract_from_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let result = extract_from_training_data(dir.path()).unwrap();
        assert!(result.lessons.is_empty());
        assert_eq!(result.violations_read, 0);
        assert_eq!(result.lint_summaries_read, 0);
    }

    #[test]
    fn extract_from_training_files() {
        let dir = tempfile::tempdir().unwrap();

        let violations = [
            r#"{"type":"violation","schema_version":2,"ts":"2026-03-25T15:43:30Z","rule":"RUST/expect","file":"/src/lib.rs","line":10,"snippet":".expect(\"msg\")","project":"","pr_number":42,"sha":"abc123","pr_state":"merged","resolved":true}"#,
            r#"{"type":"violation","schema_version":2,"ts":"2026-03-25T15:43:30Z","rule":"RUST/expect","file":"/src/main.rs","line":20,"snippet":".expect(\"other\")","project":"","pr_number":null,"sha":null}"#,
        ];
        std::fs::write(dir.path().join("violations.jsonl"), violations.join("\n")).unwrap();

        let lint = r#"{"type":"lint","schema_version":2,"ts":"2026-03-25T15:43:30Z","repo":"/repo","total_violations":100,"rules_triggered":10,"duration_ms":5000}"#;
        std::fs::write(dir.path().join("lint.jsonl"), lint).unwrap();

        let result = extract_from_training_data(dir.path()).unwrap();
        assert_eq!(result.violations_read, 2);
        assert_eq!(result.lint_summaries_read, 1);
        assert!(!result.lessons.is_empty());

        assert!(
            result
                .lessons
                .iter()
                .any(|l| l.outcome == LessonOutcome::FixedInPr),
            "should have a FixedInPr lesson"
        );
        assert!(
            result
                .lessons
                .iter()
                .any(|l| l.outcome == LessonOutcome::RecurringViolation),
            "should have a RecurringViolation lesson"
        );
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let content = "not valid json\n{\"also\": \"incomplete\"}\n";
        std::fs::write(dir.path().join("violations.jsonl"), content).unwrap();

        let result = extract_from_training_data(dir.path()).unwrap();
        assert_eq!(result.violations_read, 0);
        assert_eq!(result.records_skipped, 2);
    }

    #[test]
    fn outcome_display() {
        assert_eq!(LessonOutcome::FixedInPr.to_string(), "fixed_in_pr");
        assert_eq!(LessonOutcome::ObservedInPr.to_string(), "observed_in_pr");
        assert_eq!(
            LessonOutcome::RecurringViolation.to_string(),
            "recurring_violation"
        );
        assert_eq!(LessonOutcome::ImprovingTrend.to_string(), "improving_trend");
        assert_eq!(LessonOutcome::DegradingTrend.to_string(), "degrading_trend");
    }

    #[test]
    fn lessons_sorted_by_confidence_descending() {
        let dir = tempfile::tempdir().unwrap();
        let violations = [
            r#"{"type":"violation","schema_version":2,"ts":"2026-01-01T00:00:00Z","rule":"LOW/rule","file":"/a.rs","line":1,"snippet":"x","project":"","pr_number":null,"sha":null}"#,
            r#"{"type":"violation","schema_version":2,"ts":"2026-01-01T00:00:00Z","rule":"HIGH/rule","file":"/b.rs","line":1,"snippet":"y","project":"","pr_number":99,"sha":"abc","pr_state":"merged","resolved":true}"#,
        ];
        std::fs::write(dir.path().join("violations.jsonl"), violations.join("\n")).unwrap();

        let result = extract_from_training_data(dir.path()).unwrap();
        assert!(result.lessons.len() >= 2);

        assert!(
            result.lessons[0].confidence >= result.lessons[1].confidence,
            "lessons should be sorted by confidence descending"
        );
    }
}
