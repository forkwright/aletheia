#![expect(clippy::unwrap_used, reason = "test assertions")]

use super::*;

#[test]
fn ci_status_all_green() {
    let checks = vec![
        CheckRun {
            name: "build".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
        },
        CheckRun {
            name: "test".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
        },
    ];
    assert_eq!(determine_ci_status(&checks, &[]), CiStatus::Green);
}

#[test]
fn ci_status_one_failure() {
    let checks = vec![
        CheckRun {
            name: "build".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
        },
        CheckRun {
            name: "test".to_string(),
            status: "completed".to_string(),
            conclusion: Some("failure".to_string()),
        },
    ];
    assert_eq!(determine_ci_status(&checks, &[]), CiStatus::Red);
}

#[test]
fn ci_status_pending() {
    let checks = vec![
        CheckRun {
            name: "build".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
        },
        CheckRun {
            name: "test".to_string(),
            status: "in_progress".to_string(),
            conclusion: None,
        },
    ];
    assert_eq!(determine_ci_status(&checks, &[]), CiStatus::Pending);
}

#[test]
fn ci_status_empty_is_unknown() {
    assert_eq!(determine_ci_status(&[], &[]), CiStatus::Unknown);
}

#[test]
fn ci_status_skipped_counts_as_green() {
    let checks = vec![CheckRun {
        name: "optional".to_string(),
        status: "completed".to_string(),
        conclusion: Some("skipped".to_string()),
    }];
    assert_eq!(determine_ci_status(&checks, &[]), CiStatus::Green);
}

#[test]
fn ci_status_neutral_counts_as_green() {
    let checks = vec![CheckRun {
        name: "lint".to_string(),
        status: "completed".to_string(),
        conclusion: Some("neutral".to_string()),
    }];
    assert_eq!(determine_ci_status(&checks, &[]), CiStatus::Green);
}

#[test]
fn required_checks_ignores_non_required_failure() {
    // WHY: CodeQL can fail due to permissions without indicating code issues.
    // When required_checks is set, only those checks affect the status.
    let checks = vec![
        CheckRun {
            name: "verify-gate".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
        },
        CheckRun {
            name: "CodeQL".to_string(),
            status: "completed".to_string(),
            conclusion: Some("failure".to_string()),
        },
    ];
    let required = vec!["verify-gate".to_string()];
    assert_eq!(determine_ci_status(&checks, &required), CiStatus::Green);
}

#[test]
fn required_checks_catches_required_failure() {
    let checks = vec![
        CheckRun {
            name: "verify-gate".to_string(),
            status: "completed".to_string(),
            conclusion: Some("failure".to_string()),
        },
        CheckRun {
            name: "CodeQL".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
        },
    ];
    let required = vec!["verify-gate".to_string()];
    assert_eq!(determine_ci_status(&checks, &required), CiStatus::Red);
}

#[test]
fn empty_required_checks_considers_all() {
    // WHY: Legacy behavior -- empty list means all checks matter.
    let checks = vec![
        CheckRun {
            name: "verify-gate".to_string(),
            status: "completed".to_string(),
            conclusion: Some("success".to_string()),
        },
        CheckRun {
            name: "CodeQL".to_string(),
            status: "completed".to_string(),
            conclusion: Some("failure".to_string()),
        },
    ];
    assert_eq!(determine_ci_status(&checks, &[]), CiStatus::Red);
}

#[test]
fn extract_prompt_number_from_title_k_prefix() {
    let pr = PullRequest {
        number: 1,
        title: "feat(steward): K-014 steward pipeline".to_string(),
        head_ref_name: None,
        head_sha: None,
        state: None,
        mergeable: None,
        body: None,
        updated_at: None,
        merged_at: None,
    };
    assert_eq!(extract_prompt_number(&pr), Some(14));
}

#[test]
fn extract_prompt_number_from_branch() {
    let pr = PullRequest {
        number: 1,
        title: "some feature".to_string(),
        head_sha: None,
        head_ref_name: Some("feat/k-14-steward-merge".to_string()),
        state: None,
        mergeable: None,
        body: None,
        updated_at: None,
        merged_at: None,
    };
    assert_eq!(extract_prompt_number(&pr), Some(14));
}

