//! Tests for detailed instinct requirements: aggregation, patterns, serialization.
#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::cast_possible_truncation,
    reason = "proptest range 5..=30 is safe to cast to u32"
)]
#![expect(
    clippy::as_conversions,
    clippy::indexing_slicing,
    reason = "knowledge engine: ported codebase with numeric casts and direct indexing throughout"
)]
use super::super::*;
use crate::knowledge::parse_timestamp;

fn ts(s: &str) -> jiff::Timestamp {
    parse_timestamp(s).expect("valid test timestamp")
}

fn make_observation(
    tool_name: &str,
    context: &str,
    outcome: ToolOutcome,
    timestamp: &str,
) -> ToolObservation {
    ToolObservation {
        tool_name: tool_name.to_owned(),
        parameters: serde_json::json!({}),
        outcome,
        context_summary: context.to_owned(),
        nous_id: "test-nous".to_owned(),
        observed_at: ts(timestamp),
    }
}

#[test]
fn empty_parameters_does_not_panic() {
    let params = serde_json::json!({});
    let sanitized = sanitize_parameters(&params);
    assert!(
        sanitized.as_object().expect("should be object").is_empty(),
        "sanitizing empty params returns empty object"
    );
}

#[test]
fn aggregation_with_empty_parameters_does_not_panic() {
    let observations: Vec<_> = (0..5)
        .map(|i| ToolObservation {
            tool_name: "read_file".to_owned(),
            parameters: serde_json::json!({}),
            outcome: ToolOutcome::Success,
            context_summary: "reading code file".to_owned(),
            nous_id: "nous-1".to_owned(),
            observed_at: ts(&format!("2026-03-{:02}T10:00:00Z", i + 1)),
        })
        .collect();
    let patterns = aggregate_observations(&observations);
    assert_eq!(
        patterns.len(),
        1,
        "5 successful calls with empty params should produce a pattern"
    );
}

#[test]
fn four_successful_calls_does_not_trigger_pattern() {
    let observations: Vec<_> = (0..4)
        .map(|i| {
            make_observation(
                "grep",
                "code search",
                ToolOutcome::Success,
                &format!("2026-03-{:02}T12:00:00Z", i + 1),
            )
        })
        .collect();
    let patterns = aggregate_observations(&observations);
    assert!(
        patterns.is_empty(),
        "4 observations is below the minimum threshold of {MIN_OBSERVATIONS}"
    );
}

#[test]
fn five_successful_calls_at_threshold_triggers_pattern() {
    let observations: Vec<_> = (0..5)
        .map(|i| {
            make_observation(
                "grep",
                "code search",
                ToolOutcome::Success,
                &format!("2026-03-{:02}T12:00:00Z", i + 1),
            )
        })
        .collect();
    let patterns = aggregate_observations(&observations);
    assert_eq!(
        patterns.len(),
        1,
        "exactly {MIN_OBSERVATIONS} successful calls should meet the minimum threshold"
    );
    assert_eq!(
        patterns[0].success_count, MIN_OBSERVATIONS,
        "success count should equal minimum observations"
    );
}

#[test]
fn ten_calls_below_80_percent_success_rate_does_not_trigger() {
    let mut observations: Vec<_> = (0..7)
        .map(|i| {
            make_observation(
                "grep",
                "code search",
                ToolOutcome::Success,
                &format!("2026-03-{:02}T10:00:00Z", i + 1),
            )
        })
        .collect();
    for i in 0..3 {
        observations.push(make_observation(
            "grep",
            "code search",
            ToolOutcome::Failure {
                error: "not found".to_owned(),
            },
            &format!("2026-03-{:02}T11:00:00Z", i + 1),
        ));
    }
    let patterns = aggregate_observations(&observations);
    assert!(
        patterns.is_empty(),
        "70% success rate should not trigger a pattern (minimum is {:.0}%)",
        MIN_SUCCESS_RATE * 100.0
    );
}

