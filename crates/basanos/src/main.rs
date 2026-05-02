//! Basanos: planning and standards linter and component auditor.
//!
//! Subcommands:
//! - `lint [PROJECT_ROOT]` — scan project planning artifacts and code
//! - `audit component <CRATE> [--format json|markdown]` — 8-check audit report for a crate

#![deny(clippy::unwrap_used)]

use basanos::commands;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(String::as_str) {
        None => {
            eprintln!(
                "Usage: basanos [lint [PROJECT_ROOT] | audit component <CRATE> [--format json|markdown]]"
            );
            Err("missing subcommand".into())
        }
        Some("lint") => {
            let project_root = args.get(2).map_or(".", String::as_str);
            commands::run_lint(project_root)?;
            Ok(())
        }
        Some("audit") => {
            if let (Some("component"), Some(crate_name)) =
                (args.get(2).map(String::as_str), args.get(3))
            {
                let format = match (args.get(4).map(String::as_str), args.get(5)) {
                    (Some("--format"), Some(fmt)) => fmt.as_str(),
                    _ => "json",
                };
                let project_root = ".";
                let output = commands::audit_component::run_audit_component(
                    crate_name,
                    project_root,
                    format,
                )?;
                println!("{output}");
                Ok(())
            } else {
                eprintln!("Usage: basanos audit component <CRATE> [--format json|markdown]");
                Err("invalid audit arguments".into())
            }
        }
        Some(cmd) => {
            eprintln!("Unknown subcommand: {cmd}. Use 'lint' or 'audit component'.");
            Err("unknown subcommand".into())
        }
    }
}
