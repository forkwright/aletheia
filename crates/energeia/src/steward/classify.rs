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
#[expect(
    clippy::expect_used,
    reason = "compile-time constant regex patterns cannot fail"
)]
static ALLOW_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#\[allow\s*\(([^)]*)\)\s*\]").expect("valid regex"));

#[expect(
    clippy::expect_used,
    reason = "compile-time constant regex patterns cannot fail"
)]
static EXPECT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#\[expect\s*\(([^)]*)\)\s*\]").expect("valid regex"));

#[expect(
    clippy::expect_used,
    reason = "compile-time constant regex patterns cannot fail"
)]
static CFG_ATTR_ALLOW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"#\[cfg_attr\s*\([^,]*,\s*allow\s*\(([^)]*)\)\s*\)\s*\]").expect("valid regex")
});

#[expect(
    clippy::expect_used,
    reason = "compile-time constant regex patterns cannot fail"
)]
static LINT_IGNORE_INLINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"//\s*lint-ignore").expect("valid regex"));

#[expect(
    clippy::expect_used,
    reason = "compile-time constant regex patterns cannot fail"
)]
static STRUCTURED_COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"//\s*(SAFETY|INVARIANT)\s*:").expect("valid regex"));

#[expect(
    clippy::expect_used,
    reason = "compile-time constant regex patterns cannot fail"
)]
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
#[expect(
    clippy::too_many_lines,
    reason = "suppression detection branches are individually simple; splitting would obscure the diff-walking state machine"
)]
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
#[path = "classify_tests.rs"]
mod classify_tests;
