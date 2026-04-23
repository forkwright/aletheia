//! Audit component subcommand — 8-check report for a crate.
//!
//! Usage: `basanos audit component <crate> [--format json|markdown]`
//!
//! The 8 checks assess a crate for architectural health:
//!
//! 1. Generic vs specific — boilerplate ratio
//! 2. Map vs angle — tautological doc count
//! 3. Dimensional coherence — declare-without-derive findings
//! 4. Component count — pub types/fns/impls
//! 5. Dependencies — inbound/outbound
//! 6-8. Stubs for v1: indirection layers, error space, grounding
//!
//! Output: JSON (default) or markdown via `--format markdown`.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::error::{self, Result};

/// The 8-check audit report for a component crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditReport {
    /// Crate name under audit.
    pub crate_name: String,
    /// Overall pass/fail score (0-8).
    pub overall_score: u8,
    /// The 8 checks.
    pub checks: Vec<AuditCheck>,
}

/// A single audit check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditCheck {
    /// Check identifier: `generic-vs-specific`, `map-vs-angle`, etc.
    pub id: String,
    /// Result: `PASS`, `FAIL`, or `NEEDS_HUMAN`.
    pub result: CheckResult,
    /// Evidence or explanation.
    pub evidence: String,
}

/// Result of a single check.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckResult {
    /// Check passed.
    Pass,
    /// Check failed.
    Fail,
    /// Human review needed.
    NeedsHuman,
}

impl std::fmt::Display for CheckResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pass => f.write_str("PASS"),
            Self::Fail => f.write_str("FAIL"),
            Self::NeedsHuman => f.write_str("NEEDS_HUMAN"),
        }
    }
}

/// Run the audit component subcommand.
///
/// # Arguments
///
/// - `crate_name`: the crate to audit (e.g., "eidos")
/// - `project_root`: the workspace root (usually ".")
/// - `format`: output format ("json" or "markdown")
pub fn run_audit_component(crate_name: &str, project_root: &str, format: &str) -> Result<String> {
    // Verify the crate exists.
    let crate_path = Path::new(project_root).join("crates").join(crate_name);
    if !crate_path.exists() {
        return Err(error::Error::UnknownCrate {
            crate_name: crate_name.to_string(),
        });
    }

    // Run all 8 checks.
    let checks = vec![
        check_generic_vs_specific(&crate_path),
        check_map_vs_angle(&crate_path),
        check_dimensional_coherence(&crate_path, project_root),
        check_component_count(&crate_path),
        check_dependencies(&crate_path),
        check_indirection_layers_stub(),
        check_error_space_stub(),
        check_grounding_stub(),
    ];

    let pass_count = checks
        .iter()
        .filter(|c| c.result == CheckResult::Pass)
        .count();
    // Maximum 8 checks, so min(pass_count, 8) is always valid for u8.
    let overall_score = (pass_count.min(8)) as u8;

    let report = AuditReport {
        crate_name: crate_name.to_string(),
        overall_score,
        checks,
    };

    match format {
        "markdown" => format_markdown(&report),
        _ => format_json(&report),
    }
}

