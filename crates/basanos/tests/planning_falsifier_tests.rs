//! Integration tests for PLANNING/missing-falsifier.

#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(clippy::indexing_slicing, reason = "test assertions")]

use std::fs;
use std::path::PathBuf;

use basanos::rules::{Rule, planning::MissingFalsifierRule};

fn temp_project() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn write(root: &tempfile::TempDir, path: &str, content: &str) -> PathBuf {
    let full = root.path().join(path);
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(&full, content).expect("write");
    full
}

#[test]
fn missing_falsification_section_is_flagged() {
    let tmp = temp_project();
    write(
        &tmp,
        "phases/01-test/PLAN.md",
        "# Phase 01\n\n## Success criteria\n- Criterion A\n",
    );

    let rule = MissingFalsifierRule;
    let violations = rule.check(tmp.path().to_str().unwrap()).unwrap();

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].rule, "PLANNING/missing-falsifier");
    assert!(
        violations[0]
            .message
            .contains("no ## Falsification section"),
        "got: {}",
        violations[0].message
    );
}

#[test]
fn complete_falsification_passes() {
    let tmp = temp_project();
    write(
        &tmp,
        "phases/01-test/PLAN.md",
        r"# Phase 01

## Success criteria
- Latency p99 < 100 ms

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| Latency p99 < 100 ms | Benchmark shows p99 >= 100 ms |
",
    );

    let rule = MissingFalsifierRule;
    let violations = rule.check(tmp.path().to_str().unwrap()).unwrap();

    assert!(
        violations.is_empty(),
        "expected no violations, got: {violations:?}"
    );
}

#[test]
fn partial_falsification_is_flagged() {
    let tmp = temp_project();
    write(
        &tmp,
        "phases/01-test/PLAN.md",
        r"# Phase 01

## Success criteria
- Latency p99 < 100 ms
- Zero data loss on crash

## Falsification

| Criterion | Falsifier |
|-----------|-----------|
| Latency p99 < 100 ms | Benchmark shows p99 >= 100 ms |
",
    );

    let rule = MissingFalsifierRule;
    let violations = rule.check(tmp.path().to_str().unwrap()).unwrap();

    assert_eq!(violations.len(), 1);
    assert!(
        violations[0].message.contains("Zero data loss on crash"),
        "got: {}",
        violations[0].message
    );
}

#[test]
fn vision_doc_unfalsifiable_adjective_is_flagged() {
    let tmp = temp_project();
    write(
        &tmp,
        "vision.md",
        "# Vision\n\nWe build a world-class system.\n",
    );

    let rule = MissingFalsifierRule;
    let violations = rule.check(tmp.path().to_str().unwrap()).unwrap();

    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].rule, "PLANNING/unfalsifiable-claim");
    assert!(violations[0].message.contains("world-class"));
}

#[test]
fn roadmap_unfalsifiable_adjective_is_flagged() {
    let tmp = temp_project();
    write(
        &tmp,
        "ROADMAP.md",
        "# Roadmap\n\nGoal: scalable architecture.\n",
    );

    let rule = MissingFalsifierRule;
    let violations = rule.check(tmp.path().to_str().unwrap()).unwrap();

    assert!(!violations.is_empty());
    let v = violations
        .iter()
        .find(|v| v.message.contains("scalable"))
        .expect("scalable violation");
    assert_eq!(v.rule, "PLANNING/unfalsifiable-claim");
}

#[test]
fn no_criteria_means_no_violation() {
    let tmp = temp_project();
    write(
        &tmp,
        "phases/01-test/PLAN.md",
        "# Phase 01\n\n## Scope\n- Do thing\n",
    );

    let rule = MissingFalsifierRule;
    let violations = rule.check(tmp.path().to_str().unwrap()).unwrap();

    assert!(violations.is_empty());
}
