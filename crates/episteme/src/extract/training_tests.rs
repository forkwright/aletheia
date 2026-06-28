#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on collections with known length"
)]
#![expect(clippy::float_cmp, reason = "test assertions on exact float values")]
#![expect(
    clippy::disallowed_methods,
    reason = "tests use std::fs for synchronous fixture setup"
)]
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
    assert!(record.outcome.is_none());
    assert!(record.before_count.is_none());
    assert!(record.after_count.is_none());
}

#[test]
fn parse_violation_record_with_outcome_and_delta() {
    let json = r#"{"type":"violation","schema_version":2,"ts":"2026-03-25T15:43:30Z","rule":"RUST/expect","file":"/src/main.rs","line":10,"snippet":".expect(\"msg\")","project":"aletheia","pr_number":42,"sha":"abc123","outcome":"merged","before_count":5,"after_count":2}"#;
    let record: ViolationRecord = serde_json::from_str(json).unwrap();
    assert_eq!(record.outcome.as_deref(), Some("merged"));
    assert_eq!(record.before_count, Some(5));
    assert_eq!(record.after_count, Some(2));
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
        outcome: Some("merged".to_owned()),
        before_count: None,
        after_count: None,
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
        outcome: None,
        before_count: None,
        after_count: None,
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
        outcome: Some("merged".to_owned()),
        before_count: None,
        after_count: None,
    });

    let lessons = bucket.to_lessons("RUST/pub-visibility");
    assert_eq!(lessons.len(), 1);
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
        outcome: None,
        before_count: None,
        after_count: None,
    });

    let lessons = bucket.to_lessons("RUST/expect");
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].confidence, 0.6);
    assert!(lessons[0].pr_number.is_none());
}

#[test]
fn fixed_outcome_merged_emits_high_confidence_fixed_in_pr() {
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
        pr_number: Some(7),
        sha: Some("abc123".to_owned()),
        outcome: Some("merged".to_owned()),
        before_count: None,
        after_count: None,
    });

    let lessons = bucket.to_lessons("RUST/expect");
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].outcome, LessonOutcome::FixedInPr);
    assert_eq!(lessons[0].confidence, 0.9);
    assert_eq!(lessons[0].pr_number, Some(7));
}

#[test]
fn fixed_outcome_fixed_emits_high_confidence_fixed_in_pr() {
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
        pr_number: Some(8),
        sha: Some("def456".to_owned()),
        outcome: Some("fixed".to_owned()),
        before_count: None,
        after_count: None,
    });

    let lessons = bucket.to_lessons("RUST/expect");
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].outcome, LessonOutcome::FixedInPr);
    assert_eq!(lessons[0].confidence, 0.9);
}

#[test]
fn delta_only_fix_emits_fixed_in_pr_with_reduced_confidence() {
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
        pr_number: Some(9),
        sha: Some("ghi789".to_owned()),
        outcome: None,
        before_count: Some(5),
        after_count: Some(2),
    });

    let lessons = bucket.to_lessons("RUST/expect");
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].outcome, LessonOutcome::FixedInPr);
    assert_eq!(lessons[0].confidence, 0.75);
}

#[test]
fn pr_linked_without_outcome_is_unresolved() {
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
        pr_number: Some(10),
        sha: Some("jkl012".to_owned()),
        outcome: None,
        before_count: None,
        after_count: None,
    });

    let lessons = bucket.to_lessons("RUST/expect");
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].outcome, LessonOutcome::RecurringViolation);
    assert_eq!(lessons[0].confidence, 0.5);
    assert_eq!(lessons[0].pr_number, Some(10));
}

#[test]
fn pr_linked_introduced_is_unresolved() {
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
        pr_number: Some(11),
        sha: Some("mno345".to_owned()),
        outcome: Some("introduced".to_owned()),
        before_count: None,
        after_count: None,
    });

    let lessons = bucket.to_lessons("RUST/expect");
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].outcome, LessonOutcome::RecurringViolation);
    assert_eq!(lessons[0].confidence, 0.5);
    assert_eq!(lessons[0].pr_number, Some(11));
}

#[test]
fn pr_linked_failed_is_unresolved() {
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
        pr_number: Some(12),
        sha: Some("pqr678".to_owned()),
        outcome: Some("failed".to_owned()),
        before_count: None,
        after_count: None,
    });

    let lessons = bucket.to_lessons("RUST/expect");
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].outcome, LessonOutcome::RecurringViolation);
    assert_eq!(lessons[0].confidence, 0.5);
    assert_eq!(lessons[0].pr_number, Some(12));
}

#[test]
fn pr_linked_unmerged_is_unresolved() {
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
        pr_number: Some(13),
        sha: Some("stu901".to_owned()),
        outcome: Some("unmerged".to_owned()),
        before_count: None,
        after_count: None,
    });

    let lessons = bucket.to_lessons("RUST/expect");
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].outcome, LessonOutcome::RecurringViolation);
    assert_eq!(lessons[0].confidence, 0.5);
    assert_eq!(lessons[0].pr_number, Some(13));
}