#[test]
fn extract_prompt_number_none_when_missing() {
    let pr = PullRequest {
        number: 1,
        title: "random PR with no refs".to_string(),
        head_sha: None,
        head_ref_name: Some("feat/random-feature".to_string()),
        state: None,
        mergeable: None,
        body: None,
        updated_at: None,
        merged_at: None,
    };
    assert_eq!(extract_prompt_number(&pr), None);
}

#[test]
fn extract_prompt_number_from_text_variants() {
    assert_eq!(extract_prompt_number_from_text("K-042 foo"), Some(42));
    assert_eq!(extract_prompt_number_from_text("k-42 bar"), Some(42));
    assert_eq!(extract_prompt_number_from_text("K042 baz"), Some(42));
    assert_eq!(extract_prompt_number_from_text("prompt 7"), Some(7));
    assert_eq!(extract_prompt_number_from_text("no number here"), None);
}

// ── Gate trailer CI override tests ──

#[test]
fn gate_trailer_overrides_unknown() {
    // WHY: When no CI checks exist (minutes exhausted), the gate trailer
    // proves local gate passed and is sufficient to merge.
    assert_eq!(
        apply_gate_trailer_override(CiStatus::Unknown, true, 303),
        CiStatus::Green,
        "gate trailer must upgrade Unknown to Green"
    );
}

#[test]
fn gate_trailer_overrides_pending() {
    // WHY: When CI is still queued/running but the gate trailer proves
    // local gate passed, the steward must not wait.
    assert_eq!(
        apply_gate_trailer_override(CiStatus::Pending, true, 303),
        CiStatus::Green,
        "gate trailer must upgrade Pending to Green"
    );
}

#[test]
fn gate_trailer_overrides_red() {
    // WHY: If the local gate passed, a red CI check is informational
    // (e.g. stale check, flaky) and must not block the steward from merging.
    assert_eq!(
        apply_gate_trailer_override(CiStatus::Red, true, 303),
        CiStatus::Green,
        "gate trailer must upgrade Red to Green"
    );
}

#[test]
fn gate_trailer_noop_when_already_green() {
    // WHY: When CI is already green, the trailer changes nothing.
    assert_eq!(
        apply_gate_trailer_override(CiStatus::Green, true, 303),
        CiStatus::Green,
        "gate trailer must not change already-Green status"
    );
}

#[test]
fn no_trailer_preserves_ci_status() {
    // WHY: Without a gate trailer, no override should occur.
    assert_eq!(
        apply_gate_trailer_override(CiStatus::Unknown, false, 303),
        CiStatus::Unknown,
    );
    assert_eq!(
        apply_gate_trailer_override(CiStatus::Pending, false, 303),
        CiStatus::Pending,
    );
    assert_eq!(
        apply_gate_trailer_override(CiStatus::Red, false, 303),
        CiStatus::Red,
    );
    assert_eq!(
        apply_gate_trailer_override(CiStatus::Green, false, 303),
        CiStatus::Green,
    );
}

// ── Structural suppression detection tests ──

#[test]
fn suppression_detects_allow_attribute() {
    let diff = concat!(
        "+++ b/src/lib.rs\n",
        "@@ -1 +1,2 @@\n",
        "+#[allow(dead_code)]\n",
        " fn foo() {}\n"
    );
    let findings = parse_suppressions(diff);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings.first().cloned().unwrap().kind,
        SuppressionKind::Allow
    );
    assert_eq!(
        findings.first().cloned().unwrap().lint_name.as_deref(),
        Some("dead_code")
    );
    assert_eq!(findings.first().cloned().unwrap().file, "src/lib.rs");
}

