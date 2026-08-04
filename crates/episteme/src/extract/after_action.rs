//! After-action JSONL extraction: dispatch telemetry into knowledge facts.
//!
//! Reads the JSONL records energeia's dispatch pipeline appends to
//! `{instance}/logs/after-actions/*.jsonl` after every completed dispatch
//! (see `energeia::pipeline::after_action::append_after_action_record`) and
//! converts non-passing QA verdicts and session failure classes into
//! [`super::types::ExtractedFact`]s. This is the daemon's `lesson-extraction`
//! task's input source (#6419): unlike the retired phronesis-era
//! `workflow/training/{violations,lint}.jsonl` schema, this directory is
//! created and populated by the running instance itself.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use super::types::ExtractedFact;

/// One line of after-action telemetry.
///
/// Mirrors the JSON shape `energeia::pipeline::after_action::AfterActionRecord`
/// writes.
///
/// WHY: energeia's record type is private, write-only telemetry (no
/// `Deserialize`), and episteme must not depend on energeia (knowledge layer
/// sits below the dispatch/agent layer — see `docs/ARCHITECTURE.md`). The
/// JSONL file is the contract between the two crates instead of a shared
/// Rust type. `#[serde(default)]` on every field means schema growth on the
/// energeia side never breaks this reader, and a genuinely malformed line
/// (not valid JSON at all) is counted as skipped rather than aborting the run.
#[derive(Debug, Clone, Default, Deserialize)]
struct AfterActionRecord {
    #[serde(default)]
    dispatch_id: String,
    #[serde(default)]
    session_outcomes: Vec<AfterActionSessionOutcome>,
    #[serde(default)]
    qa_verdict: String,
    #[serde(default)]
    prompt_hash: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct AfterActionSessionOutcome {
    #[serde(default)]
    status: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    failure_class: Option<String>,
    #[serde(default)]
    category: Option<String>,
}

/// Result of mining after-action logs for knowledge-graph signal.
#[derive(Debug, Clone, Default)]
pub struct AfterActionExtraction {
    /// Facts derived from non-passing QA verdicts and session failures.
    pub facts: Vec<ExtractedFact>,
    /// Number of `.jsonl` files read.
    pub files_read: usize,
    /// Number of dispatch records successfully parsed.
    pub records_read: usize,
    /// Number of lines that failed to parse as a dispatch record.
    pub records_skipped: usize,
}

/// Extract knowledge facts from after-action JSONL logs.
///
/// Reads every `*.jsonl` file directly inside `log_dir` (non-recursive,
/// matching the flat per-day layout `append_after_action_record` writes),
/// and converts each dispatch record's QA verdict and session failure
/// classes into facts. A dispatch with a `pass` verdict and no per-session
/// failures produces no fact — only signal worth graphing is emitted.
///
/// # Errors
///
/// Returns an I/O error if `log_dir` exists but a `.jsonl` file inside it
/// cannot be read. A `log_dir` that does not exist is not an error — the
/// result is simply empty with `files_read == 0`; callers distinguish
/// "directory absent" from "directory empty" by checking `log_dir.exists()`
/// themselves before calling, since only the caller knows whether that is
/// expected (fresh instance, no dispatches yet) or a misconfiguration.
pub fn extract_from_after_action_logs(log_dir: &Path) -> std::io::Result<AfterActionExtraction> {
    let mut result = AfterActionExtraction::default();

    if !log_dir.is_dir() {
        return Ok(result);
    }

    let mut entries: Vec<_> = std::fs::read_dir(log_dir)?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    // WHY: deterministic order makes extraction output (and tests) reproducible.
    entries.sort();

    for path in entries {
        // WHY: read_to_string avoids disallowed File::open; after-action files
        // are bounded by daily rotation (one file per calendar day).
        let content = std::fs::read_to_string(&path)?;
        result.files_read += 1;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            match serde_json::from_str::<AfterActionRecord>(trimmed) {
                Ok(record) => {
                    result.records_read += 1;
                    result.facts.extend(facts_for_record(&record));
                }
                Err(_) => result.records_skipped += 1,
            }
        }
    }

    Ok(result)
}

