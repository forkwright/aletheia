//! API consistency rules: detect interfaces where conventions are inconsistent within one API surface.
//!
//! The principle: interfaces should arrive "as if they could not have been any other."
//! Inconsistent conventions within a single API surface indicate the API "feels worked."

use std::fs;
use std::path::Path;

use regex::Regex;

use crate::error::Result;
use crate::rules::{Rule, Violation};

/// Build a regex from a static pattern (infallible for literals).
fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|_| unreachable!("static regex {pattern} is valid"))
}

/// Rule: API/field-casing
///
/// Detect when types in the same crate use both `snake_case` and camelCase serde aliases.
/// Example: one struct uses `#[serde(rename = "userId")]` while another uses `#[serde(rename = "user_id")]`.
pub struct FieldCasingRule;

impl Rule for FieldCasingRule {
    fn id(&self) -> &'static str {
        "API/field-casing"
    }

    fn check(&self, project_root: &str) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        check_crate_field_casing(project_root, &mut violations);
        Ok(violations)
    }
}

/// Rule: API/error-variant-naming
///
/// Detect inconsistent error variant naming patterns within the same error enum.
/// Example: an error enum that uses both `NotFound` and `ItemDoesNotExist` for similar semantics.
pub struct ErrorVariantNamingRule;

impl Rule for ErrorVariantNamingRule {
    fn id(&self) -> &'static str {
        "API/error-variant-naming"
    }

    fn check(&self, project_root: &str) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        check_error_variant_patterns(project_root, &mut violations);
        Ok(violations)
    }
}

struct FieldRename {
    value: String,
    path: String,
    line: usize,
}

