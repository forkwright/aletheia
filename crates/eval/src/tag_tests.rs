//! Tests for the typed-tag namespace.

use std::collections::HashSet;
use std::time::Duration;

use crate::provenance::EvalProvenance;
use crate::runner::RunReport;
use crate::scenario::{ScenarioClassification, ScenarioMeta, ScenarioOutcome, ScenarioResult};
use crate::tags::{DurationBand, OutcomeTag, SizeBand, TagId, tag_eval_result};

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
    EvalProvenance::new("er-tag-test", "http://localhost")
}

fn sample_result(meta: ScenarioMeta, outcome: ScenarioOutcome) -> ScenarioResult {
    ScenarioResult {
        meta,
        outcome,
        sub_results: Vec::new(),
    }
}

fn sample_report(
    passed: usize,
    failed: usize,
    skipped: usize,
    total_duration: Duration,
    results: Vec<ScenarioResult>,
) -> RunReport {
    RunReport {
        passed,
        failed,
        skipped,
        total_duration,
        results,
        provenance: sample_provenance(),
    }
}

#[test]
fn test_tags_for_completed_run() {
    let report = sample_report(
        1,
        0,
        0,
        Duration::from_millis(500),
        vec![sample_result(
            sample_meta("test-pass", "health"),
            ScenarioOutcome::Passed {
                duration: Duration::from_millis(50),
            },
        )],
    );
    let tags = tag_eval_result(&report);
    assert!(tags.contains(&TagId::Outcome(OutcomeTag::Passed)));
    assert!(tags.contains(&TagId::Category {
        name: "health".to_owned()
    }));
    assert!(tags.contains(&TagId::SizeBand(SizeBand::Single)));
    assert!(tags.contains(&TagId::DurationBand(DurationBand::Low)));
}

#[test]
fn test_tags_for_failed_run() {
    let report = sample_report(
        0,
        1,
        0,
        Duration::from_secs(5),
        vec![sample_result(
            sample_meta("test-fail", "session"),
            ScenarioOutcome::Failed {
                duration: Duration::from_millis(150),
                error: crate::error::AssertionSnafu {
                    message: "test failure",
                }
                .build(),
            },
        )],
    );
    let tags = tag_eval_result(&report);
    assert!(tags.contains(&TagId::Outcome(OutcomeTag::Failed)));
    assert!(!tags.contains(&TagId::Outcome(OutcomeTag::Passed)));
}

#[test]
fn test_no_tools_fired() {
    // Adapted to reality: no auth/nous/criteria means those tags are absent.
    let report = sample_report(
        1,
        0,
        0,
        Duration::from_millis(100),
        vec![sample_result(
            sample_meta("test-noauth", "cognitive"),
            ScenarioOutcome::Passed {
                duration: Duration::from_millis(50),
            },
        )],
    );
    let tags = tag_eval_result(&report);
    assert!(!tags.contains(&TagId::RequiresAuth));
    assert!(!tags.contains(&TagId::RequiresNous));
    assert!(!tags.contains(&TagId::HasCriteria));
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test data setup for five synthetic reports"
)]
fn test_set_membership_filtering() {
    let reports = [
        // Report 0: health, passed, requires_auth
        sample_report(
            1,
            0,
            0,
            Duration::from_millis(100),
            vec![sample_result(
                ScenarioMeta {
                    id: "r0",
                    description: "r0",
                    category: "health",
                    requires_auth: true,
                    requires_nous: false,
                    expected_contains: None,
                    expected_pattern: None,
                    classification: ScenarioClassification::Assertive,
                },
                ScenarioOutcome::Passed {
                    duration: Duration::from_millis(50),
                },
            )],
        ),
        // Report 1: health, failed
        sample_report(
            0,
            1,
            0,
            Duration::from_millis(200),
            vec![sample_result(
                sample_meta("r1", "health"),
                ScenarioOutcome::Failed {
                    duration: Duration::from_millis(100),
                    error: crate::error::AssertionSnafu { message: "fail" }.build(),
                },
            )],
        ),
        // Report 2: session, passed, has criteria
        sample_report(
            1,
            0,
            0,
            Duration::from_millis(300),
            vec![sample_result(
                ScenarioMeta {
                    id: "r2",
                    description: "r2",
                    category: "session",
                    requires_auth: false,
                    requires_nous: false,
                    expected_contains: Some("hello"),
                    expected_pattern: None,
                    classification: ScenarioClassification::Assertive,
                },
                ScenarioOutcome::Passed {
                    duration: Duration::from_millis(50),
                },
            )],
        ),
        // Report 3: session, passed, requires_nous
        sample_report(
            1,
            0,
            0,
            Duration::from_millis(400),
            vec![sample_result(
                ScenarioMeta {
                    id: "r3",
                    description: "r3",
                    category: "session",
                    requires_auth: false,
                    requires_nous: true,
                    expected_contains: None,
                    expected_pattern: None,
                    classification: ScenarioClassification::Assertive,
                },
                ScenarioOutcome::Passed {
                    duration: Duration::from_millis(50),
                },
            )],
        ),
        // Report 4: cognitive, skipped
        sample_report(
            0,
            0,
            1,
            Duration::from_millis(50),
            vec![sample_result(
                sample_meta("r4", "cognitive"),
                ScenarioOutcome::Skipped {
                    reason: "no auth".to_owned(),
                },
            )],
        ),
    ];

    let query: HashSet<TagId> = [
        TagId::Outcome(OutcomeTag::Passed),
        TagId::Category {
            name: "health".to_owned(),
        },
    ]
    .into_iter()
    .collect();

    let matching: Vec<usize> = reports
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            let tags: HashSet<TagId> = tag_eval_result(r).into_iter().collect();
            query.is_subset(&tags)
        })
        .map(|(i, _)| i)
        .collect();

    assert_eq!(matching, vec![0], "only report 0 is passed + health");
}
