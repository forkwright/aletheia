//! Basanos: planning and standards linter for kanon projects.
//!
//! Scans project planning artifacts for missing falsifiers and
//! unfalsifiable claims.

#![deny(clippy::unwrap_used)]

mod error;
mod rules;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let project_root = args.get(1).map_or(".", String::as_str);

    let mut any_violations = false;

    for rule in rules::all_rules() {
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
        return Err("lint violations found".into());
    }

    Ok(())
}