/// Scan a crate for field casing inconsistencies.
///
/// Walk all Rust source files and look for struct/enum definitions with serde renames.
/// Flag if the same crate mixes `snake_case` and camelCase field renames.
fn check_crate_field_casing(project_root: &str, violations: &mut Vec<Violation>) {
    let root = Path::new(project_root);
    let src_dir = root.join("src");

    if !src_dir.is_dir() {
        return;
    }

    // Regex to find serde rename attributes: #[serde(rename = "...")]
    let rename_re = regex(r#"#\[serde\(rename\s*=\s*"([^"]+)"\)"#);

    let mut all_renames = Vec::new();

    // Walk all Rust source files.
    collect_rs_files(&src_dir, &mut |path, content| {
        let display_path = path
            .strip_prefix(root)
            .unwrap_or(path)
            .display()
            .to_string();
        for (idx, line) in content.lines().enumerate() {
            if let Some(captures) = rename_re.captures(line)
                && let Some(value) = captures.get(1)
            {
                all_renames.push(FieldRename {
                    value: value.as_str().to_string(),
                    path: display_path.clone(),
                    line: idx + 1,
                });
            }
        }
    });

    // Classify as snake_case or camelCase.
    let mut has_snake = false;
    let mut has_camel = false;
    let mut snake_example = None;
    let mut camel_example = None;

    for rename in &all_renames {
        if is_snake_case(&rename.value) {
            has_snake = true;
            if snake_example.is_none() {
                snake_example = Some(rename.value.clone());
            }
        } else if is_camel_case(&rename.value) {
            has_camel = true;
            if camel_example.is_none() {
                camel_example = Some(rename.value.clone());
            }
        }
    }

    // If both patterns exist, flag them.
    if has_snake && has_camel {
        for rename in &all_renames {
            if let Some(ref example) = snake_example
                && rename.value == *example
            {
                violations.push(Violation {
                    rule: "API/field-casing".into(),
                    path: rename.path.clone(),
                    line: rename.line,
                    message: "snake_case field rename found, but crate also uses camelCase; standardize on one convention"
                        .into(),
                });
            }
            if let Some(ref example) = camel_example
                && rename.value == *example
            {
                violations.push(Violation {
                    rule: "API/field-casing".into(),
                    path: rename.path.clone(),
                    line: rename.line,
                    message: "camelCase field rename found, but crate also uses snake_case; standardize on one convention"
                        .into(),
                });
            }
        }
    }
}

/// Scan a crate for error variant naming inconsistencies.
///
/// Look for error enum definitions and check if variant names follow consistent patterns.
/// Flag if an enum mixes positive naming (e.g., `NotFound`) with negative/subject-based naming (e.g., `ItemDoesNotExist`).
fn check_error_variant_patterns(project_root: &str, violations: &mut Vec<Violation>) {
    let root = Path::new(project_root);
    let src_dir = root.join("src");

    if !src_dir.is_dir() {
        return;
    }

    // Regex patterns:
    // - error enum declarations: pub enum ...Error { ... }
    // - variant names that start with adjectives: NotFound, InvalidUser, etc.
    // - variant names with "DoesNotExist" or "Does" patterns
    let error_enum_re = regex(r"^\s*pub\s+enum\s+(\w+Error)\s*\{");
    let variant_re = regex(r"^\s*(\w+)\s*(?:\{|,|\()");

    collect_rs_files(&src_dir, &mut |_path, content| {
        let mut current_enum: Option<String> = None;
        let mut enum_variants = Vec::new();

        let mut in_enum = false;
        let mut brace_depth = 0;

        for (idx, line) in content.lines().enumerate() {
            let line_no = idx + 1;

            // Check for enum declaration.
            if let Some(caps) = error_enum_re.captures(line) {
                let enum_name = caps[1].to_string();
                // If we were tracking a previous enum, analyze it now.
                if let Some(ref name) = current_enum {
                    analyze_error_variants(name, &enum_variants, violations);
                    enum_variants.clear();
                }
                current_enum = Some(enum_name);
                in_enum = true;
                brace_depth = 0;
            }

            if in_enum {
                // Count braces to track when enum ends.
                brace_depth += line.chars().filter(|&c| c == '{').count();
                brace_depth =
                    brace_depth.saturating_sub(line.chars().filter(|&c| c == '}').count());

                // Collect variant names.
                if brace_depth > 0
                    && !line.trim().starts_with("//")
                    && let Some(caps) = variant_re.captures(line.trim())
                {
                    let variant_name = &caps[1];
                    // Skip common keywords and comments.
                    if !matches!(
                        variant_name,
                        "pub" | "enum" | "struct" | "impl" | "#[" | "derive"
                    ) {
                        enum_variants.push((variant_name.to_string(), line_no));
                    }
                }

                // If we've closed the enum, analyze it.
                if brace_depth == 0
                    && line.contains('}')
                    && in_enum
                    && let Some(ref name) = current_enum
                {
                    analyze_error_variants(name, &enum_variants, violations);
                    enum_variants.clear();
                    current_enum = None;
                    in_enum = false;
                }
            }
        }
    });
}

/// Analyze variant naming patterns in an error enum for inconsistency.
fn analyze_error_variants(
    enum_name: &str,
    variants: &[(String, usize)],
    violations: &mut Vec<Violation>,
) {
    if variants.len() < 2 {
        return;
    }

    // Classify each variant by naming pattern:
    // - "DoesNotExist" / "IsNot" / "Does" patterns
    // - adjective-only patterns: "NotFound", "Invalid", "Unauthorized"
    // - noun-based patterns: "UserError", "ItemMissing"

    let mut has_does_not_exist = false;
    let mut has_adjective = false;
    let mut has_noun_based = false;

    let does_not_exist_re = regex(r"(?i:DoesNotExist|IsNot|DoesNo)");
    let adjective_re = regex(r"^(Not|Invalid|Unauthorized|Forbidden|Conflict|Timeout|Missing)");

    for (variant, _) in variants {
        if does_not_exist_re.is_match(variant) {
            has_does_not_exist = true;
        } else if adjective_re.is_match(variant) {
            has_adjective = true;
        } else if variant.contains("Error")
            || variant.ends_with("Missing")
            || variant.ends_with("Mismatch")
        {
            has_noun_based = true;
        }
    }

    // Flag if we see mixed patterns.
    if (u8::from(has_does_not_exist) + u8::from(has_adjective) + u8::from(has_noun_based)) >= 2 {
        for (variant, line_no) in variants {
            // Report the first variant of each type.
            if (does_not_exist_re.is_match(variant) && has_adjective)
                || (adjective_re.is_match(variant) && has_does_not_exist)
            {
                violations.push(Violation {
                    rule: "API/error-variant-naming".into(),
                    path: "src/lib.rs".into(),
                    line: *line_no,
                    message: format!(
                        "{enum_name} enum has mixed naming patterns (DoesNotExist + adjectives); use one style consistently"
                    ),
                });
                return; // Report only once per enum.
            }
        }
    }
}

/// Check if a string is `snake_case` (all lowercase with underscores, no camelCase).
fn is_snake_case(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_lowercase() || c == '_' || c.is_numeric())
}

