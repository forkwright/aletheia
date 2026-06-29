use std::path::Path;
use std::time::Duration;

use super::*;
use crate::scenario::{ScenarioClassification, ScenarioMeta, ScenarioResult};

fn sample_meta(id: &'static str, category: &'static str) -> ScenarioMeta {
    ScenarioMeta {
        id,
        description: "test scenario",
        category,
        requires_auth: false,
        requires_nous: false,
        expected_contains: None,
        expected_pattern: None,
        classification: ScenarioClassification::Assertive,
    }
}

fn sample_provenance() -> EvalProvenance {
    EvalProvenance::new("er-test", "http://localhost")
}

fn create_dir(path: &Path) {
    std::fs::create_dir_all(path).expect("test directory should be created");
}

fn remove_file_if_exists(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("failed to remove {}: {error}", path.display()),
    }
}

#[test]
fn records_from_report_passed() {
    let report = RunReport {
        passed: 1,
        failed: 0,
        skipped: 0,
        total_duration: Duration::from_millis(100),
        results: vec![ScenarioResult {
            meta: sample_meta("test-pass", "health"),
            outcome: ScenarioOutcome::Passed {
                duration: Duration::from_millis(50),
            },
            sub_results: vec![],
        }],
        provenance: sample_provenance(),
    };
    let records = records_from_report(&report);
    assert_eq!(records.len(), 1, "should produce one record");
    assert!(records[0].passed, "should be passed");
    assert_eq!(records[0].outcome, "passed", "outcome should be 'passed'");
    assert_eq!(records[0].duration_ms, 50, "duration should be 50ms");
    assert!(
        records[0].message.is_none(),
        "passed should have no message"
    );
    assert_eq!(records[0].eval_run_id, "er-test");
}

#[test]
fn records_from_report_failed() {
    let report = RunReport {
        passed: 0,
        failed: 1,
        skipped: 0,
        total_duration: Duration::from_millis(200),
        results: vec![ScenarioResult {
            meta: sample_meta("test-fail", "session"),
            outcome: ScenarioOutcome::Failed {
                duration: Duration::from_millis(150),
                error: crate::error::AssertionSnafu {
                    message: "test failure",
                }
                .build(),
            },
            sub_results: vec![],
        }],
        provenance: sample_provenance(),
    };
    let records = records_from_report(&report);
    assert_eq!(records.len(), 1, "should produce one record");
    assert!(!records[0].passed, "should not be passed");
    assert_eq!(records[0].outcome, "failed", "outcome should be 'failed'");
    assert!(records[0].message.is_some(), "failed should have message");
}

#[test]
fn records_from_report_skipped() {
    let report = RunReport {
        passed: 0,
        failed: 0,
        skipped: 1,
        total_duration: Duration::from_millis(10),
        results: vec![ScenarioResult {
            meta: sample_meta("test-skip", "cognitive"),
            outcome: ScenarioOutcome::Skipped {
                reason: coverage::SKIP_REASON_NO_AUTH_TOKEN.to_owned(),
            },
            sub_results: vec![],
        }],
        provenance: sample_provenance(),
    };
    let records = records_from_report(&report);
    assert_eq!(records.len(), 1, "should produce one record");
    assert!(!records[0].passed, "should not be passed");
    assert_eq!(records[0].outcome, "skipped", "outcome should be 'skipped'");
    assert_eq!(records[0].duration_ms, 0, "skipped should have 0 duration");
    assert_eq!(records[0].skip_kind, Some(SkipKind::MissingAuthToken));
    assert_eq!(records[0].skip_class, Some(SkipClass::Environmental));
}

