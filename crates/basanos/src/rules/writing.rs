//! Writing discipline rules: purpose-language detection and citation-as-compression checks.

use std::fs;
use std::path::{Path, PathBuf};

use snafu::ResultExt;

use crate::error::{self, Result};
use crate::rules::{Rule, Violation};

/// Purpose-language patterns that indicate aspirational/vision language
/// in technical documentation where capability descriptions are preferred.
const PURPOSE_LANGUAGE_PATTERNS: &[&str] = &[
    "helps you",
    "designed for",
    "built to support",
    "serves the operator",
    "enables the user",
    "aligns with our values",
    "embodies our philosophy",
    "empowers",
    "unlocks",
    "transforms",
];

/// Generic emotional adjectives that need measurement context
/// in technical documentation.
const PURPOSE_ADJECTIVES: &[&str] = &["intuitive", "powerful", "elegant"];

/// Files that are exempt from the purpose-language rule.
/// Vision docs explicitly allow aspirational language.
const PURPOSE_EXEMPT_PATHS: &[&str] = &["vision.md", "ROADMAP.md"];

/// Reference patterns that fail the compression test.
/// These are heuristics; some may fire on false positives.
const BARE_REFERENCE_PATTERNS: &[&str] = &[
    "see #",
    "per #",
    "per STANDARDS.md",
    "per RUST.md",
    "per WRITING.md",
    "per PLANNING.md",
];

/// Section headers that are allowed to contain bare references.
const REFERENCE_EXEMPT_SECTIONS: &[&str] = &["## See also", "## References", "## See Also"];

/// Rule: WRITING/purpose-in-technical-doc.
///
/// Detects purpose/vision language in technical documentation that should be
/// capability descriptions instead. Exempt files: vision.md, ROADMAP.md.
pub struct PurposeInTechnicalDocRule;

impl Rule for PurposeInTechnicalDocRule {
    fn id(&self) -> &'static str {
        "WRITING/purpose-in-technical-doc"
    }

    fn check(&self, project_root: &str) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        let root = Path::new(project_root);

        // Collect all documentation files.
        let mut files = Vec::new();
        collect_doc_files(root, &mut files)?;

        // Check each file for purpose language.
        for path in files {
            if !is_purpose_exempt(&path) {
                check_purpose_language(&path, &mut violations)?;
            }
        }

        Ok(violations)
    }
}

/// Rule: WRITING/reference-must-compress.
///
/// Detects references that fail the compression test: bare issue numbers,
/// bare standards references, and citations without context. Allows references
/// that include a brief inline description or appear in reference sections.
pub struct ReferenceMustCompressRule;

impl Rule for ReferenceMustCompressRule {
    fn id(&self) -> &'static str {
        "WRITING/reference-must-compress"
    }

    fn check(&self, project_root: &str) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        let root = Path::new(project_root);

        // Collect all documentation files.
        let mut files = Vec::new();
        collect_doc_files(root, &mut files)?;

        // Check each file for bare references.
        for path in files {
            check_reference_compression(&path, &mut violations)?;
        }

        Ok(violations)
    }
}

/// Check for purpose language in a single file.
fn check_purpose_language(path: &Path, violations: &mut Vec<Violation>) -> Result<()> {
    let content = fs::read_to_string(path).with_context(|_| error::ReadFileSnafu {
        path: path.to_path_buf(),
    })?;

    for (line_num, line) in content.lines().enumerate() {
        let lower = line.to_lowercase();

        // Check for purpose-language patterns.
        for pattern in PURPOSE_LANGUAGE_PATTERNS {
            if lower.contains(pattern) {
                violations.push(Violation {
                    rule: "WRITING/purpose-in-technical-doc".into(),
                    path: path.display().to_string(),
                    line: line_num + 1,
                    message: format!(
                        "Purpose/vision language detected: '{pattern}'. Move to vision docs or rewrite as capability description."
                    ),
                });
            }
        }

        // Check for emotional adjectives without measurement.
        for adj in PURPOSE_ADJECTIVES {
            if lower.contains(adj) && !line.contains('(') {
                // Simple heuristic: if there's a parenthetical, assume it provides context.
                violations.push(Violation {
                    rule: "WRITING/purpose-in-technical-doc".into(),
                    path: path.display().to_string(),
                    line: line_num + 1,
                    message: format!(
                        "Uncontextualized emotional adjective '{adj}': add measurement or move to vision docs."
                    ),
                });
            }
        }
    }

    Ok(())
}

