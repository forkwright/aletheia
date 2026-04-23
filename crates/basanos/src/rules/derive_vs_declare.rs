//! Derive-vs-declare rule: flag properties announced rather than exhibited.
//!
//! Detects common cases of components declaring properties (hardcoded aggregate
//! status) instead of deriving them from subsystem observations. The principle:
//! a health endpoint should return per-check results; the aggregate status is
//! derived from those results, not announced separately.

use std::fs;
use std::path::{Path, PathBuf};

use snafu::ResultExt;

use crate::error::{self, Result};
use crate::rules::{Rule, Violation};

/// Rule: STANDARDS/declare-without-derive.
///
/// Detects two patterns:
/// 1. Health endpoints that return only `"status": "..."` without per-check details.
/// 2. Version endpoints that return only `"version": "..."` without build metadata (git sha, timestamp, etc.).
pub struct DeriveVsDeclareRule;

impl Rule for DeriveVsDeclareRule {
    fn id(&self) -> &'static str {
        "STANDARDS/declare-without-derive"
    }

    fn check(&self, project_root: &str) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        let root = Path::new(project_root);

        // Collect all handler files (handlers/ subdirectories in each crate).
        let mut handler_files = Vec::new();
        collect_handler_files(root, &mut handler_files)?;

        // Check each handler for declaration patterns.
        for path in handler_files {
            check_handler_for_declarations(&path, &mut violations)?;
        }

        Ok(violations)
    }
}

/// Check a handler file for health and version endpoint declarations.
#[tracing::instrument(skip(violations))]
fn check_handler_for_declarations(path: &Path, violations: &mut Vec<Violation>) -> Result<()> {
    let content = fs::read_to_string(path).with_context(|_| error::ReadFileSnafu {
        path: path.to_path_buf(),
    })?;

    check_health_declaration(path, &content, violations);
    check_version_declaration(path, &content, violations);

    Ok(())
}

/// Detect health endpoints that return only a single `"status"` field without
/// per-check details.
///
/// Pattern: `Json(json!({"status": "..."}))` or similar one-field JSON responses
/// where the only key is "status".
fn check_health_declaration(path: &Path, content: &str, violations: &mut Vec<Violation>) {
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Skip non-JSON response patterns
        if !trimmed.contains("status") || !trimmed.contains('"') {
            continue;
        }

        // Heuristic: look for patterns like `"status": "healthy"`, `"status": "ok"`, etc.
        // in Json/json! responses that don't appear to have other fields.
        if (trimmed.contains(r#""status":"#) || trimmed.contains(r#""status" :"#))
            && (trimmed.contains("healthy") || trimmed.contains("ok") || trimmed.contains("pass"))
        {
            // Check if this is a single-field JSON declaration (no other fields visible).
            // Look for patterns like: `{"status": "healthy"}` without other keys.
            // This is a simple heuristic: if the line contains only status and no other keys,
            // it's likely a declaration.
            let json_section = extract_json_from_line(trimmed);

            if json_section
                .as_deref()
                .is_some_and(|s| is_single_field_json(s, "status"))
            {
                violations.push(Violation {
                    rule: "STANDARDS/declare-without-derive".into(),
                    path: path.display().to_string(),
                    line: line_num + 1,
                    message: "[warn] Health endpoint declares only `\"status\"` field. \
                               Exhibit per-check results (e.g., database, cache, providers) \
                               and derive the aggregate status from those checks. \
                               See STANDARDS.md: Derivation Before Declaration."
                        .into(),
                });
            }
        }
    }
}

/// Detect version endpoints that return only a version string without build metadata.
///
/// Pattern: `Json(json!({"version": "0.1.0"}))` without `git_sha`, `build_id`, timestamp, etc.
fn check_version_declaration(path: &Path, content: &str, violations: &mut Vec<Violation>) {
    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Skip non-JSON response patterns
        if !trimmed.contains("version") || !trimmed.contains('"') {
            continue;
        }

        // Heuristic: look for patterns like `"version": "..."` in Json/json! responses.
        if (trimmed.contains(r#""version":"#) || trimmed.contains(r#""version" :"#))
            && (trimmed.contains("0.") || trimmed.contains("1.") || trimmed.contains("2."))
        {
            // Check if this is a single-field JSON declaration.
            let json_section = extract_json_from_line(trimmed);

            if json_section
                .as_deref()
                .is_some_and(|s| is_single_field_json(s, "version"))
            {
                violations.push(Violation {
                    rule: "STANDARDS/declare-without-derive".into(),
                    path: path.display().to_string(),
                    line: line_num + 1,
                    message: "[warn] Version endpoint declares only `\"version\"` field. \
                               Include build metadata (`git_sha`, `build_timestamp`, etc.). \
                               Version alone is a declaration; metadata transforms it into \
                               an exhibit of the actual build. See STANDARDS.md: Derivation Before Declaration."
                        .into(),
                });
            }
        }
    }
}

/// Extract JSON object from a line (simple heuristic: from `{` to `}`).
fn extract_json_from_line(line: &str) -> Option<String> {
    let start = line.find('{')?;
    let end = line.rfind('}')?;
    if end > start {
        Some(
            line.chars()
                .skip(start)
                .take(end - start + 1)
                .collect::<String>(),
        )
    } else {
        None
    }
}

/// Check if a JSON string contains only one field (the specified key).
///
/// This is a simple heuristic: counts the number of `:` at the top level
/// (outside of quoted strings). If there's exactly one, it's a single-field object.
fn is_single_field_json(json: &str, expected_key: &str) -> bool {
    // Quick sanity check: does it contain the expected key?
    if !json.contains(expected_key) {
        return false;
    }

    // Count colons outside quoted strings (simple approximation).
    let mut in_string = false;
    let mut escape_next = false;
    let mut colon_count = 0;

    for ch in json.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            ':' if !in_string => colon_count += 1,
            _ => {}
        }
    }

    // Single field = exactly one colon at the top level.
    colon_count == 1
}

