//! Planning rules: missing falsifier and unfalsifiable claims.

use std::fs;
use std::path::Path;

use snafu::ResultExt;

use crate::error::{self, Result};
use crate::rules::{Rule, Violation};

/// Rule: PLANNING/missing-falsifier.
///
/// Ensures every phase PLAN.md has a Falsification section that covers
/// all success criteria, and that vision.md / ROADMAP.md do not contain
/// unfalsifiable adjectives without measurement.
pub struct MissingFalsifierRule;

impl Rule for MissingFalsifierRule {
    fn id(&self) -> &'static str {
        "PLANNING/missing-falsifier"
    }

    fn check(&self, project_root: &str) -> Result<Vec<Violation>> {
        let mut violations = Vec::new();
        let root = Path::new(project_root);

        // 1. Phase plans.
        let phases_dir = root.join("phases");
        if phases_dir.is_dir() {
            check_phase_plans(&phases_dir, &mut violations)?;
        }

        // 2. Vision and roadmap docs.
        for name in &["vision.md", "ROADMAP.md"] {
            let path = root.join(name);
            if path.is_file() {
                check_unfalsifiable_adjectives(&path, &mut violations)?;
            }
        }

        Ok(violations)
    }
}

/// Scan all `phases/*/PLAN.md` files under `phases_dir`.
fn check_phase_plans(phases_dir: &Path, violations: &mut Vec<Violation>) -> Result<()> {
    for entry in fs::read_dir(phases_dir).with_context(|_| error::ReadDirSnafu {
        path: phases_dir.to_path_buf(),
    })? {
        let entry = entry.with_context(|_| error::ReadDirSnafu {
            path: phases_dir.to_path_buf(),
        })?;
        let plan_path = entry.path().join("PLAN.md");
        if plan_path.is_file() {
            check_single_plan(&plan_path, violations)?;
        }
    }
    Ok(())
}

/// Check one PLAN.md for missing or incomplete falsification.
fn check_single_plan(path: &Path, violations: &mut Vec<Violation>) -> Result<()> {
    let content = fs::read_to_string(path).with_context(|_| error::ReadFileSnafu {
        path: path.to_path_buf(),
    })?;

    let criteria = extract_success_criteria(&content);
    if criteria.is_empty() {
        // No success criteria listed — nothing to falsify.
        return Ok(());
    }

    let falsification = extract_falsification_section(&content);

    if falsification.is_empty() {
        let line = find_line(&content, "## Success criteria").unwrap_or(1);
        violations.push(Violation {
            rule: "PLANNING/missing-falsifier".into(),
            path: path.display().to_string(),
            line,
            message: format!(
                "PLAN.md has {} success criterion/criteria but no ## Falsification section",
                criteria.len()
            ),
        });
        return Ok(());
    }

    // Each criterion must have a matching falsifier row.
    for (criterion, criterion_line) in &criteria {
        if !falsification_contains(falsification, criterion) {
            violations.push(Violation {
                rule: "PLANNING/missing-falsifier".into(),
                path: path.display().to_string(),
                line: *criterion_line,
                message: format!("Success criterion '{criterion}' has no corresponding falsifier"),
            });
        }
    }

    Ok(())
}

/// Extract bullet criteria from the `## Success criteria` section.
/// Returns a Vec of (`criterion_text`, `line_number`).
fn extract_success_criteria(content: &str) -> Vec<(String, usize)> {
    let mut criteria = Vec::new();
    let mut in_section = false;

    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("## success criteria") {
            in_section = true;
            continue;
        }
        if in_section {
            if trimmed.starts_with("## ") {
                break;
            }
            if trimmed.starts_with('-') || trimmed.starts_with('*') {
                let text = trimmed.trim_start_matches(['-', '*']).trim();
                if !text.is_empty() {
                    criteria.push((text.to_string(), idx + 1));
                }
            }
        }
    }

    criteria
}

/// Extract the text of the `## Falsification` section.
fn extract_falsification_section(content: &str) -> &str {
    let start = content.find("\n## Falsification").or_else(|| {
        content
            .find("\n## falsification")
            .or_else(|| content.find("## Falsification"))
    });

    let Some(start) = start else {
        return "";
    };

    let after_start = start + "\n## Falsification".len().saturating_sub(1);
    let rest = content.get(after_start..).unwrap_or("");

    let end = rest.find("\n## ").unwrap_or(rest.len());
    rest.get(..end).unwrap_or("")
}

/// Check whether the falsification section contains a row that references
/// the given criterion (case-insensitive substring match).
fn falsification_contains(falsification: &str, criterion: &str) -> bool {
    let criterion_lower = criterion.to_lowercase();
    for line in falsification.lines() {
        let line_lower = line.to_lowercase();
        // A row in a markdown table has `| ... | ... |`.
        if line_lower.contains(&criterion_lower) {
            return true;
        }
        // Also allow plain bullet matches.
        if line_lower.trim().starts_with('-') && line_lower.contains(&criterion_lower) {
            return true;
        }
    }
    false
}

/// Scan a markdown file for unfalsifiable adjectives.
fn check_unfalsifiable_adjectives(path: &Path, violations: &mut Vec<Violation>) -> Result<()> {
    let content = fs::read_to_string(path).with_context(|_| error::ReadFileSnafu {
        path: path.to_path_buf(),
    })?;

    for (idx, line) in content.lines().enumerate() {
        let lower = line.to_lowercase();
        for adj in aletheia_lexica::adjectives::UNFALSIFIABLE_ADJECTIVES {
            if lower.contains(adj) {
                violations.push(Violation {
                    rule: "PLANNING/unfalsifiable-claim".into(),
                    path: path.display().to_string(),
                    line: idx + 1,
                    message: format!(
                        "Unfalsifiable adjective '{adj}' found: add measurement, rewrite, or document as aspirational"
                    ),
                });
            }
        }
    }

    Ok(())
}

/// Find the 1-based line number of the first occurrence of `needle`.
fn find_line(content: &str, needle: &str) -> Option<usize> {
    for (idx, line) in content.lines().enumerate() {
        if line.to_lowercase().contains(&needle.to_lowercase()) {
            return Some(idx + 1);
        }
    }
    None
}

#[cfg(test)]
#[expect(clippy::indexing_slicing, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn extract_criteria_basic() {
        let md = "## Success criteria\n- Criterion A\n- Criterion B\n\n## Scope\n";
        let got = extract_success_criteria(md);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, "Criterion A");
        assert_eq!(got[1].0, "Criterion B");
    }

    #[test]
    fn extract_falsification_basic() {
        let md = "## Falsification\n\n| Criterion | Falsifier |\n| A | not A |\n\n## Other\n";
        let section = extract_falsification_section(md);
        assert!(section.contains("not A"));
    }

    #[test]
    fn falsification_contains_works() {
        let section = "| Criterion | Falsifier |\n| EM% improves >= 5pp | < 5pp |";
        assert!(falsification_contains(section, "EM% improves >= 5pp"));
        assert!(!falsification_contains(section, "Latency under 100ms"));
    }
}