/// Check for bare references that fail the compression test.
fn check_reference_compression(path: &Path, violations: &mut Vec<Violation>) -> Result<()> {
    let content = fs::read_to_string(path).with_context(|_| error::ReadFileSnafu {
        path: path.to_path_buf(),
    })?;

    let mut in_reference_section = false;

    for (line_num, line) in content.lines().enumerate() {
        // Check if we're in a reference section.
        for section in REFERENCE_EXEMPT_SECTIONS {
            if line.to_lowercase().contains(&section.to_lowercase()) {
                in_reference_section = true;
            }
        }

        // Check for new section header (exit reference section).
        if line.trim().starts_with("## ") && !REFERENCE_EXEMPT_SECTIONS.contains(&line) {
            in_reference_section = false;
        }

        // Skip checks if in a reference section.
        if in_reference_section {
            continue;
        }

        let lower = line.to_lowercase();

        // Check for bare reference patterns.
        for pattern in BARE_REFERENCE_PATTERNS {
            if lower.contains(&pattern.to_lowercase()) {
                // Check if reference has inline context (parenthetical explanation).
                let has_context = line.contains('(') && line.contains(')');

                if !has_context {
                    violations.push(Violation {
                        rule: "WRITING/reference-must-compress".into(),
                        path: path.display().to_string(),
                        line: line_num + 1,
                        message: format!(
                            "Bare reference '{pattern}' fails compression test: add inline context or restate the reason."
                        ),
                    });
                }
            }
        }
    }

    Ok(())
}

/// Check if a path is a documentation file.
fn is_documentation_file(path: &Path) -> bool {
    if let Some(ext) = path.extension() {
        return ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("rs");
    }

    false
}

/// Check if a file path is exempt from purpose-language checks.
fn is_purpose_exempt(path: &Path) -> bool {
    if let Some(name) = path.file_name() {
        let name_str = name.to_string_lossy().to_lowercase();
        for exempt in PURPOSE_EXEMPT_PATHS {
            if name_str == exempt.to_lowercase() {
                return true;
            }
        }
    }
    false
}