#[test]
fn ten_calls_at_80_percent_success_rate_boundary_triggers_pattern() {
    let mut observations: Vec<_> = (0..8)
        .map(|i| {
            make_observation(
                "grep",
                "code search",
                ToolOutcome::Success,
                &format!("2026-03-{:02}T10:00:00Z", i + 1),
            )
        })
        .collect();
    for i in 0..2 {
        observations.push(make_observation(
            "grep",
            "code search",
            ToolOutcome::Failure {
                error: "not found".to_owned(),
            },
            &format!("2026-03-{:02}T11:00:00Z", i + 1),
        ));
    }
    let patterns = aggregate_observations(&observations);
    assert_eq!(
        patterns.len(),
        1,
        "80% success rate should meet the minimum threshold"
    );
    assert_eq!(patterns[0].success_count, 8, "success count should be 8");
    assert_eq!(patterns[0].total_count, 10, "total count should be 10");
    assert!(
        (patterns[0].success_rate - 0.80).abs() < 1e-10,
        "success rate should be exactly 80%"
    );
}

#[test]
fn sanitize_strips_token_field_name() {
    let params = serde_json::json!({
        "token": "bearer-abc123-not-real",
        "auth_token": "header-value-not-real",
        "query": "normal parameter"
    });
    let sanitized = sanitize_parameters(&params);
    let obj = sanitized.as_object().expect("should be object");
    assert_eq!(obj["token"], "[REDACTED]", "token field should be redacted");
    assert_eq!(
        obj["auth_token"], "[REDACTED]",
        "auth_token field should be redacted"
    );
    assert_eq!(
        obj["query"], "normal parameter",
        "non-secret field should pass through"
    );
}

#[test]
fn sanitize_200_char_value_not_truncated() {
    let exactly_limit = "x".repeat(MAX_PARAM_VALUE_LEN);
    let params = serde_json::json!({"content": exactly_limit.clone()});
    let sanitized = sanitize_parameters(&params);
    let content = sanitized["content"].as_str().expect("should be string");
    assert_eq!(
        content, exactly_limit,
        "value of exactly {MAX_PARAM_VALUE_LEN} chars should not be truncated"
    );
    assert!(
        !content.ends_with("..."),
        "value at limit should not have '...' suffix"
    );
}

#[test]
fn sanitize_201_char_value_is_truncated() {
    let over_limit = "y".repeat(MAX_PARAM_VALUE_LEN + 1);
    let params = serde_json::json!({"content": over_limit});
    let sanitized = sanitize_parameters(&params);
    let content = sanitized["content"].as_str().expect("should be string");
    assert!(
        content.ends_with("..."),
        "value exceeding {MAX_PARAM_VALUE_LEN} chars should end with '...'"
    );
    assert_eq!(
        content.len(),
        MAX_PARAM_VALUE_LEN + 3,
        "truncated value should be {MAX_PARAM_VALUE_LEN} chars + '...'"
    );
}

#[test]
fn sanitize_array_parameter_values_element_by_element() {
    let long_item = "z".repeat(MAX_PARAM_VALUE_LEN + 50);
    let params = serde_json::json!({
        "items": [long_item, "short item", "another normal value"]
    });
    let sanitized = sanitize_parameters(&params);
    let items = sanitized["items"].as_array().expect("should be array");
    assert_eq!(items.len(), 3, "array length should be preserved");
    let first = items[0].as_str().expect("should be string");
    assert!(
        first.ends_with("..."),
        "long array element should be truncated"
    );
    assert!(
        first.len() <= MAX_PARAM_VALUE_LEN + 3,
        "truncated element length should be within limit"
    );
    assert_eq!(
        items[1].as_str().expect("string"),
        "short item",
        "short array element should pass through"
    );
    assert_eq!(
        items[2].as_str().expect("string"),
        "another normal value",
        "normal array element should pass through"
    );
}