#[test]
fn suppression_detects_expect_without_reason() {
    let diff = concat!(
        "+++ b/src/lib.rs\n",
        "@@ -1 +1,2 @@\n",
        "+#[expect(clippy::unwrap_used)]\n",
        " fn foo() {}\n"
    );
    let findings = parse_suppressions(diff);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings.first().cloned().unwrap().kind,
        SuppressionKind::ExpectNoReason
    );
    assert_eq!(
        findings.first().cloned().unwrap().lint_name.as_deref(),
        Some("clippy::unwrap_used")
    );
}

#[test]
fn suppression_allows_expect_with_reason() {
    let diff = concat!(
        "+++ b/src/lib.rs\n",
        "@@ -1 +1,2 @@\n",
        "+#[expect(clippy::unwrap_used, reason = \"test helper\")]\n",
        " fn foo() {}\n"
    );
    let findings = parse_suppressions(diff);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings.first().cloned().unwrap().kind,
        SuppressionKind::ExpectWithReason
    );
    assert_eq!(
        findings.first().cloned().unwrap().lint_name.as_deref(),
        Some("clippy::unwrap_used")
    );
    assert_eq!(
        findings.first().cloned().unwrap().reason.as_deref(),
        Some("test helper")
    );
}

#[test]
fn suppression_skips_test_files() {
    let diff = concat!(
        "+++ b/src/tests/foo_test.rs\n",
        "@@ -1 +1,2 @@\n",
        "+#[allow(dead_code)]\n",
        " fn test_foo() {}\n"
    );
    let findings = parse_suppressions(diff);
    assert!(findings.is_empty());
}

#[test]
fn suppression_skips_test_files_suffix() {
    let diff = concat!(
        "+++ b/tests.rs\n",
        "@@ -1 +1,2 @@\n",
        "+#[allow(dead_code)]\n",
        " fn test_all() {}\n"
    );
    let findings = parse_suppressions(diff);
    assert!(findings.is_empty());
}

#[test]
fn suppression_detects_multiple_in_diff() {
    let diff = concat!(
        "+++ b/src/lib.rs\n",
        "@@ -1,3 +1,5 @@\n",
        "+#[allow(dead_code)]\n",
        " fn foo() {}\n",
        "+\n",
        "+#[expect(unused_imports, reason = \"used in tests\")]\n",
        " fn bar() {}\n"
    );
    let findings = parse_suppressions(diff);
    assert_eq!(findings.len(), 2);
    assert_eq!(
        findings.first().cloned().unwrap().kind,
        SuppressionKind::Allow
    );
    assert_eq!(
        findings.get(1).cloned().unwrap().kind,
        SuppressionKind::ExpectWithReason
    );
}

#[test]
fn suppression_empty_diff_returns_empty() {
    let findings = parse_suppressions("");
    assert!(findings.is_empty());
}

#[test]
fn suppression_no_added_lines_returns_empty() {
    let diff = "+++ b/src/lib.rs\n@@ -1,2 +1,2 @@\n-removed line\n context line\n";
    let findings = parse_suppressions(diff);
    assert!(findings.is_empty());
}

#[test]
fn suppression_handles_single_quotes_in_reason() {
    let diff = concat!(
        "+++ b/src/lib.rs\n",
        "@@ -1 +1,2 @@\n",
        "+#[expect(unused, reason = 'single quoted')]\n",
        " fn foo() {}\n"
    );
    let findings = parse_suppressions(diff);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings.first().cloned().unwrap().kind,
        SuppressionKind::ExpectWithReason
    );
    assert_eq!(
        findings.first().cloned().unwrap().reason.as_deref(),
        Some("single quoted")
    );
}

// ── cfg_attr detection tests ──

#[test]
fn suppression_detects_cfg_attr_allow() {
    let diff = concat!(
        "+++ b/src/lib.rs\n",
        "@@ -1 +1,2 @@\n",
        "+#[cfg_attr(not(test), allow(dead_code))]\n",
        " fn foo() {}\n"
    );
    let findings = parse_suppressions(diff);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings.first().cloned().unwrap().kind,
        SuppressionKind::CfgAttrAllow
    );
    assert_eq!(
        findings.first().cloned().unwrap().lint_name.as_deref(),
        Some("dead_code")
    );
    assert_eq!(findings.first().cloned().unwrap().file, "src/lib.rs");
}

