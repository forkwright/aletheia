//! Tests for the typed-tag namespace.

use std::collections::HashSet;
use std::time::Duration;

use crate::runner::RunReport;
use crate::scenario::{ScenarioMeta, ScenarioOutcome, ScenarioResult};
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
    }
}

#[test]
fn test_tags_for_completed_run() {
    let report = RunReport {
        passed: 1,
        failed: 0,
        skipped: 0,
        total_duration: Duration::from_millis(500),
        results: vec![ScenarioResult {
            meta: sample_meta("test-pass", "health"),
            outcome: ScenarioOutcome::Passed {
                duration: Duration::from_millis(50),
            },
        }],
    };
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
    let report = RunReport {
        passed: 0,
        failed: 1,
        skipped: 0,
        total_duration: Duration::from_secs(5),
        results: vec![ScenarioResult {
            meta: sample_meta("test-fail", "session"),
            outcome: ScenarioOutcome::Failed {
                duration: Duration::from_millis(150),
                error: crate::error::AssertionSnafu {
                    message: "test failure",
                }
                .build(),
            },
        }],
    };
    let tags = tag_eval_result(&report);
    assert!(tags.contains(&TagId::Outcome(OutcomeTag::Failed)));
    assert!(!tags.contains(&TagId::Outcome(OutcomeTag::Passed)));
}

#[test]
fn test_no_tools_fired() {
    // Adapted to reality: no auth/nous/criteria means those tags are absent.
    let report = RunReport {
        passed: 1,
        failed: 0,
        skipped: 0,
        total_duration: Duration::from_millis(100),
        results: vec![ScenarioResult {
            meta: sample_meta("test-noauth", "cognitive"),
            outcome: ScenarioOutcome::Passed {
                duration: Duration::from_millis(50),
            },
        }],
    };
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
        RunReport {
            passed: 1,
            failed: 0,
            skipped: 0,
            total_duration: Duration::from_millis(100),
            results: vec![ScenarioResult {
                meta: ScenarioMeta {
                    id: "r0",
                    description: "r0",
                    category: "health",
                    requires_auth: true,
                    requires_nous: false,
                    expected_contains: None,
                    expected_pattern: None,
                },
                outcome: ScenarioOutcome::Passed {
                    duration: Duration::from_millis(50),
                },
            }],
        },
        // Report 1: health, failed
        RunReport {
            passed: 0,
            failed: 1,
            skipped: 0,
            total_duration: Duration::from_millis(200),
            results: vec![ScenarioResult {
                meta: sample_meta("r1", "health"),
                outcome: ScenarioOutcome::Failed {
                    duration: Duration::from_millis(100),
                    error: crate::error::AssertionSnafu { message: "fail" }.build(),
                },
            }],
        },
        // Report 2: session, passed, has criteria
        RunReport {
            passed: 1,
            failed: 0,
            skipped: 0,
            total_duration: Duration::from_millis(300),
            results: vec![ScenarioResult {
                meta: ScenarioMeta {
                    id: "r2",
                    description: "r2",
                    category: "session",
                    requires_auth: false,
                    requires_nous: false,
                    expected_contains: Some("hello"),
                    expected_pattern: None,
                },
                outcome: ScenarioOutcome::Passed {
                    duration: Duration::from_millis(50),
                },
            }],
        },
        // Report 3: session, passed, requires_nous
        RunReport {
            passed: 1,
            failed: 0,
            skipped: 0,
            total_duration: Duration::from_millis(400),
            results: vec![ScenarioResult {
                meta: ScenarioMeta {
                    id: "r3",
                    description: "r3",
                    category: "session",
                    requires_auth: false,
                    requires_nous: true,
                    expected_contains: None,
                    expected_pattern: None,
                },
                outcome: ScenarioOutcome::Passed {
                    duration: Duration::from_millis(50),
                },
            }],
        },
        // Report 4: cognitive, skipped
        RunReport {
            passed: 0,
            failed: 0,
            skipped: 1,
            total_duration: Duration::from_millis(50),
            results: vec![ScenarioResult {
                meta: sample_meta("r4", "cognitive"),
                outcome: ScenarioOutcome::Skipped {
                    reason: "no auth".to_owned(),
                },
            }],
        },
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
