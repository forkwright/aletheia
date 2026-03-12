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
