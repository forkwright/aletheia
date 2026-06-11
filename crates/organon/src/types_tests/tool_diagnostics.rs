//! `ToolDiagnostics` and `ToolOutcome` tests.

#![expect(clippy::expect_used, reason = "test assertions")]

use super::super::*;

// ── ToolDiagnostics tests ────────────────────────────────────────────

#[test]
fn tool_diagnostics_to_llm_text_includes_all_fields() {
    let diag = ToolDiagnostics {
        exit_code: Some(127),
        stderr: Some("command not found".to_owned()),
        sandbox_violations: vec!["read /etc/shadow".to_owned()],
        duration_ms: 42,
    };
    let text = diag.to_llm_text();
    assert!(
        text.contains("exit_code=127"),
        "should include exit_code: {text}"
    );
    assert!(
        text.contains("stderr=command not found"),
        "should include stderr: {text}"
    );
    assert!(
        text.contains("sandbox_violations=read /etc/shadow"),
        "should include sandbox violations: {text}"
    );
    assert!(
        text.contains("duration_ms=42"),
        "should include duration: {text}"
    );
}

#[test]
fn tool_diagnostics_to_llm_text_omits_empty_fields() {
    let diag = ToolDiagnostics {
        exit_code: None,
        stderr: None,
        sandbox_violations: Vec::new(),
        duration_ms: 100,
    };
    let text = diag.to_llm_text();
    assert!(
        !text.contains("exit_code"),
        "should omit exit_code when None: {text}"
    );
    assert!(
        !text.contains("stderr"),
        "should omit stderr when None: {text}"
    );
    assert!(
        !text.contains("sandbox_violations"),
        "should omit sandbox_violations when empty: {text}"
    );
    assert!(
        text.contains("duration_ms=100"),
        "should always include duration: {text}"
    );
}

#[test]
fn tool_diagnostics_bounds_long_stderr() {
    let long_stderr = "e".repeat(600);
    let diag = ToolDiagnostics {
        exit_code: Some(1),
        stderr: Some(long_stderr.clone()),
        sandbox_violations: Vec::new(),
        duration_ms: 0,
    };
    let text = diag.to_llm_text();
    assert!(
        text.contains("stderr="),
        "should include stderr prefix: {text}"
    );
    assert!(
        text.contains("…[truncated]"),
        "should truncate long stderr: {text}"
    );
    assert!(
        text.len() < long_stderr.len() + 100,
        "diagnostic text should be bounded: {text}"
    );
}

#[test]
fn tool_result_with_diagnostics_builder() {
    let result = ToolResult::text("hello").with_diagnostics(ToolDiagnostics {
        exit_code: Some(0),
        stderr: None,
        sandbox_violations: Vec::new(),
        duration_ms: 5,
    });
    assert!(!result.is_error, "should not be error");
    assert_eq!(
        result
            .diagnostics
            .as_ref()
            .expect("diagnostics present")
            .exit_code,
        Some(0),
        "should carry diagnostics"
    );
}

#[test]
fn tool_result_serde_roundtrip_preserves_diagnostics() {
    let result = ToolResult::error("fail").with_diagnostics(ToolDiagnostics {
        exit_code: Some(1),
        stderr: Some("stderr output".to_owned()),
        sandbox_violations: vec!["violation".to_owned()],
        duration_ms: 10,
    });
    let json = serde_json::to_string(&result).expect("serialize");
    let back: ToolResult = serde_json::from_str(&json).expect("deserialize");
    assert!(back.is_error, "should preserve is_error");
    let diag = back.diagnostics.expect("should preserve diagnostics");
    assert_eq!(diag.exit_code, Some(1));
    assert_eq!(diag.stderr.as_deref(), Some("stderr output"));
    assert_eq!(diag.sandbox_violations, vec!["violation".to_owned()]);
    assert_eq!(diag.duration_ms, 10);
}