/// Check 1: Generic vs specific — ratio of boilerplate LOC to logic LOC.
fn check_generic_vs_specific(crate_path: &Path) -> AuditCheck {
    let src_path = crate_path.join("src");
    if !src_path.exists() {
        return AuditCheck {
            id: "generic-vs-specific".into(),
            result: CheckResult::NeedsHuman,
            evidence: "src/ directory not found".into(),
        };
    }

    // Count boilerplate patterns: error wrapping, generic Result types, etc.
    let mut boilerplate_count = 0;
    let mut total_lines = 0;

    for entry in walkdir_recursively(&src_path) {
        if let Ok(entry) = entry {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("rs") {
                if let Ok(content) = fs::read_to_string(entry.path()) {
                    for line in content.lines() {
                        total_lines += 1;
                        let trimmed = line.trim();

                        // Count boilerplate patterns: generic error handling, Result wrapping.
                        if trimmed.contains("Result<")
                            || trimmed.contains(".context(")
                            || trimmed.contains("snafu::")
                            || trimmed.contains("WithContext")
                        {
                            boilerplate_count += 1;
                        }
                    }
                }
            }
        }
    }

    let boilerplate_ratio = if total_lines > 0 {
        f64::from(boilerplate_count) / f64::from(total_lines) * 100.0
    } else {
        0.0
    };

    let (result, evidence) = if boilerplate_ratio < 20.0 {
        (
            CheckResult::Pass,
            format!("{boilerplate_ratio:.1}% boilerplate ratio (below 20% threshold)"),
        )
    } else if boilerplate_ratio < 35.0 {
        (
            CheckResult::Pass,
            format!("{boilerplate_ratio:.1}% boilerplate ratio (acceptable, below 35%)"),
        )
    } else {
        (
            CheckResult::Fail,
            format!(
                "{boilerplate_ratio:.1}% boilerplate ratio (exceeds 35% threshold; \
                 consider higher-order abstractions)"
            ),
        )
    };

    AuditCheck {
        id: "generic-vs-specific".into(),
        result,
        evidence,
    }
}