/// Check if a string is `camelCase` (starts lowercase, has uppercase letters, no underscores).
fn is_camel_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let first = s.chars().next();
    if !matches!(first, Some(c) if c.is_lowercase()) {
        return false;
    }
    s.contains(|c: char| c.is_uppercase()) && !s.contains('_')
}

/// Collect all Rust source files from a directory and process them.
fn collect_rs_files<F>(dir: &Path, callback: &mut F)
where
    F: FnMut(&Path, String),
{
    let _ = collect_rs_files_impl(dir, callback);
}

fn collect_rs_files_impl<F>(dir: &Path, callback: &mut F) -> std::io::Result<()>
where
    F: FnMut(&Path, String),
{
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            if let Ok(content) = fs::read_to_string(&path) {
                callback(&path, content);
            }
        } else if path.is_dir() && path.file_name().is_none_or(|name| name != "target") {
            collect_rs_files_impl(&path, callback)?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn check_field_casing_violation_count(snake_rename: &str, camel_rename: &str) -> usize {
        let content = format!(
            r#"
pub struct A {{
    #[serde(rename = "{snake_rename}")]
    f1: String,
}}

pub struct B {{
    #[serde(rename = "{camel_rename}")]
    f2: String,
}}
"#
        );

        // Simulate the check by counting renames directly
        let rename_re = regex(r#"#\[serde\(rename\s*=\s*"([^"]+)"\)"#);
        let mut renames = Vec::new();
        for line in content.lines() {
            if let Some(captures) = rename_re.captures(line)
                && let Some(value) = captures.get(1)
            {
                renames.push(value.as_str().to_string());
            }
        }

        let has_snake = renames.iter().any(|r| is_snake_case(r));
        let has_camel = renames.iter().any(|r| is_camel_case(r));

        if has_snake && has_camel {
            2 // Would report 2 violations in practice
        } else {
            0
        }
    }

    #[test]
    fn field_casing_detects_mixed_conventions() {
        let count = check_field_casing_violation_count("userId", "user_name");
        assert_eq!(count, 2, "should detect mixed camelCase and snake_case");
    }

    #[test]
    fn field_casing_allows_consistent_camelcase() {
        let count = check_field_casing_violation_count("userId", "userName");
        assert_eq!(count, 0, "consistent camelCase should not violate");
    }

    #[test]
    fn field_casing_allows_consistent_snake_case() {
        let count = check_field_casing_violation_count("user_id", "user_name");
        assert_eq!(count, 0, "consistent snake_case should not violate");
    }

    #[test]
    fn field_casing_reports_real_file_path() {
        use crate::rules::Rule;

        let project = tempfile::tempdir().expect("tempdir");
        let models = project.path().join("src").join("models");
        std::fs::create_dir_all(&models).expect("mkdir");
        std::fs::write(
            project.path().join("src").join("lib.rs"),
            r#"
pub struct ApiUser {
    #[serde(rename = "userId")]
    id: String,
}
"#,
        )
        .expect("write lib");
        std::fs::write(
            models.join("profile.rs"),
            r#"
pub struct Profile {
    #[serde(rename = "display_name")]
    display_name: String,
}
"#,
        )
        .expect("write model");

        let violations = FieldCasingRule
            .check(project.path().to_str().expect("utf-8 path"))
            .expect("rule check");
        assert!(
            violations.iter().any(|v| v.path == "src/models/profile.rs"),
            "expected real nested path, got {violations:?}"
        );
        assert!(
            violations
                .iter()
                .all(|v| v.path != "src/lib.rs" || v.line == 3),
            "src/lib.rs should appear only for the actual lib.rs violation"
        );
    }

    #[test]
    fn error_variant_naming_detects_mixed_adjectives_and_does_not() {
        let content = "
#[derive(Debug)]
pub enum ApiError {
    NotFound,
    InvalidUser,
    ItemDoesNotExist,
}";

        let error_enum_re = regex(r"^\s*pub\s+enum\s+(\w+Error)\s*\{");
        let variant_re = regex(r"^\s*(\w+)\s*(?:\{|,|\()");

        let mut variants = Vec::new();
        let mut in_enum = false;
        let mut brace_depth = 0;

        for line in content.lines() {
            if error_enum_re.is_match(line) {
                in_enum = true;
            }
            if in_enum {
                brace_depth += line.chars().filter(|&c| c == '{').count();
                brace_depth =
                    brace_depth.saturating_sub(line.chars().filter(|&c| c == '}').count());

                if brace_depth > 0
                    && !line.trim().starts_with("//")
                    && let Some(caps) = variant_re.captures(line.trim())
                {
                    let variant_name = &caps[1];
                    if !matches!(
                        variant_name,
                        "pub" | "enum" | "struct" | "impl" | "#[" | "derive"
                    ) {
                        variants.push(variant_name.to_string());
                    }
                }
            }
        }

        // Check if mixed patterns exist
        let does_not_exist_re = regex(r"(?i:DoesNotExist|IsNot|DoesNo)");
        let adjective_re = regex(r"^(Not|Invalid|Unauthorized|Forbidden|Conflict|Timeout|Missing)");

        let has_does_not_exist = variants.iter().any(|v| does_not_exist_re.is_match(v));
        let has_adjective = variants.iter().any(|v| adjective_re.is_match(v));

        assert!(
            has_does_not_exist && has_adjective,
            "should detect mixed adjective + DoesNotExist patterns"
        );
    }

    #[test]
    fn error_variant_naming_allows_consistent_adjectives() {
        let content = "
#[derive(Debug)]
pub enum ApiError {
    NotFound,
    InvalidUser,
    Unauthorized,
}";

        let adjective_re = regex(r"^(Not|Invalid|Unauthorized|Forbidden|Conflict|Timeout|Missing)");
        let variant_re = regex(r"^\s*(\w+)\s*(?:\{|,|\()");

        let mut variants = Vec::new();
        for line in content.lines() {
            if let Some(caps) = variant_re.captures(line.trim()) {
                let variant_name = &caps[1];
                if !matches!(
                    variant_name,
                    "pub" | "enum" | "struct" | "impl" | "#[" | "derive"
                ) {
                    variants.push(variant_name.to_string());
                }
            }
        }

        let all_adjective = variants.iter().all(|v| adjective_re.is_match(v));
        assert!(
            all_adjective,
            "consistent adjective names should not violate"
        );
    }

    #[test]
    fn is_snake_case_works() {
        assert!(is_snake_case("user_id"));
        assert!(is_snake_case("user_name_123"));
        assert!(!is_snake_case("userId"));
        assert!(!is_snake_case("UserID"));
        assert!(!is_snake_case("user-id"));
    }

    #[test]
    fn is_camel_case_works() {
        assert!(is_camel_case("userId"));
        assert!(is_camel_case("userName"));
        assert!(!is_camel_case("UserId"));
        assert!(!is_camel_case("user_id"));
        assert!(!is_camel_case("user-id"));
    }
}