/// Collect all documentation files recursively from a directory.
fn collect_doc_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|_| error::ReadDirSnafu {
        path: dir.to_path_buf(),
    })? {
        let entry = entry.with_context(|_| error::ReadDirSnafu {
            path: dir.to_path_buf(),
        })?;
        let path = entry.path();

        if path.is_dir() {
            // Recurse into subdirectories, but skip hidden and common exclusions.
            if let Some(name) = path.file_name() {
                let name_str = name.to_string_lossy();
                if !name_str.starts_with('.') && name_str != "target" && name_str != "node_modules"
                {
                    collect_doc_files(&path, files)?;
                }
            }
        } else if is_documentation_file(&path) {
            files.push(path);
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
    fn purpose_flagged_in_claude_md() {
        let tmp = TempDir::new().expect("temp dir created");
        let file = tmp.path().join("CLAUDE.md");
        fs::write(&file, "This feature helps you manage sessions better.").expect("file written");

        let mut violations = Vec::new();
        check_purpose_language(&file, &mut violations).expect("rule checked");

        assert!(!violations.is_empty());
        let first_violation = violations.first().expect("at least one violation found");
        assert!(first_violation.message.contains("helps you"));
    }

    #[test]
    fn purpose_not_flagged_in_vision_md() {
        let tmp = TempDir::new().expect("temp dir created");
        let file = tmp.path().join("vision.md");
        fs::write(&file, "This feature helps you manage sessions better.").expect("file written");

        let rule = PurposeInTechnicalDocRule;
        let path_str = tmp.path().to_str().expect("path is valid utf8");
        let violations = rule.check(path_str).expect("rule checked");

        // The file is exempt, so no violations should be found.
        assert!(
            violations.is_empty()
                || violations
                    .iter()
                    .all(|v| v.path != file.display().to_string())
        );
    }

    #[test]
    fn purpose_not_flagged_in_roadmap_md() {
        let tmp = TempDir::new().expect("temp dir created");
        let file = tmp.path().join("ROADMAP.md");
        fs::write(&file, "This feature transforms the user experience.").expect("file written");

        let rule = PurposeInTechnicalDocRule;
        let path_str = tmp.path().to_str().expect("path is valid utf8");
        let violations = rule.check(path_str).expect("rule checked");

        assert!(
            violations.is_empty()
                || violations
                    .iter()
                    .all(|v| v.path != file.display().to_string())
        );
    }

    #[test]
    fn citation_flagged_for_bare_issue_reference() {
        let tmp = TempDir::new().expect("temp dir created");
        let file = tmp.path().join("notes.md");
        fs::write(&file, "Per #1981 we revalidate the path.").expect("file written");

        let mut violations = Vec::new();
        check_reference_compression(&file, &mut violations).expect("rule checked");

        assert!(!violations.is_empty());
        let first_violation = violations.first().expect("at least one violation found");
        assert!(first_violation.message.contains("compression"));
    }

    #[test]
    fn citation_not_flagged_when_context_provided() {
        let tmp = TempDir::new().expect("temp dir created");
        let file = tmp.path().join("notes.md");
        fs::write(
            &file,
            "Per #1981 (which requires ATOMIC operations) we revalidate the path.",
        )
        .expect("file written");

        let mut violations = Vec::new();
        check_reference_compression(&file, &mut violations).expect("rule checked");

        // Should not flag because context is provided in parentheses.
        assert!(violations.is_empty());
    }

    #[test]
    fn citation_not_flagged_in_reference_section() {
        let tmp = TempDir::new().expect("temp dir created");
        let file = tmp.path().join("notes.md");
        let content = "## See also\n- Per #1981\n- See #2042\n";
        fs::write(&file, content).expect("file written");

        let mut violations = Vec::new();
        check_reference_compression(&file, &mut violations).expect("rule checked");

        // References in "See also" section are allowed.
        assert!(violations.is_empty());
    }

    #[test]
    fn citation_flagged_for_bare_standards_reference() {
        let tmp = TempDir::new().expect("temp dir created");
        let file = tmp.path().join("impl.md");
        fs::write(&file, "Per RUST.md we use snafu for errors.").expect("file written");

        let mut violations = Vec::new();
        check_reference_compression(&file, &mut violations).expect("rule checked");

        assert!(!violations.is_empty());
    }

    #[test]
    fn purpose_adjective_flagged_without_context() {
        let tmp = TempDir::new().expect("temp dir created");
        let file = tmp.path().join("docs.md");
        fs::write(&file, "The API is powerful and elegant.").expect("file written");

        let mut violations = Vec::new();
        check_purpose_language(&file, &mut violations).expect("rule checked");

        assert!(!violations.is_empty());
        assert!(
            violations
                .iter()
                .any(|v| v.message.contains("powerful") || v.message.contains("elegant"))
        );
    }

    #[test]
    fn purpose_adjective_not_flagged_with_parenthetical() {
        let tmp = TempDir::new().expect("temp dir created");
        let file = tmp.path().join("docs.md");
        fs::write(
            &file,
            "The API is powerful (supports 10k requests/sec) and elegant.",
        )
        .expect("file written");

        let mut violations = Vec::new();
        check_purpose_language(&file, &mut violations).expect("rule checked");

        // Since both adjectives are on a line with parentheses, our heuristic
        // assumes context is provided. This is a simple approximation.
        // No violations should be raised for this line.
        let purpose_violations: Vec<_> = violations
            .iter()
            .filter(|v| v.message.contains("emotional") || v.message.contains("Uncontextualized"))
            .collect();
        assert!(purpose_violations.is_empty());
    }
}