#[test]
fn pr_linked_with_increasing_delta_is_unresolved() {
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
        pr_number: Some(14),
        sha: Some("vwx234".to_owned()),
        outcome: None,
        before_count: Some(1),
        after_count: Some(4),
    });

    let lessons = bucket.to_lessons("RUST/expect");
    assert_eq!(lessons.len(), 1);
    assert_eq!(lessons[0].outcome, LessonOutcome::RecurringViolation);
    assert_eq!(lessons[0].confidence, 0.5);
    assert_eq!(lessons[0].pr_number, Some(14));
}

#[test]
fn extract_from_training_files_handles_unresolved_pr() {
    let dir = tempfile::tempdir().unwrap();

    let violations = [
        // Verified fix by explicit merged outcome.
        r#"{"type":"violation","schema_version":2,"ts":"2026-03-25T15:43:30Z","rule":"RUST/expect","file":"/src/lib.rs","line":10,"snippet":".expect(\"msg\")","project":"","pr_number":42,"sha":"abc123","outcome":"merged"}"#,
        // PR-linked but unresolved: pr_number and sha present, no fixed evidence.
        r#"{"type":"violation","schema_version":2,"ts":"2026-03-25T15:43:30Z","rule":"RUST/expect","file":"/src/main.rs","line":20,"snippet":".expect(\"other\")","project":"","pr_number":43,"sha":"def456"}"#,
        // PR-linked introduced: pr_number and sha present, negative outcome.
        r#"{"type":"violation","schema_version":2,"ts":"2026-03-25T15:43:30Z","rule":"RUST/unwrap","file":"/src/parse.rs","line":5,"snippet":".unwrap()","project":"","pr_number":44,"sha":"ghi789","outcome":"introduced"}"#,
    ];
    std::fs::write(dir.path().join("violations.jsonl"), violations.join("\n")).unwrap();

    let result = extract_from_training_data(dir.path()).unwrap();
    assert_eq!(result.violations_read, 3);

    let fixed = result
        .lessons
        .iter()
        .filter(|l| l.outcome == LessonOutcome::FixedInPr)
        .collect::<Vec<_>>();
    assert_eq!(fixed.len(), 1);
    assert_eq!(fixed[0].pr_number, Some(42));
    assert_eq!(fixed[0].confidence, 0.9);

    let unresolved = result
        .lessons
        .iter()
        .filter(|l| l.outcome == LessonOutcome::RecurringViolation && l.pr_number.is_some())
        .collect::<Vec<_>>();
    assert_eq!(unresolved.len(), 2);
    assert!(unresolved.iter().any(|l| l.pr_number == Some(43)));
    assert!(unresolved.iter().any(|l| l.pr_number == Some(44)));
    assert!(unresolved.iter().all(|l| l.confidence == 0.5));
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
        },
        TrainingLesson {
            rule: "RUST/pub-visibility".to_owned(),
            outcome: LessonOutcome::RecurringViolation,
            description: "pub items not narrowed".to_owned(),
            confidence: 0.6,
            affected_files: vec!["/src/a.rs".to_owned(), "/src/b.rs".to_owned()],
            occurrence_count: 5,
            pr_number: None,
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
    assert!(result.is_empty());
    assert_eq!(result.violations_read, 0);
    assert_eq!(result.lint_summaries_read, 0);
}

#[test]
fn extract_from_training_files() {
    let dir = tempfile::tempdir().unwrap();

    let violations = [
        r#"{"type":"violation","schema_version":2,"ts":"2026-03-25T15:43:30Z","rule":"RUST/expect","file":"/src/lib.rs","line":10,"snippet":".expect(\"msg\")","project":"","pr_number":42,"sha":"abc123","outcome":"merged"}"#,
        r#"{"type":"violation","schema_version":2,"ts":"2026-03-25T15:43:30Z","rule":"RUST/expect","file":"/src/main.rs","line":20,"snippet":".expect(\"other\")","project":"","pr_number":null,"sha":null}"#,
    ];
    std::fs::write(dir.path().join("violations.jsonl"), violations.join("\n")).unwrap();

    let lint = r#"{"type":"lint","schema_version":2,"ts":"2026-03-25T15:43:30Z","repo":"/repo","total_violations":100,"rules_triggered":10,"duration_ms":5000}"#;
    std::fs::write(dir.path().join("lint.jsonl"), lint).unwrap();

    let result = extract_from_training_data(dir.path()).unwrap();
    assert_eq!(result.violations_read, 2);
    assert_eq!(result.lint_summaries_read, 1);
    assert!(!result.is_empty());

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
        r#"{"type":"violation","schema_version":2,"ts":"2026-01-01T00:00:00Z","rule":"HIGH/rule","file":"/b.rs","line":1,"snippet":"y","project":"","pr_number":99,"sha":"abc","outcome":"merged"}"#,
    ];
    std::fs::write(dir.path().join("violations.jsonl"), violations.join("\n")).unwrap();

    let result = extract_from_training_data(dir.path()).unwrap();
    assert!(result.lessons.len() >= 2);

    assert!(
        result.lessons[0].confidence >= result.lessons[1].confidence,
        "lessons should be sorted by confidence descending"
    );
}
