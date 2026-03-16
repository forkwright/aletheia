#![expect(clippy::expect_used, reason = "test assertions")]
#![allow(
    clippy::cast_possible_truncation,
    reason = "proptest range 5..=30 is safe to cast to u32"
)]
use super::*;
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

// --- 1. Observation recording fields ---

#[test]
fn observation_stores_correct_fields() {
    let obs = ToolObservation {
        tool_name: "grep".to_owned(),
        parameters: serde_json::json!({"pattern": "TODO", "path": "/src"}),
        outcome: ToolOutcome::Success,
        context_summary: "searching for TODOs in code".to_owned(),
        nous_id: "nous-1".to_owned(),
        observed_at: ts("2026-03-01T12:00:00Z"),
    };
    assert_eq!(obs.tool_name, "grep");
    assert_eq!(obs.nous_id, "nous-1");
    assert!(obs.outcome.is_success());
    assert_eq!(obs.context_summary, "searching for TODOs in code");
}

// --- 2. Aggregation: 10 successful calls → pattern ---

#[test]
fn aggregation_creates_pattern_from_successful_calls() {
    let observations: Vec<_> = (0..10)
        .map(|i| {
            make_observation(
                "grep",
                "searching code files",
                ToolOutcome::Success,
                &format!("2026-03-{:02}T12:00:00Z", i + 1),
            )
        })
        .collect();

    let patterns = aggregate_observations(&observations);
    assert_eq!(patterns.len(), 1);
    assert_eq!(patterns[0].tool_name, "grep");
    assert_eq!(patterns[0].context_type, "code");
    assert_eq!(patterns[0].success_count, 10);
    assert_eq!(patterns[0].total_count, 10);
    assert!((patterns[0].success_rate - 1.0).abs() < f64::EPSILON);
}

// --- 3. Threshold: below minimum observations ---

#[test]
fn aggregation_no_pattern_below_minimum_observations() {
    let observations: Vec<_> = (0..3)
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
        "3 observations should not produce a pattern (minimum is 5)"
    );
}

// --- 4. Success rate threshold ---

#[test]
fn aggregation_no_pattern_below_success_rate() {
    let mut observations: Vec<_> = (0..4)
        .map(|i| {
            make_observation(
                "web_search",
                "research topic",
                ToolOutcome::Success,
                &format!("2026-03-{:02}T12:00:00Z", i + 1),
            )
        })
        .collect();
    // Add 6 failures → 4/10 = 40% success rate
    for i in 4..10 {
        observations.push(make_observation(
            "web_search",
            "research topic",
            ToolOutcome::Failure {
                error: "timeout".to_owned(),
            },
            &format!("2026-03-{:02}T12:00:00Z", i + 1),
        ));
    }

    let patterns = aggregate_observations(&observations);
    assert!(
        patterns.is_empty(),
        "40% success rate should not produce a pattern (minimum is 80%)"
    );
}

// --- 5. Fact content generation ---

#[test]
fn pattern_generates_correct_fact_content() {
    let pattern = BehavioralPattern {
        pattern: String::new(),
        tool_name: "grep".to_owned(),
        context_type: "code".to_owned(),
        success_count: 8,
        total_count: 10,
        success_rate: 0.8,
        first_observed: ts("2026-03-01T00:00:00Z"),
        last_observed: ts("2026-03-10T00:00:00Z"),
    };
    let content = pattern.to_fact_content();
    assert!(content.contains("code"));
    assert!(content.contains("grep"));
    assert!(content.contains("80%"));
    assert!(content.contains("n=10"));
}

// --- 6. Sanitization: secrets stripped ---

#[test]
fn sanitize_strips_secret_parameters() {
    let params = serde_json::json!({
        "url": "https://acme.corp/api",
        "api_key": "test-key-not-real",
        "password": "hunter2",
        "query": "normal value"
    });
    let sanitized = sanitize_parameters(&params);
    let obj = sanitized.as_object().expect("should be object");
    assert_eq!(obj["api_key"], "[REDACTED]");
    assert_eq!(obj["password"], "[REDACTED]");
    assert_eq!(obj["url"], "https://acme.corp/api");
    assert_eq!(obj["query"], "normal value");
}