/// Collect all handler files recursively from a directory.
///
/// Looks for files under `handlers/` subdirectories in crates.
fn collect_handler_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|_| error::ReadDirSnafu {
        path: dir.to_path_buf(),
    })? {
        let entry = entry.with_context(|_| error::ReadDirSnafu {
            path: dir.to_path_buf(),
        })?;
        let path = entry.path();

        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            // Skip hidden dirs and common exclusions.
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }

            // Check if this is a `handlers` directory.
            if name == "handlers" {
                // Collect .rs files from this handlers directory.
                if let Ok(entries) = fs::read_dir(&path) {
                    for entry in entries.flatten() {
                        let entry_path = entry.path();
                        if entry_path.is_file()
                            && entry_path
                                .extension()
                                .is_some_and(|ext| ext.eq_ignore_ascii_case("rs"))
                        {
                            files.push(entry_path);
                        }
                    }
                }
            } else {
                // Recurse into other directories.
                collect_handler_files(&path, files)?;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test setup")]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn health_declaration_flagged() {
        let tmp = TempDir::new().expect("temp dir created");
        let handler_dir = tmp
            .path()
            .join("crates")
            .join("pylon")
            .join("src")
            .join("handlers");
        fs::create_dir_all(&handler_dir).expect("dir created");

        let file = handler_dir.join("health.rs");
        let content = r#"
async fn health_check() -> Json<HealthResponse> {
    Json(json!({"status": "healthy"}))
}
"#;
        fs::write(&file, content).expect("file written");

        let mut violations = Vec::new();
        check_handler_for_declarations(&file, &mut violations).expect("check completed");

        assert!(
            !violations.is_empty(),
            "expected health declaration violation"
        );
        let first = violations.first().expect("violation exists");
        assert!(first.message.contains("Health endpoint"));
    }

    #[test]
    fn health_with_checks_not_flagged() {
        let tmp = TempDir::new().expect("temp dir created");
        let handler_dir = tmp
            .path()
            .join("crates")
            .join("pylon")
            .join("src")
            .join("handlers");
        fs::create_dir_all(&handler_dir).expect("dir created");

        let file = handler_dir.join("health.rs");
        let content = r#"
async fn health_check() -> Json<HealthResponse> {
    let checks = vec![
        HealthCheck { name: "db", status: "pass" },
    ];
    let status = if checks.iter().any(|c| c.status == "fail") {
        "unhealthy"
    } else {
        "healthy"
    };
    Json(json!({"status": status, "checks": checks}))
}
"#;
        fs::write(&file, content).expect("file written");

        let mut violations = Vec::new();
        check_handler_for_declarations(&file, &mut violations).expect("check completed");

        // Should not flag because it has multiple fields (status + checks)
        let health_violations: Vec<_> = violations
            .iter()
            .filter(|v| v.message.contains("Health endpoint"))
            .collect();
        assert!(
            health_violations.is_empty(),
            "expected no violations for multi-field response; got: {health_violations:?}"
        );
    }

    #[test]
    fn version_declaration_flagged() {
        let tmp = TempDir::new().expect("temp dir created");
        let handler_dir = tmp
            .path()
            .join("crates")
            .join("pylon")
            .join("src")
            .join("handlers");
        fs::create_dir_all(&handler_dir).expect("dir created");

        let file = handler_dir.join("version.rs");
        let content = r#"
async fn version() -> Json<VersionResponse> {
    Json(json!({"version": "0.1.0"}))
}
"#;
        fs::write(&file, content).expect("file written");

        let mut violations = Vec::new();
        check_handler_for_declarations(&file, &mut violations).expect("check completed");

        assert!(
            !violations.is_empty(),
            "expected version declaration violation"
        );
        let first = violations.first().expect("violation exists");
        assert!(first.message.contains("Version endpoint"));
    }

    #[test]
    fn version_with_metadata_not_flagged() {
        let tmp = TempDir::new().expect("temp dir created");
        let handler_dir = tmp
            .path()
            .join("crates")
            .join("pylon")
            .join("src")
            .join("handlers");
        fs::create_dir_all(&handler_dir).expect("dir created");

        let file = handler_dir.join("version.rs");
        let content = r#"
async fn version() -> Json<VersionResponse> {
    Json(json!({
        "version": "0.1.0",
        "git_sha": "abc123",
        "build_timestamp": "2025-01-01T00:00:00Z"
    }))
}
"#;
        fs::write(&file, content).expect("file written");

        let mut violations = Vec::new();
        check_handler_for_declarations(&file, &mut violations).expect("check completed");

        // Should not flag because it has multiple fields
        let version_violations: Vec<_> = violations
            .iter()
            .filter(|v| v.message.contains("Version endpoint"))
            .collect();
        assert!(
            version_violations.is_empty(),
            "expected no violations for version with metadata; got: {version_violations:?}"
        );
    }

    #[test]
    fn handlers_dir_collection() {
        let tmp = TempDir::new().expect("temp dir created");
        let handlers = tmp
            .path()
            .join("crates")
            .join("pylon")
            .join("src")
            .join("handlers");
        fs::create_dir_all(&handlers).expect("dir created");
        fs::write(handlers.join("health.rs"), "// health").expect("write");
        fs::write(handlers.join("version.rs"), "// version").expect("write");

        let mut files = Vec::new();
        collect_handler_files(tmp.path(), &mut files).expect("collection completed");

        assert_eq!(files.len(), 2, "expected two handler files; got: {files:?}");
    }

    #[test]
    fn single_field_json_detection() {
        assert!(is_single_field_json(r#"{"status": "healthy"}"#, "status"));
        assert!(is_single_field_json(r#"{"version": "0.1.0"}"#, "version"));
        assert!(!is_single_field_json(
            r#"{"status": "healthy", "checks": []}"#,
            "status"
        ));
        assert!(!is_single_field_json(
            r#"{"version": "0.1.0", "git_sha": "abc"}"#,
            "version"
        ));
    }

    #[test]
    fn json_extraction_from_line() {
        let line = r#"    Json(json!({"status": "healthy"})) "#;
        let extracted = extract_json_from_line(line);
        assert_eq!(extracted, Some(r#"{"status": "healthy"}"#.to_owned()));
    }
}