#[test]
fn eval_record_serializes_to_json() {
    let record = EvalRecord {
        timestamp: "1234567890".to_owned(),
        eval_run_id: "er-1".to_owned(),
        provenance: sample_provenance(),
        eval_type: "health".to_owned(),
        scenario_id: "health-ok".to_owned(),
        passed: true,
        duration_ms: 50,
        outcome: "passed".to_owned(),
        message: None,
        required_for_coverage: true,
        skip_kind: None,
        skip_class: None,
        coverage: None,
        sub_results: vec![],
    };
    let json = serde_json::to_string(&record).expect("should serialize");
    assert!(
        json.contains("health-ok"),
        "JSON should contain scenario ID"
    );
    assert!(
        json.contains("eval_run_id"),
        "JSON should contain eval_run_id"
    );
    assert!(!json.contains("message"), "None message should be skipped");
}

#[test]
fn eval_record_with_message_serializes() {
    let record = EvalRecord {
        timestamp: "1234567890".to_owned(),
        eval_run_id: "er-1".to_owned(),
        provenance: sample_provenance(),
        eval_type: "cognitive".to_owned(),
        scenario_id: "test-fail".to_owned(),
        passed: false,
        duration_ms: 100,
        outcome: "failed".to_owned(),
        message: Some("assertion failed".to_owned()),
        required_for_coverage: true,
        skip_kind: None,
        skip_class: None,
        coverage: None,
        sub_results: vec![],
    };
    let json = serde_json::to_string(&record).expect("should serialize");
    assert!(
        json.contains("assertion failed"),
        "JSON should contain error message"
    );
}

#[test]
fn eval_record_does_not_serialize_token() {
    let provenance = EvalProvenance::new("er-1", "http://localhost")
        .with_redacted_args(&["--token".to_owned(), "secret-token".to_owned()]);
    let record = EvalRecord {
        timestamp: "1234567890".to_owned(),
        eval_run_id: "er-1".to_owned(),
        provenance,
        eval_type: "health".to_owned(),
        scenario_id: "health-ok".to_owned(),
        passed: true,
        duration_ms: 50,
        outcome: "passed".to_owned(),
        message: None,
        required_for_coverage: true,
        skip_kind: None,
        skip_class: None,
        coverage: None,
        sub_results: vec![],
    };
    let json = serde_json::to_string(&record).expect("should serialize");
    assert!(
        !json.contains("secret-token"),
        "token must not leak into JSONL"
    );
    assert!(
        json.contains("[REDACTED]"),
        "redacted placeholder should appear"
    );
}

