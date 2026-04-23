//! Basanos: planning and standards linter and component auditor.
//!
//! Subcommands:
//! - `lint [PROJECT_ROOT]` — scan project planning artifacts and code
//! - `audit component <CRATE> [--format json|markdown]` — 8-check audit report for a crate

#![deny(clippy::unwrap_used)]

use basanos::commands;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!(
            "Usage: basanos [lint [PROJECT_ROOT] | audit component <CRATE> [--format json|markdown]]"
        );
        return Err("missing subcommand".into());
    }

    match args[1].as_str() {
        "lint" => {
            let project_root = args.get(2).map_or(".", String::as_str);
            commands::run_lint(project_root)?;
            Ok(())
        }
        "audit" => {
            if args.len() < 4 || args[2] != "component" {
                eprintln!("Usage: basanos audit component <CRATE> [--format json|markdown]");
                return Err("invalid audit arguments".into());
            }
            let crate_name = &args[3];
            let format = if args.len() > 5 && args[4] == "--format" {
                args[5].as_str()
            } else {
                "json"
            };
            let project_root = ".";
            let output =
                commands::audit_component::run_audit_component(crate_name, project_root, format)?;
            println!("{}", output);
            Ok(())
        }
        _ => {
            eprintln!(
                "Unknown subcommand: {}. Use 'lint' or 'audit component'.",
                args[1]
            );
            Err("unknown subcommand".into())
        }
    }
}
