// WHY: Mechanical pre-screening catches structural issues before spending LLM
// tokens on semantic evaluation. Blast radius, anti-patterns, lint, and format
// checks run deterministically on the diff or via cargo commands.

use std::path::Path;

use crate::qa::PromptSpec;
use crate::types::{MechanicalIssue, MechanicalIssueKind};

// ---------------------------------------------------------------------------
// Anti-pattern definitions
// ---------------------------------------------------------------------------

/// Patterns detected in added diff lines that violate project standards.
///
/// Each entry is `(pattern_string, human_message)`. Checked against every
/// added line in non-test files.
#[expect(clippy::unwrap_used, reason = "pattern string for anti-pattern detection, not an actual unwrap call")]
const ANTI_PATTERNS: &[(&str, &str)] = &[
    (
        ".unwrap()",
        "unwrap() call in library code — use ? operator with error context",
    ),
    (
        "#[allow(",
        "#[allow()] attribute — use #[expect(lint, reason = \"...\")] instead",
    ),
    (
        "println!",
        "println!() in library code — use tracing macros instead",
    ),
    ("todo!()", "todo!() macro left in code"),
    ("unimplemented!()", "unimplemented!() macro left in code"),
];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Run all synchronous mechanical checks on a PR diff.
///
/// Checks blast radius compliance and scans for anti-patterns. These checks
/// incur no LLM cost and run on the diff text alone.
pub fn mechanical_check(diff: &str, prompt: &PromptSpec) -> Vec<MechanicalIssue> {
    let mut issues = Vec::new();

    let changed_files = parse_changed_files(diff);

    check_blast_radius(&changed_files, &prompt.blast_radius, &mut issues);
    check_anti_patterns(diff, &mut issues);

    issues
}

/// Run `cargo fmt --check` in the given directory.
///
/// Returns a [`MechanicalIssue`] per file with formatting violations.
/// Returns an empty vec on success or if the command cannot run.
pub async fn format_check(working_dir: &Path) -> Vec<MechanicalIssue> {
    let output = match tokio::process::Command::new("cargo")
        .args(["fmt", "--all", "--", "--check"])
        .current_dir(working_dir)
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(error = %e, "cargo fmt failed to execute");
            return Vec::new();
        }
    };

    if output.status.success() {
        return Vec::new();
    }

    let stderr = String::from_utf8_lossy(&output.stdout);
    let mut issues = Vec::new();

    for line in stderr.lines() {
        // NOTE: `cargo fmt --check` emits "Diff in /path/to/file.rs" lines.
        if let Some(path) = line.strip_prefix("Diff in ") {
            issues.push(MechanicalIssue {
                kind: MechanicalIssueKind::FormatViolation,
                message: "code formatting violation".to_owned(),
                details: Some(path.trim().to_owned()),
            });
        }
    }

    // NOTE: If no specific files were parsed, emit a generic issue.
    if issues.is_empty() {
        issues.push(MechanicalIssue {
            kind: MechanicalIssueKind::FormatViolation,
            message: "cargo fmt --check failed".to_owned(),
            details: Some(stderr.into_owned()),
        });
    }

    issues
}

/// Run `cargo clippy` in the given directory.
///
/// Returns a [`MechanicalIssue`] per warning or error detected.
/// Returns an empty vec on success or if the command cannot run.
pub async fn lint_check(working_dir: &Path) -> Vec<MechanicalIssue> {
    let output = match tokio::process::Command::new("cargo")
        .args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ])
        .current_dir(working_dir)
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(error = %e, "cargo clippy failed to execute");
            return Vec::new();
        }
    };

    if output.status.success() {
        return Vec::new();
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut issues = Vec::new();

    for line in stderr.lines() {
        // NOTE: Clippy emits "warning:" and "error:" lines with context.
        if line.starts_with("warning:") || line.starts_with("error:") {
            issues.push(MechanicalIssue {
                kind: MechanicalIssueKind::LintViolation,
                message: line.to_owned(),
                details: None,
            });
        }
    }

    if issues.is_empty() {
        issues.push(MechanicalIssue {
            kind: MechanicalIssueKind::LintViolation,
            message: "cargo clippy failed".to_owned(),
            details: Some(stderr.into_owned()),
        });
    }

    issues
}

// ---------------------------------------------------------------------------
// Diff parsing
// ---------------------------------------------------------------------------

/// Extract changed file paths from a unified diff.
///
/// Parses `+++ b/path` lines which give the destination path and handle
/// renames correctly.
pub fn parse_changed_files(diff: &str) -> Vec<String> {
    let mut files = Vec::new();

    for line in diff.lines() {
        // WHY: `+++ b/path` gives the destination file path and is more
        // reliable than `diff --git` for renames.
        if let Some(path) = line.strip_prefix("+++ b/") {
            let path = path.trim();
            if path != "/dev/null" && !path.is_empty() {
                files.push(path.to_owned());
            }
        }
    }

    files
}