/// Derive facts from a single dispatch record: one for a non-passing QA
/// verdict, plus one per session outcome that carries a failure class.
fn facts_for_record(record: &AfterActionRecord) -> Vec<ExtractedFact> {
    let subject = if record.prompt_hash.is_empty() {
        record.dispatch_id.clone()
    } else {
        record.prompt_hash.clone()
    };

    let mut facts = Vec::new();

    if !record.qa_verdict.is_empty() && record.qa_verdict != "pass" {
        let failing = record
            .session_outcomes
            .iter()
            .filter(|o| o.status != "success")
            .count();
        facts.push(ExtractedFact {
            subject: subject.clone(),
            predicate: "dispatch QA verdict was".to_owned(),
            object: format!(
                "{} ({failing}/{} sessions not successful)",
                record.qa_verdict,
                record.session_outcomes.len()
            ),
            confidence: 0.7,
            is_correction: false,
            fact_type: Some("ops-training".to_owned()),
        });
    }

    for outcome in &record.session_outcomes {
        let Some(failure_class) = outcome.failure_class.as_deref().filter(|c| !c.is_empty()) else {
            continue;
        };
        let model = outcome.model.as_deref().unwrap_or("unknown model");
        let category = outcome.category.as_deref().unwrap_or("uncategorized");
        facts.push(ExtractedFact {
            subject: failure_class.to_owned(),
            predicate: "occurred in dispatch".to_owned(),
            object: format!("{subject} ({model}, category: {category})"),
            confidence: 0.7,
            is_correction: false,
            fact_type: Some("ops-training".to_owned()),
        });
    }

    facts
}

/// Aggregate occurrence counts of failure classes across an extraction, for
/// summary logging.
#[must_use]
pub fn failure_class_counts(extraction: &AfterActionExtraction) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for fact in &extraction.facts {
        if fact.predicate == "occurred in dispatch" {
            *counts.entry(fact.subject.clone()).or_insert(0) += 1;
        }
    }
    counts
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn missing_directory_is_not_an_error() {
        let extraction =
            extract_from_after_action_logs(Path::new("/nonexistent/does-not-exist-6419"))
                .expect("missing dir must not error");
        assert_eq!(extraction.files_read, 0);
        assert_eq!(extraction.facts.len(), 0);
    }

    #[test]
    fn passing_dispatch_with_no_failures_produces_no_facts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let line = serde_json::json!({
            "dispatch_id": "d1",
            "session_outcomes": [{"status": "success", "model": "m1"}],
            "qa_verdict": "pass",
            "prompt_hash": "h1",
        });
        std::fs::write(dir.path().join("2026-08-04.jsonl"), line.to_string() + "\n")
            .expect("write fixture");

        let extraction = extract_from_after_action_logs(dir.path()).expect("extract");
        assert_eq!(extraction.records_read, 1);
        assert!(
            extraction.facts.is_empty(),
            "a passing dispatch with no session failures must produce no facts, got: {:?}",
            extraction.facts
        );
    }

    #[test]
    fn non_passing_verdict_and_failure_class_both_produce_facts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let line = serde_json::json!({
            "dispatch_id": "d2",
            "session_outcomes": [
                {"status": "failed", "model": "m1", "failure_class": "prompt-quality", "category": "refactor"},
                {"status": "success", "model": "m2"},
            ],
            "qa_verdict": "fail",
            "prompt_hash": "h2",
        });
        std::fs::write(dir.path().join("2026-08-04.jsonl"), line.to_string() + "\n")
            .expect("write fixture");

        let extraction = extract_from_after_action_logs(dir.path()).expect("extract");
        assert_eq!(extraction.records_read, 1);
        assert_eq!(extraction.records_skipped, 0);
        // One dispatch-level verdict fact + one session-level failure-class fact.
        assert_eq!(extraction.facts.len(), 2);
        assert!(
            extraction
                .facts
                .iter()
                .any(|f| f.subject == "h2" && f.object.contains("fail")),
            "expected a dispatch-verdict fact: {:?}",
            extraction.facts
        );
        assert!(
            extraction
                .facts
                .iter()
                .any(|f| f.subject == "prompt-quality"),
            "expected a failure-class fact: {:?}",
            extraction.facts
        );

        let counts = failure_class_counts(&extraction);
        assert_eq!(counts.get("prompt-quality"), Some(&1));
    }

    #[test]
    fn malformed_line_is_counted_as_skipped_not_fatal() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("2026-08-04.jsonl"),
            "not valid json\n{\"dispatch_id\": \"d3\", \"qa_verdict\": \"fail\", \"session_outcomes\": []}\n",
        )
        .expect("write fixture");

        let extraction = extract_from_after_action_logs(dir.path()).expect("extract");
        assert_eq!(extraction.records_skipped, 1);
        assert_eq!(extraction.records_read, 1);
        assert_eq!(extraction.facts.len(), 1);
    }
}