#[test]
fn suppression_detects_cfg_attr_clippy() {
    let diff = concat!(
        "+++ b/src/lib.rs\n",
        "@@ -1 +1,2 @@\n",
        "+#[cfg_attr(feature = \"nightly\", allow(clippy::unwrap_used))]\n",
        " fn foo() {}\n"
    );
    let findings = parse_suppressions(diff);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings.first().cloned().unwrap().kind,
        SuppressionKind::CfgAttrAllow
    );
    assert_eq!(
        findings.first().cloned().unwrap().lint_name.as_deref(),
        Some("clippy::unwrap_used")
    );
}

// ── lint-ignore file detection tests ──

#[test]
fn suppression_detects_lint_ignore_file_addition() {
    let diff = concat!(
        "+++ b/.lint-ignore\n",
        "@@ -0,0 +1,2 @@\n",
        "+RULE/naming:src/legacy.rs\n",
        "+RULE/docs:crates/old/\n"
    );
    let findings = parse_suppressions(diff);
    assert_eq!(findings.len(), 2);
    assert_eq!(
        findings.first().cloned().unwrap().kind,
        SuppressionKind::LintIgnoreFile
    );
    assert_eq!(
        findings.first().cloned().unwrap().lint_name.as_deref(),
        Some("RULE/naming:src/legacy.rs")
    );
    assert_eq!(
        findings.get(1).cloned().unwrap().kind,
        SuppressionKind::LintIgnoreFile
    );
}

#[test]
fn suppression_ignores_comments_in_lint_ignore() {
    let diff = concat!(
        "+++ b/.lint-ignore\n",
        "@@ -0,0 +1,3 @@\n",
        "+# This is a comment\n",
        "+\n",
        "+RULE/naming:src/legacy.rs\n"
    );
    let findings = parse_suppressions(diff);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings.first().cloned().unwrap().kind,
        SuppressionKind::LintIgnoreFile
    );
}

// ── lint-ignore inline comment detection tests ──

#[test]
fn suppression_detects_lint_ignore_inline() {
    let diff = concat!(
        "+++ b/src/lib.rs\n",
        "@@ -1 +1,2 @@\n",
        "+fn foo() {} // lint-ignore\n",
        " fn bar() {}\n"
    );
    let findings = parse_suppressions(diff);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings.first().cloned().unwrap().kind,
        SuppressionKind::LintIgnoreInline
    );
    assert_eq!(findings.first().cloned().unwrap().file, "src/lib.rs");
}

// ── SAFETY/INVARIANT comment bypass detection tests ──

#[test]
fn suppression_detects_safety_comment() {
    let diff = concat!(
        "+++ b/src/lib.rs\n",
        "@@ -1 +1,3 @@\n",
        "+// SAFETY: this is safe because reasons\n",
        "+unsafe { ptr.read() }\n",
        " fn foo() {}\n"
    );
    let findings = parse_suppressions(diff);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings.first().cloned().unwrap().kind,
        SuppressionKind::StructuredCommentBypass
    );
    assert_eq!(
        findings.first().cloned().unwrap().lint_name.as_deref(),
        Some("SAFETY")
    );
}

#[test]
fn suppression_detects_invariant_comment() {
    let diff = concat!(
        "+++ b/src/lib.rs\n",
        "@@ -1 +1,2 @@\n",
        "+// INVARIANT: value is always positive\n",
        " fn foo() {}\n"
    );
    let findings = parse_suppressions(diff);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings.first().cloned().unwrap().kind,
        SuppressionKind::StructuredCommentBypass
    );
    assert_eq!(
        findings.first().cloned().unwrap().lint_name.as_deref(),
        Some("INVARIANT")
    );
}

#[test]
fn suppression_skips_safety_in_test_files() {
    let diff = concat!(
        "+++ b/src/tests/safety_test.rs\n",
        "@@ -1 +1,2 @@\n",
        "+// SAFETY: test helper\n",
        " fn test_unsafe() {}\n"
    );
    let findings = parse_suppressions(diff);
    assert!(findings.is_empty());
}