#[test]
fn different_nous_observations_aggregate_together_without_filter() {
    // NOTE: aggregate_observations groups by (tool_name, context_category) only: not nous_id.
    // Callers must filter by nous_id before calling to get per-nous patterns.
    let mut observations: Vec<ToolObservation> = (0..3)
        .map(|i| ToolObservation {
            tool_name: "grep".to_owned(),
            parameters: serde_json::json!({}),
            outcome: ToolOutcome::Success,
            context_summary: "code search".to_owned(),
            nous_id: "nous-alice".to_owned(),
            observed_at: ts(&format!("2026-03-{:02}T10:00:00Z", i + 1)),
        })
        .collect();
    observations.extend((0..3).map(|i| ToolObservation {
        tool_name: "grep".to_owned(),
        parameters: serde_json::json!({}),
        outcome: ToolOutcome::Success,
        context_summary: "code search".to_owned(),
        nous_id: "nous-bob".to_owned(),
        observed_at: ts(&format!("2026-03-{:02}T11:00:00Z", i + 1)),
    }));
    let all = aggregate_observations(&observations);
    assert_eq!(
        all.len(),
        1,
        "mixed-nous observations aggregate into one pattern"
    );
    assert_eq!(all[0].total_count, 6, "combined total count should be 6");
    let alice: Vec<_> = observations
        .iter()
        .filter(|o| o.nous_id == "nous-alice")
        .cloned()
        .collect();
    let alice_patterns = aggregate_observations(&alice);
    assert!(
        alice_patterns.is_empty(),
        "3 observations below threshold when filtered per-nous"
    );
}

#[test]
fn two_tools_identical_parameters_produce_separate_patterns() {
    let mut observations = Vec::new();
    for i in 0..5 {
        observations.push(ToolObservation {
            tool_name: "grep".to_owned(),
            parameters: serde_json::json!({"pattern": "TODO"}),
            outcome: ToolOutcome::Success,
            context_summary: "code search".to_owned(),
            nous_id: "nous-1".to_owned(),
            observed_at: ts(&format!("2026-03-{:02}T10:00:00Z", i + 1)),
        });
        observations.push(ToolObservation {
            tool_name: "read_file".to_owned(),
            parameters: serde_json::json!({"pattern": "TODO"}),
            outcome: ToolOutcome::Success,
            context_summary: "code search".to_owned(),
            nous_id: "nous-1".to_owned(),
            observed_at: ts(&format!("2026-03-{:02}T11:00:00Z", i + 1)),
        });
    }
    let patterns = aggregate_observations(&observations);
    assert_eq!(
        patterns.len(),
        2,
        "different tool names produce separate patterns even with identical parameters"
    );
    let tool_names: std::collections::HashSet<_> =
        patterns.iter().map(|p| p.tool_name.as_str()).collect();
    assert!(
        tool_names.contains("grep"),
        "grep pattern should be present"
    );
    assert!(
        tool_names.contains("read_file"),
        "read_file pattern should be present"
    );
}

#[test]
fn tool_name_matching_is_case_sensitive() {
    let mut observations = Vec::new();
    for i in 0..5 {
        observations.push(make_observation(
            "Grep",
            "code search",
            ToolOutcome::Success,
            &format!("2026-03-{:02}T10:00:00Z", i + 1),
        ));
    }
    for i in 0..5 {
        observations.push(make_observation(
            "grep",
            "code search",
            ToolOutcome::Success,
            &format!("2026-03-{:02}T11:00:00Z", i + 1),
        ));
    }
    let patterns = aggregate_observations(&observations);
    assert_eq!(
        patterns.len(),
        2,
        "'Grep' and 'grep' are distinct tool names — case-sensitive matching"
    );
}

#[test]
fn context_category_json_serde_roundtrip() {
    let categories = [
        ContextCategory::Code,
        ContextCategory::Research,
        ContextCategory::System,
        ContextCategory::Memory,
        ContextCategory::Communication,
        ContextCategory::Other,
    ];
    for cat in categories {
        let json = serde_json::to_string(&cat).expect("serialize");
        let back: ContextCategory = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cat, back, "JSON serde roundtrip failed for {cat:?}");
    }
}