// --- 7. Context classification ---

#[test]
fn classify_grep_as_code() {
    assert_eq!(
        ContextCategory::classify("grep", "anything"),
        ContextCategory::Code
    );
}

#[test]
fn classify_web_search_as_research() {
    assert_eq!(
        ContextCategory::classify("web_search", "anything"),
        ContextCategory::Research
    );
}

#[test]
fn classify_exec_as_system() {
    assert_eq!(
        ContextCategory::classify("exec", "running command"),
        ContextCategory::System
    );
}

#[test]
fn classify_memory_search_as_memory() {
    assert_eq!(
        ContextCategory::classify("memory_search", "looking up facts"),
        ContextCategory::Memory
    );
}

#[test]
fn classify_send_message_as_communication() {
    assert_eq!(
        ContextCategory::classify("send_message", "notifying user"),
        ContextCategory::Communication
    );
}

#[test]
fn classify_unknown_tool_with_code_context() {
    assert_eq!(
        ContextCategory::classify("custom_tool", "editing code file"),
        ContextCategory::Code
    );
}

#[test]
fn classify_unknown_tool_unknown_context_as_other() {
    assert_eq!(
        ContextCategory::classify("custom_tool", "doing something"),
        ContextCategory::Other
    );
}

// --- 8. Sanitization: value truncation ---

#[test]
fn sanitize_truncates_long_values() {
    let long_value = "x".repeat(300);
    let params = serde_json::json!({"content": long_value});
    let sanitized = sanitize_parameters(&params);
    let content = sanitized["content"].as_str().expect("should be string");
    assert!(
        content.len() <= MAX_PARAM_VALUE_LEN + 5,
        "value should be truncated to ~200 chars"
    );
    assert!(content.ends_with("..."));
}

// --- 9. Context summary truncation ---

#[test]
fn truncate_context_summary_short_passthrough() {
    let short = "brief summary";
    assert_eq!(truncate_context_summary(short), short);
}

#[test]
fn truncate_context_summary_long_truncated() {
    let long = "a".repeat(200);
    let truncated = truncate_context_summary(&long);
    assert!(truncated.len() <= MAX_CONTEXT_SUMMARY_LEN + 5);
    assert!(truncated.ends_with("..."));
}

// --- 10. ToolOutcome serialization roundtrip ---

#[test]
fn tool_outcome_stored_string_roundtrip() {
    let outcomes = [
        ToolOutcome::Success,
        ToolOutcome::Failure {
            error: "timeout".to_owned(),
        },
        ToolOutcome::Partial {
            note: "partial data".to_owned(),
        },
    ];
    for outcome in &outcomes {
        let stored = outcome.as_stored_string();
        let back = ToolOutcome::from_stored_string(&stored);
        assert_eq!(outcome, &back);
    }
}

// --- 11. BehavioralPattern threshold check ---

#[test]
fn pattern_meets_thresholds_at_boundary() {
    let pattern = BehavioralPattern {
        pattern: String::new(),
        tool_name: "grep".to_owned(),
        context_type: "code".to_owned(),
        success_count: 5,
        total_count: 6,
        success_rate: 5.0 / 6.0,
        first_observed: ts("2026-03-01T00:00:00Z"),
        last_observed: ts("2026-03-06T00:00:00Z"),
    };
    assert!(
        pattern.meets_thresholds(),
        "5 successes with 83% rate should meet thresholds"
    );
}

#[test]
fn pattern_does_not_meet_thresholds_below() {
    let pattern = BehavioralPattern {
        pattern: String::new(),
        tool_name: "grep".to_owned(),
        context_type: "code".to_owned(),
        success_count: 4,
        total_count: 5,
        success_rate: 0.8,
        first_observed: ts("2026-03-01T00:00:00Z"),
        last_observed: ts("2026-03-05T00:00:00Z"),
    };
    assert!(
        !pattern.meets_thresholds(),
        "4 successes should not meet threshold (minimum is 5)"
    );
}