// ── Combined pattern detection tests ──

#[test]
fn suppression_detects_all_patterns_in_single_diff() {
    let diff = concat!(
        "+++ b/src/lib.rs\n",
        "@@ -1,3 +1,8 @@\n",
        "+#[allow(dead_code)]\n",
        "+#[cfg_attr(not(test), allow(unused))]\n",
        "+// SAFETY: trust me\n",
        "+fn bad() {} // lint-ignore\n",
        " fn good() {}\n",
        "+++ b/.lint-ignore\n",
        "@@ -0,0 +1 @@\n",
        "+RULE/naming:src/lib.rs\n"
    );
    let findings = parse_suppressions(diff);
    // NOTE: 5 findings: allow, cfg_attr, safety, lint-inline, lint-file
    assert_eq!(findings.len(), 5);
    assert_eq!(
        findings.first().cloned().unwrap().kind,
        SuppressionKind::Allow
    );
    assert_eq!(
        findings.get(1).cloned().unwrap().kind,
        SuppressionKind::CfgAttrAllow
    );
    assert_eq!(
        findings.get(2).cloned().unwrap().kind,
        SuppressionKind::StructuredCommentBypass
    );
    assert_eq!(
        findings.get(3).cloned().unwrap().kind,
        SuppressionKind::LintIgnoreInline
    );
    assert_eq!(
        findings.get(4).cloned().unwrap().kind,
        SuppressionKind::LintIgnoreFile
    );
}

// ── QA verdict extraction tests ──

#[test]
fn extract_qa_verdict_machine_readable_pass() {
    let body = "Some description\n<!-- qa-verdict: PASS -->\nMore text";
    assert_eq!(
        extract_qa_verdict_from_body(Some(body)),
        Some(QaVerdictStatus::Pass)
    );
}

#[test]
fn extract_qa_verdict_machine_readable_partial() {
    let body = "<!-- qa-verdict: PARTIAL -->";
    assert_eq!(
        extract_qa_verdict_from_body(Some(body)),
        Some(QaVerdictStatus::Partial)
    );
}

#[test]
fn extract_qa_verdict_machine_readable_fail() {
    let body = "<!-- qa-verdict: FAIL -->";
    assert_eq!(
        extract_qa_verdict_from_body(Some(body)),
        Some(QaVerdictStatus::Fail)
    );
}

#[test]
fn extract_qa_verdict_machine_readable_case_insensitive() {
    let body = "<!-- qa-verdict: pass -->";
    assert_eq!(
        extract_qa_verdict_from_body(Some(body)),
        Some(QaVerdictStatus::Pass)
    );
}

#[test]
fn extract_qa_verdict_human_readable_fallback() {
    let body = "**QA Verdict:** PASS\n\nSome criteria met.";
    assert_eq!(
        extract_qa_verdict_from_body(Some(body)),
        Some(QaVerdictStatus::Pass)
    );
}

#[test]
fn extract_qa_verdict_none_when_missing() {
    let body = "Normal PR description without QA info";
    assert_eq!(extract_qa_verdict_from_body(Some(body)), None);
}

#[test]
fn extract_qa_verdict_none_when_body_is_none() {
    assert_eq!(extract_qa_verdict_from_body(None), None);
}

// ── Hunk header parsing tests ──

#[test]
fn parse_hunk_new_start_normal() {
    assert_eq!(parse_hunk_new_start("@@ -1,3 +1,5 @@"), Some(1));
    assert_eq!(parse_hunk_new_start("@@ -10,2 +42,7 @@"), Some(42));
}

#[test]
fn parse_hunk_new_start_single_line() {
    assert_eq!(parse_hunk_new_start("@@ -1 +1,2 @@"), Some(1));
}

#[test]
fn parse_hunk_new_start_invalid() {
    assert_eq!(parse_hunk_new_start("not a hunk header"), None);
}