/// Check 2: Map vs angle — tautological doc count (docs that say "Returns X" without behavior).
fn check_map_vs_angle(crate_path: &Path) -> AuditCheck {
    let src_path = crate_path.join("src");
    if !src_path.exists() {
        return AuditCheck {
            id: "map-vs-angle".into(),
            result: CheckResult::NeedsHuman,
            evidence: "src/ directory not found".into(),
        };
    }

    let mut tautological_count = 0;
    let mut doc_count = 0;

    for entry in walkdir_recursively(&src_path) {
        if let Ok(entry) = entry {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("rs") {
                if let Ok(content) = fs::read_to_string(entry.path()) {
                    for line in content.lines() {
                        let trimmed = line.trim();

                        // Count documentation lines.
                        if trimmed.starts_with("///") {
                            doc_count += 1;

                            // Detect tautological docs: "Returns X", "Does Y", "Checks Z".
                            let lower = trimmed.to_lowercase();
                            if lower.contains("returns")
                                || (lower.contains("does") && !lower.contains("does not"))
                                || lower.contains("checks")
                                || (lower.contains("takes") && lower.contains("as parameter"))
                            {
                                // Check if it's pure description without behavior.
                                if trimmed.chars().filter(|c| c.is_alphanumeric()).count() < 40 {
                                    tautological_count += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let tautological_ratio = if doc_count > 0 {
        f64::from(tautological_count) / f64::from(doc_count) * 100.0
    } else {
        0.0
    };

    let (result, evidence) = if tautological_ratio < 30.0 {
        (
            CheckResult::Pass,
            format!(
                "{tautological_count} tautological docs out of {doc_count} \
                 (ratio {tautological_ratio:.1}%, acceptable)"
            ),
        )
    } else if tautological_ratio < 50.0 {
        (
            CheckResult::Fail,
            format!(
                "{tautological_count} tautological docs out of {doc_count} \
                 (ratio {tautological_ratio:.1}%, exceeds 30% threshold)"
            ),
        )
    } else {
        (
            CheckResult::Fail,
            format!(
                "{tautological_count} tautological docs out of {doc_count} \
                 (ratio {tautological_ratio:.1}%, many docs lack behavior explanation)"
            ),
        )
    };

    AuditCheck {
        id: "map-vs-angle".into(),
        result,
        evidence,
    }
}

/// Check 3: Dimensional coherence — cross-reference derive-vs-declare findings.
fn check_dimensional_coherence(crate_path: &Path, project_root: &str) -> AuditCheck {
    use crate::rules::Rule;

    // Reuse the derive-vs-declare rule to find handlers in this crate that violate the standard.
    let rule = crate::rules::DeriveVsDeclareRule;
    match rule.check(project_root) {
        Ok(violations) => {
            // Filter violations to those in this crate.
            let crate_name = crate_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            let crate_violations: Vec<_> = violations
                .iter()
                .filter(|v| v.path.contains(&format!("crates/{crate_name}")))
                .collect();

            let (result, evidence) = if crate_violations.is_empty() {
                (
                    CheckResult::Pass,
                    "No declare-without-derive violations found".into(),
                )
            } else {
                (
                    CheckResult::Fail,
                    format!(
                        "{} handlers flagged by STANDARDS/declare-without-derive",
                        crate_violations.len()
                    ),
                )
            };

            AuditCheck {
                id: "dimensional-coherence".into(),
                result,
                evidence,
            }
        }
        Err(_) => AuditCheck {
            id: "dimensional-coherence".into(),
            result: CheckResult::NeedsHuman,
            evidence:
                "Could not run derive-vs-declare check; manual review of handlers recommended"
                    .into(),
        },
    }
}

/// Check 4: Component count — count pub types, pub fns, pub trait impls.
fn check_component_count(crate_path: &Path) -> AuditCheck {
    let src_path = crate_path.join("src");
    if !src_path.exists() {
        return AuditCheck {
            id: "component-count".into(),
            result: CheckResult::NeedsHuman,
            evidence: "src/ directory not found".into(),
        };
    }

    let mut pub_type_count = 0;
    let mut pub_fn_count = 0;
    let mut pub_impl_count = 0;

    for entry in walkdir_recursively(&src_path) {
        if let Ok(entry) = entry {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("rs") {
                if let Ok(content) = fs::read_to_string(entry.path()) {
                    for line in content.lines() {
                        let trimmed = line.trim();

                        if trimmed.starts_with("pub ") || trimmed.starts_with("pub(") {
                            if trimmed.contains("struct ")
                                || trimmed.contains("enum ")
                                || trimmed.contains("type ")
                            {
                                pub_type_count += 1;
                            } else if trimmed.contains("fn ") {
                                pub_fn_count += 1;
                            } else if trimmed.contains("impl ") {
                                pub_impl_count += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    let total_components = pub_type_count + pub_fn_count + pub_impl_count;
    let evidence = format!(
        "{pub_type_count} pub types, {pub_fn_count} pub functions, \
         {pub_impl_count} pub impls (total: {total_components})"
    );

    // Threshold heuristic: 20+ components is reasonable for a crate.
    let result = if total_components >= 20 {
        CheckResult::Pass
    } else {
        CheckResult::Fail
    };

    AuditCheck {
        id: "component-count".into(),
        result,
        evidence,
    }
}

/// Check 5: Dependencies — inbound/outbound from this crate.
fn check_dependencies(crate_path: &Path) -> AuditCheck {
    let cargo_toml = crate_path.join("Cargo.toml");
    if !cargo_toml.exists() {
        return AuditCheck {
            id: "dependencies".into(),
            result: CheckResult::NeedsHuman,
            evidence: "Cargo.toml not found".into(),
        };
    }

    // Parse Cargo.toml to count dependencies.
    match fs::read_to_string(&cargo_toml) {
        Ok(content) => {
            let dep_count = content
                .lines()
                .filter(|line| {
                    let trimmed = line.trim();
                    trimmed.ends_with(" = { workspace = true }")
                        || trimmed.ends_with(" = { version = ")
                        || (trimmed.ends_with(" = { workspace = true }\"")
                            && !trimmed.starts_with('#'))
                })
                .count();

            let evidence = if dep_count < 5 {
                format!("{dep_count} dependencies (lean, good for reusability)")
            } else if dep_count < 15 {
                format!("{dep_count} dependencies (moderate, reasonable for a component)")
            } else {
                format!("{dep_count} dependencies (heavy, consider breaking into smaller crates)")
            };

            let result = if dep_count < 20 {
                CheckResult::Pass
            } else {
                CheckResult::Fail
            };

            AuditCheck {
                id: "dependencies".into(),
                result,
                evidence,
            }
        }
        Err(_) => AuditCheck {
            id: "dependencies".into(),
            result: CheckResult::NeedsHuman,
            evidence: "Could not read Cargo.toml".into(),
        },
    }
}

/// Check 6: Indirection layers — STUB for v1.
fn check_indirection_layers_stub() -> AuditCheck {
    AuditCheck {
        id: "indirection-layers".into(),
        result: CheckResult::NeedsHuman,
        evidence: "Not yet implemented; requires trait/wrapper detection heuristics".into(),
    }
}

/// Check 7: Error space — STUB for v1.
fn check_error_space_stub() -> AuditCheck {
    AuditCheck {
        id: "error-space".into(),
        result: CheckResult::NeedsHuman,
        evidence:
            "Not yet implemented; requires error variant enumeration and code-size correlation"
                .into(),
    }
}

/// Check 8: Grounding — STUB for v1.
fn check_grounding_stub() -> AuditCheck {
    AuditCheck {
        id: "grounding".into(),
        result: CheckResult::NeedsHuman,
        evidence: "Not yet implemented; requires example doc count and test coverage analysis"
            .into(),
    }
}

/// Format the report as JSON.
fn format_json(report: &AuditReport) -> Result<String> {
    serde_json::to_string_pretty(report).with_context(|_| error::SerializeJsonSnafu)
}

/// Format the report as markdown.
fn format_markdown(report: &AuditReport) -> Result<String> {
    let mut output = String::new();
    output.push_str(&format!("# Audit Report: {}\n\n", report.crate_name));
    output.push_str(&format!(
        "**Overall Score**: {}/8\n\n",
        report.overall_score
    ));

    output.push_str("## Checks\n\n");
    for check in &report.checks {
        output.push_str(&format!(
            "### {}\n\n**Result**: {}\n\n**Evidence**: {}\n\n",
            check.id, check.result, check.evidence
        ));
    }

    Ok(output)
}

/// Recursively walk a directory, yielding entries.
fn walkdir_recursively(path: &Path) -> Vec<std::io::Result<fs::DirEntry>> {
    let mut entries = Vec::new();
    if let Ok(dir_entries) = fs::read_dir(path) {
        for entry in dir_entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_dir() {
                    entries.extend(walkdir_recursively(&entry.path()));
                } else {
                    entries.push(Ok(entry));
                }
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_component_unknown_crate() {
        let result = run_audit_component("nonexistent", ".", "json");
        assert!(result.is_err());
    }

    #[test]
    fn test_audit_component_on_known_crate_returns_report() {
        let result = run_audit_component("eidos", ".", "json");
        assert!(result.is_ok());

        let json_str = result.unwrap();
        let report: AuditReport = serde_json::from_str(&json_str).expect("valid JSON");

        // Eidos should have a good audit score.
        assert_eq!(report.crate_name, "eidos");
        assert!(!report.checks.is_empty());
        assert_eq!(report.checks.len(), 8);
    }

    #[test]
    fn test_report_json_round_trip() {
        let report = AuditReport {
            crate_name: "test".into(),
            overall_score: 5,
            checks: vec![AuditCheck {
                id: "test-check".into(),
                result: CheckResult::Pass,
                evidence: "test evidence".into(),
            }],
        };

        let json_str = serde_json::to_string(&report).expect("serialization");
        let deserialized: AuditReport = serde_json::from_str(&json_str).expect("deserialization");

        assert_eq!(report.crate_name, deserialized.crate_name);
        assert_eq!(report.overall_score, deserialized.overall_score);
        assert_eq!(report.checks.len(), deserialized.checks.len());
    }

    #[test]
    fn test_check_result_display() {
        assert_eq!(CheckResult::Pass.to_string(), "PASS");
        assert_eq!(CheckResult::Fail.to_string(), "FAIL");
        assert_eq!(CheckResult::NeedsHuman.to_string(), "NEEDS_HUMAN");
    }

    #[test]
    fn test_audit_component_markdown_format() {
        let result = run_audit_component("eidos", ".", "markdown");
        assert!(result.is_ok());

        let markdown = result.unwrap();
        assert!(markdown.contains("# Audit Report:"));
        assert!(markdown.contains("eidos"));
        assert!(markdown.contains("Overall Score"));
    }
}