// --- 12. Multiple tools aggregate independently ---

#[test]
fn aggregation_separates_tools() {
    let mut observations = Vec::new();
    for i in 0..6 {
        observations.push(make_observation(
            "grep",
            "code search",
            ToolOutcome::Success,
            &format!("2026-03-{:02}T12:00:00Z", i + 1),
        ));
        observations.push(make_observation(
            "web_search",
            "research topic",
            ToolOutcome::Success,
            &format!("2026-03-{:02}T13:00:00Z", i + 1),
        ));
    }

    let patterns = aggregate_observations(&observations);
    assert_eq!(patterns.len(), 2, "should have separate patterns per tool");
    let names: Vec<_> = patterns.iter().map(|p| p.tool_name.as_str()).collect();
    assert!(names.contains(&"grep"));
    assert!(names.contains(&"web_search"));
}

// --- 13. Nested secret sanitization ---

#[test]
fn sanitize_handles_nested_objects() {
    let params = serde_json::json!({
        "config": {
            "api_key": "test-key-not-real",
            "name": "test"
        }
    });
    let sanitized = sanitize_parameters(&params);
    let config = sanitized["config"].as_object().expect("nested object");
    assert_eq!(config["api_key"], "[REDACTED]");
    assert_eq!(config["name"], "test");
}

// --- 14. Observation serde roundtrip ---

#[test]
fn observation_serde_roundtrip() {
    let obs = ToolObservation {
        tool_name: "read_file".to_owned(),
        parameters: serde_json::json!({"path": "/tmp/test.txt"}),
        outcome: ToolOutcome::Success,
        context_summary: "reading test file".to_owned(),
        nous_id: "nous-1".to_owned(),
        observed_at: ts("2026-03-01T12:00:00Z"),
    };
    let json = serde_json::to_string(&obs).expect("serialize");
    let back: ToolObservation = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(obs.tool_name, back.tool_name);
    assert_eq!(obs.nous_id, back.nous_id);
}

// --- 15. ContextCategory display/parse ---

#[test]
fn context_category_roundtrip() {
    let categories = [
        ContextCategory::Code,
        ContextCategory::Research,
        ContextCategory::System,
        ContextCategory::Memory,
        ContextCategory::Communication,
        ContextCategory::Other,
    ];
    for cat in categories {
        let s = cat.as_str();
        let back = ContextCategory::from_str_lossy(s);
        assert_eq!(cat, back, "roundtrip failed for {s}");
    }
}

// --- 16. Req 3: Different context categories aggregate separately ---

#[test]
fn observations_different_context_categories_aggregate_separately() {
    // "custom_tool" has no tool-name classification; context summary governs.
    let mut observations = Vec::new();
    for i in 0..5 {
        observations.push(ToolObservation {
            tool_name: "custom_tool".to_owned(),
            parameters: serde_json::json!({}),
            outcome: ToolOutcome::Success,
            context_summary: "searching code files".to_owned(),
            nous_id: "nous-alice".to_owned(),
            observed_at: ts(&format!("2026-03-{:02}T10:00:00Z", i + 1)),
        });
    }
    for i in 0..5 {
        observations.push(ToolObservation {
            tool_name: "custom_tool".to_owned(),
            parameters: serde_json::json!({}),
            outcome: ToolOutcome::Success,
            context_summary: "web search and research documentation".to_owned(),
            nous_id: "nous-alice".to_owned(),
            observed_at: ts(&format!("2026-03-{:02}T12:00:00Z", i + 1)),
        });
    }
    let patterns = aggregate_observations(&observations);
    let custom: Vec<_> = patterns
        .iter()
        .filter(|p| p.tool_name == "custom_tool")
        .collect();
    assert_eq!(
        custom.len(),
        2,
        "same tool in code vs research context should produce two separate patterns"
    );
    let ctx_types: std::collections::HashSet<_> =
        custom.iter().map(|p| p.context_type.as_str()).collect();
    assert!(
        ctx_types.contains("code"),
        "expected a code context pattern"
    );
    assert!(
        ctx_types.contains("research"),
        "expected a research context pattern"
    );
}