#[test]
fn tool_result_serde_backward_compat_missing_diagnostics() {
    // WHY: ToolResultContent is an untagged enum; Text(String) serializes as
    // a plain JSON string, not an object.
    let json = r#"{"content":"legacy","is_error":false}"#;
    let back: ToolResult = serde_json::from_str(json).expect("deserialize legacy");
    assert_eq!(back.content.text_summary(), "legacy");
    assert!(!back.is_error);
    assert!(
        back.diagnostics.is_none(),
        "missing diagnostics should default to None"
    );
}

// ── ToolOutcome / partial success ──

#[test]
fn tool_result_partial_success_constructor_carries_reasons() {
    // WHY: a 3-way sub-agent dispatch where 2 succeed and 1 fails must surface
    // as PartialSuccess with one reason per failed sub-op — a bare is_error
    // bool would collapse it into a binary.
    let r = ToolResult::partial_success(
        r#"[{"ok":1},{"ok":2},{"err":"boom"}]"#,
        vec!["sub-agent #3: boom".to_owned()],
    );
    assert!(
        !r.is_error,
        "partial_success should map to is_error=false — tool delivered usable output"
    );
    assert!(r.outcome.is_partial(), "outcome should be PartialSuccess");
    assert_eq!(
        r.outcome.partial_reasons(),
        &["sub-agent #3: boom".to_owned()],
        "reasons should propagate through the outcome, not be collapsed"
    );

    // NOTE: consumers still reading is_error get the back-compat view.
    match &r.outcome {
        ToolOutcome::PartialSuccess(info) => {
            assert_eq!(info.reasons.len(), 1, "one reason per failed sub-operation");
        }
        other => panic!("expected PartialSuccess, got {other:?}"),
    }
}

#[test]
fn tool_outcome_is_error_collapse_matches_legacy_bool() {
    assert!(!ToolOutcome::Success.is_error());
    assert!(!ToolOutcome::partial(Vec::<String>::new()).is_error());
    assert!(ToolOutcome::failure("x").is_error());
}

#[test]
fn tool_result_outcome_serde_roundtrip_partial() {
    let r = ToolResult::partial_success("payload", vec!["warn".to_owned()]);
    let json = serde_json::to_string(&r).expect("serialize");
    let back: ToolResult = serde_json::from_str(&json).expect("deserialize");
    assert!(back.outcome.is_partial(), "outcome should survive serde");
    assert_eq!(back.outcome.partial_reasons(), &["warn".to_owned()]);
    assert!(!back.is_error);
}

#[test]
fn tool_result_serde_legacy_payload_defaults_to_success_then_normalizes() {
    // Legacy payloads predate the `outcome` field; #[serde(default)]
    // populates it as Success. For legacy error records, normalize()
    // promotes the outcome to Failure to match is_error=true.
    let success_legacy = r#"{"content":"ok","is_error":false}"#;
    let back: ToolResult = serde_json::from_str(success_legacy).expect("deserialize success");
    assert!(
        back.outcome.is_success(),
        "legacy success defaults correctly"
    );

    let error_legacy = r#"{"content":"bad","is_error":true}"#;
    let back: ToolResult = serde_json::from_str::<ToolResult>(error_legacy)
        .expect("deserialize error")
        .normalize();
    assert!(
        back.outcome.is_error(),
        "normalize() promotes legacy is_error=true records to Failure"
    );
    assert!(back.is_error);
}

#[test]
fn tool_stats_record_outcome_separates_partial_from_error() {
    let mut stats = ToolStats::default();
    stats.record_outcome("dispatch", 10, &ToolOutcome::Success);
    stats.record_outcome(
        "dispatch",
        20,
        &ToolOutcome::partial(vec!["one failed".to_owned()]),
    );
    stats.record_outcome("dispatch", 30, &ToolOutcome::failure("crash"));
    assert_eq!(stats.total_calls, 3);
    assert_eq!(
        stats.error_count, 1,
        "only true failures count as errors, not partials"
    );
    assert_eq!(
        stats.partial_count, 1,
        "partial successes get their own counter (#3633)"
    );
    assert_eq!(stats.total_duration_ms, 60);
}