#[test]
fn append_jsonl_creates_file() {
    let dir = std::env::temp_dir().join("aletheia-eval-test");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test-output.jsonl");
    let _ = std::fs::remove_file(&path);

    let records = vec![EvalRecord {
        timestamp: "1234567890".to_owned(),
        eval_run_id: "er-1".to_owned(),
        provenance: sample_provenance(),
        eval_type: "test".to_owned(),
        scenario_id: "test-1".to_owned(),
        passed: true,
        duration_ms: 10,
        outcome: "passed".to_owned(),
        message: None,
        required_for_coverage: true,
        skip_kind: None,
        skip_class: None,
        coverage: None,
        sub_results: vec![],
    }];

    let result = append_jsonl(&path, &records);
    assert!(result.is_ok(), "append_jsonl should succeed");

    let content = std::fs::read_to_string(&path).expect("should read file");
    assert!(content.contains("test-1"), "file should contain record");
    assert!(content.ends_with('\n'), "file should end with newline");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn append_jsonl_appends_to_existing() {
    let dir = std::env::temp_dir().join("aletheia-eval-test");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("test-append.jsonl");
    let _ = std::fs::remove_file(&path);

    let record1 = vec![EvalRecord {
        timestamp: "1".to_owned(),
        eval_run_id: "er-1".to_owned(),
        provenance: sample_provenance(),
        eval_type: "test".to_owned(),
        scenario_id: "first".to_owned(),
        passed: true,
        duration_ms: 10,
        outcome: "passed".to_owned(),
        message: None,
        required_for_coverage: true,
        skip_kind: None,
        skip_class: None,
        coverage: None,
        sub_results: vec![],
    }];
    let record2 = vec![EvalRecord {
        timestamp: "2".to_owned(),
        eval_run_id: "er-1".to_owned(),
        provenance: sample_provenance(),
        eval_type: "test".to_owned(),
        scenario_id: "second".to_owned(),
        passed: true,
        duration_ms: 20,
        outcome: "passed".to_owned(),
        message: None,
        required_for_coverage: true,
        skip_kind: None,
        skip_class: None,
        coverage: None,
        sub_results: vec![],
    }];

    append_jsonl(&path, &record1).expect("first append");
    append_jsonl(&path, &record2).expect("second append");

    let content = std::fs::read_to_string(&path).expect("should read file");
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2, "should have two lines");
    assert!(
        lines[0].contains("first"),
        "first line should be first record"
    );
    assert!(
        lines[1].contains("second"),
        "second line should be second record"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn now_iso8601_returns_nonempty() {
    let ts = now_iso8601();
    assert!(!ts.is_empty(), "timestamp should not be empty");
}

#[test]
fn now_iso8601_roundtrips_as_jiff_zoned() {
    let ts = now_iso8601();
    let parsed = jiff::Zoned::strptime("%Y-%m-%dT%H:%M:%S%.3f%:z", &ts)
        .expect("timestamp should parse as ISO 8601 UTC");
    assert_eq!(
        parsed.strftime("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string(),
        ts,
        "formatted timestamp should round-trip through jiff"
    );
}

#[test]
fn millis_from_duration_converts() {
    let d = Duration::from_millis(42);
    assert_eq!(millis_from_duration(&d), 42, "should convert to 42ms");
}

#[test]
fn append_jsonl_stamped_writes_meta_sibling() {
    let dir = std::env::temp_dir().join("aletheia-eval-stamped-test");
    create_dir(&dir);
    let path = dir.join("stamped-test.jsonl");
    let meta_path = dir.join("stamped-test.jsonl.meta.json");
    let manifest_path = dir.join("stamped-test.jsonl.manifest.json");
    remove_file_if_exists(&path);
    remove_file_if_exists(&meta_path);
    remove_file_if_exists(&manifest_path);

    let report = RunReport {
        passed: 2,
        failed: 1,
        skipped: 0,
        total_duration: std::time::Duration::from_millis(300),
        results: vec![],
        provenance: sample_provenance(),
    };

    append_jsonl_stamped(&path, &report).expect("stamped append should succeed");

    assert!(path.exists(), "JSONL file should exist after stamped write");
    assert!(
        meta_path.exists(),
        "meta JSON sibling should exist after stamped write"
    );

    let meta_content = std::fs::read_to_string(&meta_path).expect("should read meta file");
    let meta: mneme::meta::ArtefactMeta =
        serde_json::from_str(&meta_content).expect("meta should be valid JSON");
    assert!(
        meta.producer.starts_with("dokimion@"),
        "producer must start with 'dokimion@'"
    );
    assert_eq!(
        meta.row_counts.get("passed").copied(),
        Some(2),
        "meta should carry passed count"
    );

    remove_file_if_exists(&path);
    remove_file_if_exists(&meta_path);
    remove_file_if_exists(&manifest_path);
}

#[test]
fn append_jsonl_stamped_with_coverage_writes_denominators() {
    let dir = std::env::temp_dir().join("aletheia-eval-coverage-test");
    create_dir(&dir);
    let path = dir.join("stamped-coverage.jsonl");
    let meta_path = dir.join("stamped-coverage.jsonl.meta.json");
    let coverage_path = dir.join("stamped-coverage.jsonl.coverage.json");
    let manifest_path = dir.join("stamped-coverage.jsonl.manifest.json");
    remove_file_if_exists(&path);
    remove_file_if_exists(&meta_path);
    remove_file_if_exists(&coverage_path);
    remove_file_if_exists(&manifest_path);

    let report = RunReport {
        passed: 1,
        failed: 0,
        skipped: 1,
        total_duration: std::time::Duration::from_millis(100),
        results: vec![
            ScenarioResult {
                meta: sample_meta("coverage-pass", "health"),
                outcome: ScenarioOutcome::Passed {
                    duration: std::time::Duration::from_millis(50),
                },
                sub_results: vec![],
            },
            ScenarioResult {
                meta: sample_meta("coverage-skip", "session"),
                outcome: ScenarioOutcome::Skipped {
                    reason: coverage::SKIP_REASON_NO_AUTH_TOKEN.to_owned(),
                },
                sub_results: vec![],
            },
        ],
        provenance: sample_provenance(),
    };
    let coverage = coverage::Policy::Ci.evaluate(&report);

    append_jsonl_stamped_with_coverage(&path, &report, Some(&coverage))
        .expect("stamped append should succeed");

    let meta_content = std::fs::read_to_string(&meta_path).expect("should read meta file");
    let meta: mneme::meta::ArtefactMeta =
        serde_json::from_str(&meta_content).expect("meta should be valid JSON");
    assert_eq!(
        meta.row_counts.get("required").copied(),
        Some(2),
        "meta should carry required denominator"
    );
    assert_eq!(
        meta.row_counts.get("required_skipped").copied(),
        Some(1),
        "meta should carry skipped required count"
    );

    let coverage_content =
        std::fs::read_to_string(&coverage_path).expect("should read coverage file");
    assert!(coverage_content.contains("\"policy\": \"ci\""));
    assert!(coverage_content.contains("\"required_scenarios\": 2"));

    let jsonl_content = std::fs::read_to_string(&path).expect("should read JSONL file");
    assert!(jsonl_content.contains("\"skip_kind\":\"missing_auth_token\""));
    assert!(jsonl_content.contains("\"coverage\""));

    remove_file_if_exists(&path);
    remove_file_if_exists(&meta_path);
    remove_file_if_exists(&coverage_path);
    remove_file_if_exists(&manifest_path);
}

#[test]
fn append_jsonl_stamped_writes_tags_sibling() {
    let dir = std::env::temp_dir().join("aletheia-eval-tags-test");
    create_dir(&dir);
    let path = dir.join("stamped-tags.jsonl");
    let tags_path = dir.join("stamped-tags.jsonl.tags.json");
    let manifest_path = dir.join("stamped-tags.jsonl.manifest.json");
    remove_file_if_exists(&path);
    remove_file_if_exists(&tags_path);
    remove_file_if_exists(&manifest_path);

    let report = RunReport {
        passed: 1,
        failed: 0,
        skipped: 0,
        total_duration: std::time::Duration::from_millis(100),
        results: vec![ScenarioResult {
            meta: sample_meta("tag-test", "health"),
            outcome: ScenarioOutcome::Passed {
                duration: std::time::Duration::from_millis(50),
            },
            sub_results: vec![],
        }],
        provenance: sample_provenance(),
    };

    append_jsonl_stamped(&path, &report).expect("stamped append should succeed");

    assert!(path.exists(), "JSONL file should exist after stamped write");
    assert!(
        tags_path.exists(),
        "tags JSON sibling should exist after stamped write"
    );

    let tags_content = std::fs::read_to_string(&tags_path).expect("should read tags file");
    let tags: Vec<crate::tags::TagId> =
        serde_json::from_str(&tags_content).expect("tags should be valid JSON");
    assert!(
        tags.iter().any(|t| matches!(
            t,
            crate::tags::TagId::Outcome(crate::tags::OutcomeTag::Passed)
        )),
        "tags should contain passed outcome"
    );
    let manifest_content =
        std::fs::read_to_string(&manifest_path).expect("should read manifest file");
    let manifest: StampedJsonlManifest =
        serde_json::from_str(&manifest_content).expect("manifest should be valid JSON");
    assert_eq!(manifest.runs.len(), 1);
    assert_eq!(manifest.runs[0].tags_path, tags_path.display().to_string());

    remove_file_if_exists(&path);
    remove_file_if_exists(&tags_path);
    remove_file_if_exists(&manifest_path);
}

#[test]
fn append_jsonl_stamped_is_idempotent_for_run_id() {
    let dir = std::env::temp_dir().join("aletheia-eval-idempotent-test");
    create_dir(&dir);
    let path = dir.join("stamped-idempotent.jsonl");
    let meta_path = dir.join("stamped-idempotent.jsonl.meta.json");
    let tags_path = dir.join("stamped-idempotent.jsonl.tags.json");
    let manifest_path = dir.join("stamped-idempotent.jsonl.manifest.json");
    remove_file_if_exists(&path);
    remove_file_if_exists(&meta_path);
    remove_file_if_exists(&tags_path);
    remove_file_if_exists(&manifest_path);

    let report = RunReport {
        passed: 1,
        failed: 0,
        skipped: 0,
        total_duration: std::time::Duration::from_millis(100),
        results: vec![ScenarioResult {
            meta: sample_meta("idempotent-pass", "health"),
            outcome: ScenarioOutcome::Passed {
                duration: std::time::Duration::from_millis(50),
            },
            sub_results: vec![],
        }],
        provenance: sample_provenance(),
    };

    append_jsonl_stamped(&path, &report).expect("first stamped append should succeed");
    append_jsonl_stamped(&path, &report).expect("second stamped append should be idempotent");

    let jsonl_content = std::fs::read_to_string(&path).expect("should read JSONL file");
    assert_eq!(jsonl_content.lines().count(), 1);
    let manifest_content =
        std::fs::read_to_string(&manifest_path).expect("should read manifest file");
    let manifest: StampedJsonlManifest =
        serde_json::from_str(&manifest_content).expect("manifest should be valid JSON");
    assert_eq!(manifest.runs.len(), 1);
    assert!(!manifest.runs[0].recovered_from_partial_write);

    remove_file_if_exists(&path);
    remove_file_if_exists(&meta_path);
    remove_file_if_exists(&tags_path);
    remove_file_if_exists(&manifest_path);
}

#[test]
fn append_jsonl_stamped_recovers_jsonl_without_sidecars() {
    let dir = std::env::temp_dir().join("aletheia-eval-recovery-test");
    create_dir(&dir);
    let path = dir.join("stamped-recovery.jsonl");
    let meta_path = dir.join("stamped-recovery.jsonl.meta.json");
    let tags_path = dir.join("stamped-recovery.jsonl.tags.json");
    let manifest_path = dir.join("stamped-recovery.jsonl.manifest.json");
    remove_file_if_exists(&path);
    remove_file_if_exists(&meta_path);
    remove_file_if_exists(&tags_path);
    remove_file_if_exists(&manifest_path);

    let report = RunReport {
        passed: 1,
        failed: 0,
        skipped: 0,
        total_duration: std::time::Duration::from_millis(100),
        results: vec![ScenarioResult {
            meta: sample_meta("recovery-pass", "health"),
            outcome: ScenarioOutcome::Passed {
                duration: std::time::Duration::from_millis(50),
            },
            sub_results: vec![],
        }],
        provenance: sample_provenance(),
    };
    let records = records_from_report(&report);
    append_jsonl(&path, &records).expect("simulate crash after JSONL append");

    append_jsonl_stamped(&path, &report).expect("stamped append should recover sidecars");

    let jsonl_content = std::fs::read_to_string(&path).expect("should read JSONL file");
    assert_eq!(jsonl_content.lines().count(), records.len());
    assert!(meta_path.exists(), "meta sidecar should be recovered");
    assert!(tags_path.exists(), "tags sidecar should be recovered");
    let manifest_content =
        std::fs::read_to_string(&manifest_path).expect("should read manifest file");
    let manifest: StampedJsonlManifest =
        serde_json::from_str(&manifest_content).expect("manifest should be valid JSON");
    assert_eq!(manifest.runs.len(), 1);
    assert!(manifest.runs[0].recovered_from_partial_write);

    remove_file_if_exists(&path);
    remove_file_if_exists(&meta_path);
    remove_file_if_exists(&tags_path);
    remove_file_if_exists(&manifest_path);
}