// --- 17. Req 4: Empty parameters does not panic ---

#[test]
fn empty_parameters_does_not_panic() {
    let params = serde_json::json!({});
    let sanitized = sanitize_parameters(&params);
    assert!(
        sanitized.as_object().expect("should be object").is_empty(),
        "sanitizing empty params returns empty object"
    );
}

// --- 18. Req 4: Aggregation with empty parameters does not panic ---

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

// --- 19. Req 6: 4 calls does NOT trigger pattern (below threshold of 5) ---

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

// --- 20. Req 5: 5 calls DOES trigger pattern (at threshold) ---

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
    assert_eq!(patterns[0].success_count, MIN_OBSERVATIONS);
}

// --- 21. Req 7: 10 calls with 70% success rate does NOT trigger (below 80%) ---

#[test]
fn ten_calls_below_80_percent_success_rate_does_not_trigger() {
    // 7 successes + 3 failures = 70%: below MIN_SUCCESS_RATE of 80%.
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

// --- 22. Req 8: 10 calls with exactly 80% success rate DOES trigger ---

#[test]
fn ten_calls_at_80_percent_success_rate_boundary_triggers_pattern() {
    // 8 successes + 2 failures = exactly 80%.
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
    assert_eq!(patterns[0].success_count, 8);
    assert_eq!(patterns[0].total_count, 10);
    assert!((patterns[0].success_rate - 0.80).abs() < 1e-10);
}

// --- 23. Req 9: Token field names are redacted ---

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

// --- 24. Req 10: Values at exactly MAX_PARAM_VALUE_LEN chars are NOT truncated ---

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

// --- 25. Req 10: Values at MAX_PARAM_VALUE_LEN + 1 chars ARE truncated ---

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

// --- 26. Req 12: Array parameter values are sanitized element-by-element ---

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
    assert!(first.len() <= MAX_PARAM_VALUE_LEN + 3);
    assert_eq!(items[1].as_str().expect("string"), "short item");
    assert_eq!(items[2].as_str().expect("string"), "another normal value");
}

// --- 27. Req 13: Mixed-nous observations aggregate together; per-nous filter gives independence ---

#[test]
fn different_nous_observations_aggregate_together_without_filter() {
    // aggregate_observations groups by (tool_name, context_category) only: not nous_id.
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
    // Combined 6 → meets threshold
    let all = aggregate_observations(&observations);
    assert_eq!(
        all.len(),
        1,
        "mixed-nous observations aggregate into one pattern"
    );
    assert_eq!(all[0].total_count, 6);
    // Alice's 3 alone → below threshold
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

// --- 28. Req 14: Different tools with identical parameters produce separate patterns ---

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
    assert!(tool_names.contains("grep"));
    assert!(tool_names.contains("read_file"));
}

// --- 29. Req 15: Tool name matching is case-sensitive ---

#[test]
fn tool_name_matching_is_case_sensitive() {
    // Both classify as Code context (classification lowercases the tool name),
    // but the pattern key uses the original tool_name. So "Grep" ≠ "grep".
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

// --- 30. Req 16: All context categories serialize/deserialize via JSON ---

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

// --- 31. Req 17: Context summary at exactly the limit is NOT truncated ---

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

// --- 32. Req 18: Unknown context category string falls back to Other ---

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
    // from_str_lossy is case-sensitive; "CODE" is not a recognized variant.
    assert_eq!(
        ContextCategory::from_str_lossy("CODE"),
        ContextCategory::Other,
        "uppercase variant should fall back to Other (case-sensitive)"
    );
}

// === Property-based tests ===

mod proptests {
    use super::super::{
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
                n as u32,
                "total_count should equal the number of observations"
            );
        }
    }
}