// ---------------------------------------------------------------------------
// Blast radius
// ---------------------------------------------------------------------------

/// Verify all changed files fall within the declared blast radius.
fn check_blast_radius(
    changed_files: &[String],
    blast_radius: &[String],
    issues: &mut Vec<MechanicalIssue>,
) {
    if blast_radius.is_empty() {
        return;
    }

    for file in changed_files {
        let within_scope = blast_radius.iter().any(|allowed| {
            // WHY: A blast radius entry ending with `/` is a directory prefix.
            // Anything under that path is allowed. Otherwise, exact match only.
            if allowed.ends_with('/') {
                file.starts_with(allowed.as_str())
            } else {
                file == allowed
            }
        });

        if !within_scope {
            issues.push(MechanicalIssue {
                kind: MechanicalIssueKind::BlastRadiusViolation,
                message: format!("file modified outside blast radius: {file}"),
                details: Some(format!("allowed paths: {}", blast_radius.join(", "))),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Anti-patterns
// ---------------------------------------------------------------------------

/// Scan added lines in the diff for known anti-patterns.
fn check_anti_patterns(diff: &str, issues: &mut Vec<MechanicalIssue>) {
    let mut current_file = String::new();
    let mut line_number: u32 = 0;

    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            path.trim().clone_into(&mut current_file);
            line_number = 0;
            continue;
        }

        // NOTE: Parse `@@ -old,count +new,count @@` to track line numbers.
        if line.starts_with("@@ ") {
            if let Some(new_start) = parse_hunk_new_start(line) {
                line_number = new_start;
            }
            continue;
        }

        // NOTE: Only check added lines (starting with `+` but not `+++`).
        if let Some(added) = line.strip_prefix('+') {
            if !line.starts_with("+++") {
                let is_test_file = current_file.contains("/tests/")
                    || current_file.contains("_test.rs")
                    || current_file.ends_with("tests.rs");

                if !is_test_file {
                    for (pattern, message) in ANTI_PATTERNS {
                        if added.contains(pattern) {
                            issues.push(MechanicalIssue {
                                kind: MechanicalIssueKind::AntiPattern,
                                message: (*message).to_owned(),
                                details: Some(format!("{current_file}:{line_number}")),
                            });
                        }
                    }

                    check_blanket_glob_import(added, &current_file, line_number, issues);
                }
            }

            line_number += 1;
        } else if !line.starts_with('-') {
            // NOTE: Context lines advance the line counter.
            line_number += 1;
        }
    }
}

/// Flag blanket `use X::*;` imports (but allow `use super::*` and `use crate::*`).
fn check_blanket_glob_import(
    line: &str,
    file: &str,
    line_number: u32,
    issues: &mut Vec<MechanicalIssue>,
) {
    let trimmed = line.trim();

    if trimmed.starts_with("use ")
        && trimmed.ends_with("::*;")
        && !trimmed.contains("super::*")
        && !trimmed.contains("crate::*")
    {
        issues.push(MechanicalIssue {
            kind: MechanicalIssueKind::AntiPattern,
            message: "blanket use X::* import — prefer explicit imports".to_owned(),
            details: Some(format!("{file}:{line_number}")),
        });
    }
}

/// Parse the new-file start line from a unified diff hunk header.
///
/// Format: `@@ -old_start,old_count +new_start,new_count @@`
fn parse_hunk_new_start(hunk_line: &str) -> Option<u32> {
    let plus_idx = hunk_line.find('+')?;
    let after_plus = hunk_line.get(plus_idx + 1..)?;
    let end = after_plus.find(|c: char| !c.is_ascii_digit())?;
    after_plus.get(..end)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // parse_changed_files
    // -----------------------------------------------------------------------

    #[test]
    fn parse_changed_files_from_diff() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!(\"hello\");
 }
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1,2 @@
+pub fn foo() {}
";
        let files = parse_changed_files(diff);
        assert_eq!(files, vec!["src/main.rs", "src/lib.rs"]);
    }

    #[test]
    fn parse_changed_files_ignores_dev_null() {
        let diff = "+++ b/new_file.rs\n--- a/deleted.rs\n+++ /dev/null\n";
        let files = parse_changed_files(diff);
        assert_eq!(files, vec!["new_file.rs"]);
    }

    // -----------------------------------------------------------------------
    // Blast radius
    // -----------------------------------------------------------------------

    #[test]
    fn blast_radius_violation_detected() {
        let diff = "+++ b/src/outside/file.rs\n@@ -1 +1,2 @@\n+new line\n";
        let prompt = stub_prompt(vec!["src/allowed/".to_owned()]);

        let issues = mechanical_check(diff, &prompt);

        assert!(
            issues
                .iter()
                .any(|i| i.kind == MechanicalIssueKind::BlastRadiusViolation)
        );
    }

    #[test]
    fn blast_radius_allows_files_in_scope() {
        let diff = "+++ b/src/allowed/file.rs\n@@ -1 +1,2 @@\n+new line\n";
        let prompt = stub_prompt(vec!["src/allowed/".to_owned()]);

        let issues = mechanical_check(diff, &prompt);

        assert!(
            !issues
                .iter()
                .any(|i| i.kind == MechanicalIssueKind::BlastRadiusViolation)
        );
    }

    #[test]
    fn blast_radius_exact_file_match() {
        let diff = "+++ b/Cargo.toml\n@@ -1 +1,2 @@\n+new line\n";
        let prompt = stub_prompt(vec!["Cargo.toml".to_owned()]);

        let issues = mechanical_check(diff, &prompt);

        assert!(
            !issues
                .iter()
                .any(|i| i.kind == MechanicalIssueKind::BlastRadiusViolation)
        );
    }

    #[test]
    fn blast_radius_empty_allows_all() {
        let diff = "+++ b/anywhere/file.rs\n@@ -1 +1,2 @@\n+new line\n";
        let prompt = stub_prompt(vec![]);

        let issues = mechanical_check(diff, &prompt);

        assert!(
            !issues
                .iter()
                .any(|i| i.kind == MechanicalIssueKind::BlastRadiusViolation)
        );
    }

    // -----------------------------------------------------------------------
    // Anti-patterns
    // -----------------------------------------------------------------------

    #[test]
    fn anti_pattern_unwrap_detected() {
        let diff = "+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+    let x = foo.unwrap();\n";
        let prompt = stub_prompt(vec![]);

        let issues = mechanical_check(diff, &prompt);

        assert!(
            issues
                .iter()
                .any(|i| i.kind == MechanicalIssueKind::AntiPattern
                    && i.message.contains("unwrap()"))
        );
    }

    #[test]
    fn anti_pattern_allow_detected() {
        let diff = "+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+#[allow(dead_code)]\n";
        let prompt = stub_prompt(vec![]);

        let issues = mechanical_check(diff, &prompt);

        assert!(issues.iter().any(
            |i| i.kind == MechanicalIssueKind::AntiPattern && i.message.contains("#[allow()]")
        ));
    }

    #[test]
    fn anti_pattern_println_detected() {
        let diff = "+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+    println!(\"debug\");\n";
        let prompt = stub_prompt(vec![]);

        let issues = mechanical_check(diff, &prompt);

        assert!(issues.iter().any(
            |i| i.kind == MechanicalIssueKind::AntiPattern && i.message.contains("println!()")
        ));
    }

    #[test]
    fn anti_pattern_todo_detected() {
        let diff = "+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+    todo!()\n";
        let prompt = stub_prompt(vec![]);

        let issues = mechanical_check(diff, &prompt);

        assert!(
            issues.iter().any(
                |i| i.kind == MechanicalIssueKind::AntiPattern && i.message.contains("todo!()")
            )
        );
    }

    #[test]
    fn anti_pattern_skipped_in_test_files() {
        let diff = "+++ b/src/tests/foo_test.rs\n@@ -1 +1,2 @@\n+    let x = foo.unwrap();\n";
        let prompt = stub_prompt(vec![]);

        let issues = mechanical_check(diff, &prompt);

        assert!(
            !issues
                .iter()
                .any(|i| i.kind == MechanicalIssueKind::AntiPattern)
        );
    }

    #[test]
    fn blanket_glob_import_detected() {
        let diff = "+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+use std::collections::*;\n";
        let prompt = stub_prompt(vec![]);

        let issues = mechanical_check(diff, &prompt);

        assert!(issues.iter().any(
            |i| i.kind == MechanicalIssueKind::AntiPattern && i.message.contains("blanket use")
        ));
    }

    #[test]
    fn super_glob_import_allowed() {
        let diff = "+++ b/src/lib.rs\n@@ -1 +1,2 @@\n+use super::*;\n";
        let prompt = stub_prompt(vec![]);

        let issues = mechanical_check(diff, &prompt);

        assert!(!issues.iter().any(|i| i.message.contains("blanket use")));
    }

    #[test]
    fn parse_hunk_new_start_basic() {
        assert_eq!(parse_hunk_new_start("@@ -1,3 +10,4 @@"), Some(10));
        assert_eq!(parse_hunk_new_start("@@ -0,0 +1,5 @@"), Some(1));
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn stub_prompt(blast_radius: Vec<String>) -> PromptSpec {
        PromptSpec {
            prompt_number: 1,
            description: "test prompt".to_owned(),
            acceptance_criteria: vec![],
            blast_radius,
        }
    }
}
