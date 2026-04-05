//! PR classification by CI status, blast radius, and anti-pattern detection.
//!
//! Contains the pure classification functions for determining CI status,
//! extracting prompt numbers, parsing suppressions, and extracting QA verdicts.
//! Async functions that depend on GitHub API are not included here -- they
//! will be wired when a backend trait is implemented.

use std::sync::LazyLock;

use regex::Regex;

use super::types::{
    CheckRun, CiStatus, PullRequest, QaVerdictStatus, SuppressionFinding, SuppressionKind,
};

// WHY: Pre-compile regex patterns once via LazyLock for performance.
// Using LazyLock instead of lazy_static per project convention.
// INVARIANT: All patterns are compile-time constant strings, so Regex::new
// will never fail. The expect() calls are safe.
#[expect(clippy::expect_used, reason = "compile-time constant regex patterns cannot fail")]
static ALLOW_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#\[allow\s*\(([^)]*)\)\s*\]").expect("valid regex"));

#[expect(clippy::expect_used, reason = "compile-time constant regex patterns cannot fail")]
static EXPECT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#\[expect\s*\(([^)]*)\)\s*\]").expect("valid regex"));

#[expect(clippy::expect_used, reason = "compile-time constant regex patterns cannot fail")]
static CFG_ATTR_ALLOW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"#\[cfg_attr\s*\([^,]*,\s*allow\s*\(([^)]*)\)\s*\)\s*\]").expect("valid regex")
});

#[expect(clippy::expect_used, reason = "compile-time constant regex patterns cannot fail")]
static LINT_IGNORE_INLINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"//\s*lint-ignore").expect("valid regex"));

#[expect(clippy::expect_used, reason = "compile-time constant regex patterns cannot fail")]
static STRUCTURED_COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"//\s*(SAFETY|INVARIANT)\s*:").expect("valid regex"));

