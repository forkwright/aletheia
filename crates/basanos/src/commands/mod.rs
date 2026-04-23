//! Basanos subcommands: lint, audit.

pub mod audit_component;

use crate::error::Result;

/// Run the lint subcommand (the original behavior).
pub fn run_lint(project_root: &str) -> Result<()> {
    let mut any_violations = false;

    for rule in crate::rules::all_rules() {
        match rule.check(project_root) {
            Ok(violations) => {
                for v in &violations {
                    any_violations = true;
                    eprintln!("{}:{}: [{}] {}", v.path, v.line, v.rule, v.message);
                }
            }
            Err(e) => {
                any_violations = true;
                eprintln!("rule {} failed: {e}", rule.id());
            }
        }
    }

    if any_violations {
        return Err(crate::error::Error::LintViolations);
    }

    Ok(())
}