#[test]
fn context_summary_exactly_at_limit_not_truncated() {
    let exactly_limit = "a".repeat(MAX_CONTEXT_SUMMARY_LEN);
    let result = truncate_context_summary(&exactly_limit);
    assert_eq!(
        result, exactly_limit,
        "context summary of exactly {MAX_CONTEXT_SUMMARY_LEN} chars should not be truncated"
    );
    assert!(
        !result.ends_with("..."),
        "at-limit summary should not have '...' suffix"
    );
}

#[test]
fn context_category_unknown_string_parses_to_other() {
    assert_eq!(
        ContextCategory::from_str_lossy("completely_unknown"),
        ContextCategory::Other,
        "unknown string should fall back to Other"
    );
    assert_eq!(
        ContextCategory::from_str_lossy(""),
        ContextCategory::Other,
        "empty string should fall back to Other"
    );
    assert_eq!(
        ContextCategory::from_str_lossy("CODE"),
        ContextCategory::Other,
        "uppercase variant should fall back to Other (case-sensitive)"
    );
}

mod proptests {
    use crate::instinct::{
        MAX_PARAM_VALUE_LEN, ToolObservation, ToolOutcome, aggregate_observations,
        sanitize_parameters,
    };
    use proptest::prelude::*;

    proptest! {
        /// ToolOutcome stored-string roundtrip holds for any content in error/note fields.
        ///
        /// Verifies that arbitrary alphanumeric strings survive the
        /// `as_stored_string` → `from_stored_string` roundtrip for all three variants.
        #[test]
        fn outcome_stored_string_roundtrip_any_content(
            error in "[a-zA-Z0-9 ]{0,50}",
            note in "[a-zA-Z0-9 ]{0,50}",
        ) {
            let outcomes = [
                ToolOutcome::Success,
                ToolOutcome::Failure { error: error.clone() },
                ToolOutcome::Partial { note: note.clone() },
            ];
            for outcome in &outcomes {
                let stored = outcome.as_stored_string();
                let back = ToolOutcome::from_stored_string(&stored);
                prop_assert_eq!(outcome, &back, "roundtrip failed for {:?}", outcome);
            }
        }

        /// Sanitized non-secret string values are bounded: length ≤ MAX_PARAM_VALUE_LEN + 3.
        ///
        /// The bound accounts for the "..." truncation suffix appended to long values.
        #[test]
        fn sanitize_output_string_length_bounded(s in "[a-zA-Z0-9 ]{0,300}") {
            let params = serde_json::json!({"content": s});
            let sanitized = sanitize_parameters(&params);
            let content = sanitized["content"].as_str().expect("string");
            prop_assert!(
                content.len() <= MAX_PARAM_VALUE_LEN + 3,
                "sanitized length {} exceeds MAX_PARAM_VALUE_LEN + 3 = {}",
                content.len(),
                MAX_PARAM_VALUE_LEN + 3
            );
        }

        /// For any N ≥ 5 observations for the same tool and context at 100% success,
        /// the aggregated pattern's `total_count` equals N exactly.
        #[test]
        fn aggregation_total_count_equals_observation_count(n in 5_usize..=30_usize) {
            let observations: Vec<_> = (0..n)
                .map(|_| ToolObservation {
                    tool_name: "grep".to_owned(),
                    parameters: serde_json::json!({}),
                    outcome: ToolOutcome::Success,
                    context_summary: "code search".to_owned(),
                    nous_id: "nous-1".to_owned(),
                    observed_at: jiff::Timestamp::UNIX_EPOCH,
                })
                .collect();
            let patterns = aggregate_observations(&observations);
            prop_assert_eq!(patterns.len(), 1, "should produce exactly one pattern");
            prop_assert_eq!(
                patterns[0].total_count,
                u32::try_from(n).unwrap_or(u32::MAX),
                "total_count should equal the number of observations"
            );
        }
    }
}