#[expect(clippy::expect_used, reason = "compile-time constant regex patterns cannot fail")]
static REASON_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"reason\s*=\s*["']([^"']+)["']"#).expect("valid regex"));

/// Determine aggregate CI status from individual check runs.
///
/// When `required_checks` is non-empty, only checks whose name appears in
/// the list affect the pass/fail decision. Failures on non-required checks
/// are logged as warnings but don't block merges.
///
/// When `required_checks` is empty, ALL checks are considered (legacy behavior).
#[must_use]
pub fn determine_ci_status(checks: &[CheckRun], required_checks: &[String]) -> CiStatus {
    if checks.is_empty() {
        return CiStatus::Unknown;
    }

    let filter_enabled = !required_checks.is_empty();

    let mut has_failure = false;
    let mut has_pending = false;

    for check in checks {
        let is_required = !filter_enabled
            || required_checks
                .iter()
                .any(|r| check.name.contains(r.as_str()));

        match check.status.as_str() {
            "completed" | "COMPLETED" => {
                if let Some(ref conclusion) = check.conclusion {
                    match conclusion.as_str() {
                        "success" | "SUCCESS" | "neutral" | "NEUTRAL" | "skipped" | "SKIPPED" => {}
                        _ => {
                            if is_required {
                                has_failure = true;
                            } else {
                                // NOTE: Non-required check failed -- log but don't block.
                                tracing::debug!(
                                    check = %check.name,
                                    conclusion = %conclusion,
                                    "non-required check failed, ignoring for merge decision"
                                );
                            }
                        }
                    }
                }
            }
            // NOTE: All non-completed statuses (including unknown ones) are
            // treated as pending. This is intentional -- unknown status values
            // from future API versions should block rather than pass.
            _ => {
                if is_required {
                    has_pending = true;
                }
            }
        }
    }

    if has_failure {
        CiStatus::Red
    } else if has_pending {
        CiStatus::Pending
    } else {
        CiStatus::Green
    }
}

/// Extract a prompt number from a PR title or branch name.
///
/// Looks for patterns like `K-042`, `K042`, `prompt 42`
/// in the title, then falls back to the branch name.
#[must_use]
pub fn extract_prompt_number(pr: &PullRequest) -> Option<u32> {
    // NOTE: Try title first, then branch name.
    let title_match = extract_prompt_number_from_text(&pr.title);
    if title_match.is_some() {
        return title_match;
    }

    if let Some(ref branch) = pr.head_ref_name {
        return extract_prompt_number_from_text(branch);
    }

    None
}

/// Extract a prompt number from free-form text.
///
/// Recognized patterns: `K-NNN`, `KNNN`, `prompt NNN`
#[must_use]
pub(crate) fn extract_prompt_number_from_text(text: &str) -> Option<u32> {
    // WHY: Use a simple regex to match common prompt number patterns.
    // The patterns are tried in order of specificity.
    let patterns = [
        r"[Kk]-?(\d{1,4})",      // K-042, K042, k-42
        r"prompt[\s-](\d{1,4})", // prompt 42, prompt-42
    ];

    for pattern in &patterns {
        // NOTE: Pattern compilation is cheap for these small patterns,
        // and they're only used in classification (not hot path).
        if let Ok(re) = Regex::new(pattern)
            && let Some(caps) = re.captures(text)
            && let Some(num_str) = caps.get(1)
            && let Ok(num) = num_str.as_str().parse::<u32>()
            && num > 0
        {
            return Some(num);
        }
    }

    None
}

/// Parse a diff for suppression attributes and structural bypass patterns.
///
/// WHY: Uses regex to detect `#[allow(...)]`, `#[expect(...)]`,
/// `#[cfg_attr(..., allow(...))]`, lint-ignore file additions,
/// `// lint-ignore` inline comments, and `// SAFETY:` / `// INVARIANT:`
/// comments added to bypass skip patterns.
#[expect(clippy::too_many_lines, reason = "suppression detection branches are individually simple; splitting would obscure the diff-walking state machine")]
#[must_use]
pub fn parse_suppressions(diff: &str) -> Vec<SuppressionFinding> {
    let mut findings = Vec::new();
    let mut current_file = String::new();
    let mut line_number: u32 = 0;

    for line in diff.lines() {
        // NOTE: Track which file we're in.
        if let Some(path) = line.strip_prefix("+++ b/") {
            current_file = path.trim().to_string();
            line_number = 0;
            continue;
        }

        // NOTE: Track line numbers from @@ hunk headers.
        if line.starts_with("@@ ") {
            if let Some(new_start) = parse_hunk_new_start(line) {
                line_number = new_start;
            }
            continue;
        }

        // NOTE: Only check added lines (starting with `+` but not `+++`).
        if let Some(added) = line.strip_prefix('+') {
            if !line.starts_with("+++") {
                // NOTE: Additions to lint-ignore files are always flagged
                // regardless of test file status.
                let is_lint_ignore = current_file.ends_with(".lint-ignore")
                    || current_file.ends_with("-lint-ignore");
                if is_lint_ignore {
                    let trimmed = added.trim();
                    // WHY: Skip blank lines and comments in the ignore file.
                    if !trimmed.is_empty() && !trimmed.starts_with('#') {
                        findings.push(SuppressionFinding {
                            file: current_file.clone(),
                            line: line_number,
                            kind: SuppressionKind::LintIgnoreFile,
                            lint_name: Some(trimmed.to_string()),
                            reason: None,
                        });
                    }
                }

                // NOTE: Skip test files -- suppressions are acceptable in tests.
                let is_test_file = current_file.contains("/tests/")
                    || current_file.contains("_test.rs")
                    || current_file.ends_with("tests.rs");

                if !is_test_file && !is_lint_ignore {
                    // Check for #[allow(...)]
                    if let Some(caps) = ALLOW_RE.captures(added) {
                        let lint_name = caps.get(1).map(|m| m.as_str().trim().to_string());
                        findings.push(SuppressionFinding {
                            file: current_file.clone(),
                            line: line_number,
                            kind: SuppressionKind::Allow,
                            lint_name,
                            reason: None,
                        });
                    }

                    // Check for #[cfg_attr(..., allow(...))]
                    if let Some(caps) = CFG_ATTR_ALLOW_RE.captures(added) {
                        let lint_name = caps.get(1).map(|m| m.as_str().trim().to_string());
                        findings.push(SuppressionFinding {
                            file: current_file.clone(),
                            line: line_number,
                            kind: SuppressionKind::CfgAttrAllow,
                            lint_name,
                            reason: None,
                        });
                    }

                    // Check for #[expect(...)]
                    if let Some(caps) = EXPECT_RE.captures(added) {
                        let content = caps.get(1).map_or("", |m| m.as_str());
                        let lint_name = content.split(',').next().map(str::trim).map(String::from);

                        let reason = REASON_RE
                            .captures(content)
                            .and_then(|r| r.get(1))
                            .map(|m| m.as_str().to_string());

                        let kind = if reason.is_some() {
                            SuppressionKind::ExpectWithReason
                        } else {
                            SuppressionKind::ExpectNoReason
                        };

                        findings.push(SuppressionFinding {
                            file: current_file.clone(),
                            line: line_number,
                            kind,
                            lint_name,
                            reason,
                        });
                    }

                    // Check for // lint-ignore inline comment
                    if LINT_IGNORE_INLINE_RE.is_match(added) {
                        findings.push(SuppressionFinding {
                            file: current_file.clone(),
                            line: line_number,
                            kind: SuppressionKind::LintIgnoreInline,
                            lint_name: None,
                            reason: None,
                        });
                    }

                    // Check for // SAFETY: or // INVARIANT: comments
                    // WHY: LLM workers add these to bypass skip patterns.
                    if let Some(caps) = STRUCTURED_COMMENT_RE.captures(added) {
                        let tag = caps.get(1).map(|m| m.as_str().to_string());
                        findings.push(SuppressionFinding {
                            file: current_file.clone(),
                            line: line_number,
                            kind: SuppressionKind::StructuredCommentBypass,
                            lint_name: tag,
                            reason: None,
                        });
                    }
                }
            }

            // NOTE: Added lines advance the line counter.
            #[expect(clippy::arithmetic_side_effects, reason = "line numbers fit in u32")]
            {
                line_number += 1;
            }
        } else if !line.starts_with('-') {
            // NOTE: Context lines (no prefix) also advance the line counter.
            #[expect(clippy::arithmetic_side_effects, reason = "line numbers fit in u32")]
            {
                line_number += 1;
            }
        }
    }

    findings
}

/// Extract QA verdict from the PR body.
///
/// WHY: Tiered merge policy requires the QA verdict at merge-decision
/// time. The dispatch pipeline writes a machine-readable marker into the PR
/// body: `<!-- qa-verdict: PASS -->`, `<!-- qa-verdict: PARTIAL -->`, or
/// `<!-- qa-verdict: FAIL -->`. This function parses that marker.
///
/// Falls back to scanning for human-readable patterns like `**QA Verdict:** PASS`.
#[must_use]
pub fn extract_qa_verdict_from_body(body: Option<&str>) -> Option<QaVerdictStatus> {
    let body = body?;

    // NOTE: Check for machine-readable marker first.
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(inner) = trimmed
            .strip_prefix("<!-- qa-verdict:")
            .and_then(|s| s.strip_suffix("-->"))
        {
            return match inner.trim().to_uppercase().as_str() {
                "PASS" => Some(QaVerdictStatus::Pass),
                "PARTIAL" => Some(QaVerdictStatus::Partial),
                "FAIL" => Some(QaVerdictStatus::Fail),
                _ => None,
            };
        }
    }

    // NOTE: Fall back to human-readable patterns.
    for line in body.lines() {
        let upper = line.to_uppercase();
        if upper.contains("QA VERDICT") || upper.contains("QA:") {
            if upper.contains("PASS") && !upper.contains("PARTIAL") {
                return Some(QaVerdictStatus::Pass);
            }
            if upper.contains("PARTIAL") {
                return Some(QaVerdictStatus::Partial);
            }
            if upper.contains("FAIL") {
                return Some(QaVerdictStatus::Fail);
            }
        }
    }

    None
}

/// Apply the gate trailer override to CI status.
///
/// WHY: The Gate-Passed trailer is the primary gate signal -- it proves
/// the local gate passed. CI checks are informational only. When the
/// trailer is present, upgrade any non-green status to Green so the steward
/// can merge without depending on CI minutes or queued checks.
#[must_use]
pub fn apply_gate_trailer_override(
    ci_status: CiStatus,
    has_gate_trailer: bool,
    pr_number: u64,
) -> CiStatus {
    if has_gate_trailer && ci_status != CiStatus::Green {
        tracing::info!(
            pr_number,
            prev_status = ?ci_status,
            "Gate-Passed trailer found, treating as green"
        );
        CiStatus::Green
    } else {
        ci_status
    }
}

/// Parse the new-file start line from a unified diff hunk header.
///
/// Format: `@@ -old_start,old_count +new_start,new_count @@`
#[must_use]
pub(crate) fn parse_hunk_new_start(hunk_line: &str) -> Option<u32> {
    let plus_idx = hunk_line.find('+')?;
    let after_plus = hunk_line.get(plus_idx + 1..)?;
    let end = after_plus.find(|c: char| !c.is_ascii_digit())?;
    after_plus.get(..end)?.parse().ok()
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
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

    // -----------------------------------------------------------------------
    // Gate trailer CI override tests
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Structural suppression detection tests
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // cfg_attr detection tests
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // lint-ignore file detection tests
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // lint-ignore inline comment detection tests
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // SAFETY/INVARIANT comment bypass detection tests
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Combined pattern detection tests
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // QA verdict extraction tests
    // -----------------------------------------------------------------------

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

    // -----------------------------------------------------------------------
    // Hunk header parsing tests
    // -----------------------------------------------------------------------

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
}
