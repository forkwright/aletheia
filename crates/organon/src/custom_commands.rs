//! User-defined slash commands loaded from YAML files via oikos cascade.

use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::debug;

use aletheia_koina::id::ToolName;
use aletheia_taxis::cascade::{self, CascadeEntry};
use aletheia_taxis::oikos::Oikos;

use crate::types::Reversibility;

/// A single step in a custom command's execution pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandStep {
    /// Tool name to invoke.
    pub tool: String,
    /// Arguments to pass to the tool.
    #[serde(default)]
    pub args: serde_json::Value,
}

/// A user-defined slash command loaded from a YAML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomCommandDef {
    /// Slash command name (without the leading `/`).
    pub name: String,
    /// Short description for autocomplete.
    pub description: String,
    /// Ordered list of tool invocations.
    pub steps: Vec<CommandStep>,
    /// Whether to prompt the user for confirmation before executing.
    #[serde(default)]
    pub confirm: bool,
    /// Reversibility override for the command as a whole.
    #[serde(default)]
    pub reversibility: Option<Reversibility>,
}

impl CustomCommandDef {
    /// Derive the effective reversibility for this command.
    ///
    /// If explicitly set, uses the override. Otherwise derives from the
    /// most dangerous step: a command with any irreversible step is irreversible.
    #[must_use]
    pub fn effective_reversibility(
        &self,
        lookup: impl Fn(&str) -> Option<Reversibility>,
    ) -> Reversibility {
        if let Some(rev) = self.reversibility {
            return rev;
        }

        // WHY: a chain is as reversible as its least reversible step
        let mut worst = Reversibility::FullyReversible;
        for step in &self.steps {
            let step_rev = lookup(&step.tool).unwrap_or(Reversibility::Irreversible);
            worst = max_reversibility(worst, step_rev);
        }
        worst
    }
}

/// Return the more dangerous of two reversibility levels.
fn max_reversibility(a: Reversibility, b: Reversibility) -> Reversibility {
    let rank = |r: Reversibility| -> u8 {
        match r {
            Reversibility::FullyReversible => 0,
            Reversibility::Reversible => 1,
            Reversibility::PartiallyReversible => 2,
            Reversibility::Irreversible => 3,
        }
    };
    if rank(b) > rank(a) { b } else { a }
}

/// Parse a single YAML file into a command definition.
///
/// # Errors
///
/// Returns an error string if the file cannot be read or parsed.
pub fn parse_command_file(path: &Path) -> Result<CustomCommandDef, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    parse_command_yaml(&content)
}

/// Parse YAML content into a command definition.
///
/// # Errors
///
/// Returns an error string if the YAML is malformed or missing required fields.
pub fn parse_command_yaml(content: &str) -> Result<CustomCommandDef, String> {
    let def: CustomCommandDef =
        serde_yaml::from_str(content).map_err(|e| format!("invalid command YAML: {e}"))?;

    if def.name.is_empty() {
        return Err("command name must not be empty".to_owned());
    }
    if def.steps.is_empty() {
        return Err("command must have at least one step".to_owned());
    }
    for step in &def.steps {
        if step.tool.is_empty() {
            return Err("step tool name must not be empty".to_owned());
        }
    }

    Ok(def)
}

/// Load all custom command definitions from a single directory.
///
/// Reads all `.yaml` and `.yml` files in the directory. Malformed files
/// are logged and skipped.
#[must_use]
pub fn load_commands_from_dir(dir: &Path) -> Vec<CustomCommandDef> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut commands = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "yaml" && ext != "yml" {
            continue;
        }
        match parse_command_file(&path) {
            Ok(cmd) => {
                debug!(name = %cmd.name, path = %path.display(), "loaded custom command");
                commands.push(cmd);
            }
            Err(e) => {
                debug!(path = %path.display(), error = %e, "skipped malformed command file");
            }
        }
    }
    commands
}

/// Load custom commands via oikos cascade for a given agent.
///
/// Searches the three-tier hierarchy (nous → shared → theke) in the
/// `commands/` subdirectory. Most-specific definition wins on name collision.
#[must_use]
pub fn load_commands_cascade(oikos: &Oikos, nous_id: &str) -> Vec<CustomCommandDef> {
    let mut seen = std::collections::HashSet::new();
    let mut commands = Vec::new();

    let entries_long_ext = cascade::discover(oikos, nous_id, "commands", Some("yaml"));
    let entries_short_ext = cascade::discover(oikos, nous_id, "commands", Some("yml"));

    let all_entries: Vec<CascadeEntry> = entries_long_ext
        .into_iter()
        .chain(entries_short_ext)
        .collect();

    for entry in all_entries {
        match parse_command_file(&entry.path) {
            Ok(cmd) => {
                if seen.insert(cmd.name.clone()) {
                    debug!(
                        name = %cmd.name,
                        tier = %entry.tier,
                        "loaded custom command via cascade"
                    );
                    commands.push(cmd);
                }
            }
            Err(e) => {
                debug!(
                    path = %entry.path.display(),
                    error = %e,
                    "skipped malformed command in cascade"
                );
            }
        }
    }

    commands
}

/// Validate that all tool names referenced in command steps are syntactically valid.
///
/// Returns a list of invalid tool references. An empty list means all are valid.
#[must_use]
pub fn validate_step_tools(def: &CustomCommandDef) -> Vec<String> {
    let mut invalid = Vec::new();
    for step in &def.steps {
        if ToolName::new(&step.tool).is_err() {
            invalid.push(step.tool.clone());
        }
    }
    invalid
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::indexing_slicing,
    reason = "test assertions on known-length vecs"
)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_command_yaml() {
        let yaml = r#"
name: deploy
description: Deploy to production
steps:
  - tool: exec
    args:
      command: "cargo build --release"
  - tool: exec
    args:
      command: "./deploy.sh"
confirm: true
"#;
        let cmd = parse_command_yaml(yaml).expect("valid yaml");
        assert_eq!(cmd.name, "deploy", "command name should be 'deploy'");
        assert_eq!(cmd.steps.len(), 2, "should have 2 steps");
        assert!(cmd.confirm, "confirm should be true");
        assert_eq!(
            cmd.steps[0].tool, "exec",
            "first step tool should be 'exec'"
        );
        assert_eq!(
            cmd.steps[0].args["command"], "cargo build --release",
            "first step args should contain command"
        );
    }

    #[test]
    fn parses_minimal_command() {
        let yaml = r#"
name: test
description: Run tests
steps:
  - tool: exec
    args:
      command: "cargo test"
"#;
        let cmd = parse_command_yaml(yaml).expect("valid yaml");
        assert_eq!(cmd.name, "test", "command name should be 'test'");
        assert!(!cmd.confirm, "confirm should default to false");
        assert!(
            cmd.reversibility.is_none(),
            "reversibility should default to None"
        );
    }

    #[test]
    fn rejects_empty_name() {
        let yaml = r#"
name: ""
description: Bad command
steps:
  - tool: exec
    args: {}
"#;
        let err = parse_command_yaml(yaml).expect_err("empty name");
        assert!(
            err.contains("name must not be empty"),
            "error should mention empty name: {err}"
        );
    }

    #[test]
    fn rejects_no_steps() {
        let yaml = r"
name: empty
description: No steps
steps: []
";
        let err = parse_command_yaml(yaml).expect_err("no steps");
        assert!(
            err.contains("at least one step"),
            "error should mention missing steps: {err}"
        );
    }

    #[test]
    fn rejects_empty_tool_name() {
        let yaml = r#"
name: bad
description: Bad step
steps:
  - tool: ""
    args: {}
"#;
        let err = parse_command_yaml(yaml).expect_err("empty tool");
        assert!(
            err.contains("tool name must not be empty"),
            "error should mention empty tool name: {err}"
        );
    }

    #[test]
    fn rejects_malformed_yaml() {
        let yaml = "not: valid: yaml: {{{{";
        let err = parse_command_yaml(yaml).expect_err("malformed");
        assert!(
            err.contains("invalid command YAML"),
            "error should mention invalid YAML: {err}"
        );
    }

    #[test]
    fn parses_reversibility_override() {
        let yaml = r#"
name: safe_deploy
description: Safe deploy
steps:
  - tool: exec
    args:
      command: "echo hello"
reversibility: reversible
"#;
        let cmd = parse_command_yaml(yaml).expect("valid yaml");
        assert_eq!(
            cmd.reversibility,
            Some(Reversibility::Reversible),
            "reversibility override should be Reversible"
        );
    }

    #[test]
    fn effective_reversibility_uses_override() {
        let cmd = CustomCommandDef {
            name: "test".to_owned(),
            description: "test".to_owned(),
            steps: vec![CommandStep {
                tool: "exec".to_owned(),
                args: serde_json::json!({}),
            }],
            confirm: false,
            reversibility: Some(Reversibility::Reversible),
        };

        let rev = cmd.effective_reversibility(|_| Some(Reversibility::Irreversible));
        assert_eq!(
            rev,
            Reversibility::Reversible,
            "override should take precedence"
        );
    }

    #[test]
    fn effective_reversibility_derives_from_worst_step() {
        let cmd = CustomCommandDef {
            name: "test".to_owned(),
            description: "test".to_owned(),
            steps: vec![
                CommandStep {
                    tool: "read".to_owned(),
                    args: serde_json::json!({}),
                },
                CommandStep {
                    tool: "exec".to_owned(),
                    args: serde_json::json!({}),
                },
            ],
            confirm: false,
            reversibility: None,
        };

        let rev = cmd.effective_reversibility(|tool| match tool {
            "read" => Some(Reversibility::FullyReversible),
            "exec" => Some(Reversibility::Irreversible),
            _ => None,
        });
        assert_eq!(
            rev,
            Reversibility::Irreversible,
            "chain should be as dangerous as worst step"
        );
    }

    #[test]
    fn effective_reversibility_defaults_unknown_to_irreversible() {
        let cmd = CustomCommandDef {
            name: "test".to_owned(),
            description: "test".to_owned(),
            steps: vec![CommandStep {
                tool: "unknown_tool".to_owned(),
                args: serde_json::json!({}),
            }],
            confirm: false,
            reversibility: None,
        };

        let rev = cmd.effective_reversibility(|_| None);
        assert_eq!(
            rev,
            Reversibility::Irreversible,
            "unknown tools should default to irreversible"
        );
    }

    #[test]
    fn validate_step_tools_catches_invalid_names() {
        let cmd = CustomCommandDef {
            name: "test".to_owned(),
            description: "test".to_owned(),
            steps: vec![
                CommandStep {
                    tool: "valid_tool".to_owned(),
                    args: serde_json::json!({}),
                },
                CommandStep {
                    tool: "INVALID TOOL NAME!".to_owned(),
                    args: serde_json::json!({}),
                },
            ],
            confirm: false,
            reversibility: None,
        };

        let invalid = validate_step_tools(&cmd);
        assert_eq!(invalid.len(), 1, "should find one invalid tool name");
        assert_eq!(
            invalid[0], "INVALID TOOL NAME!",
            "should report the invalid name"
        );
    }

    #[test]
    fn load_commands_from_dir_skips_non_yaml() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let yaml_path = dir.path().join("deploy.yaml");
        let txt_path = dir.path().join("readme.txt");

        #[expect(clippy::disallowed_methods, reason = "test: creating fixture files")]
        {
            std::fs::write(
                &yaml_path,
                "name: deploy\ndescription: Deploy\nsteps:\n  - tool: exec\n    args:\n      command: echo",
            )
            .expect("write yaml");
            std::fs::write(&txt_path, "not a command").expect("write txt");
        }

        let cmds = load_commands_from_dir(dir.path());
        assert_eq!(cmds.len(), 1, "should load only the yaml file");
        assert_eq!(cmds[0].name, "deploy", "loaded command should be 'deploy'");
    }

    #[test]
    fn load_commands_from_dir_handles_missing_dir() {
        let cmds = load_commands_from_dir(Path::new("/nonexistent/path"));
        assert!(
            cmds.is_empty(),
            "missing directory should return empty list"
        );
    }

    #[test]
    fn load_commands_from_dir_skips_malformed_yaml() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let bad_path = dir.path().join("bad.yaml");
        let good_path = dir.path().join("good.yaml");

        #[expect(clippy::disallowed_methods, reason = "test: creating fixture files")]
        {
            std::fs::write(&bad_path, "not: valid: yaml: {{{{").expect("write bad");
            std::fs::write(
                &good_path,
                "name: good\ndescription: Good\nsteps:\n  - tool: exec\n    args:\n      command: echo",
            )
            .expect("write good");
        }

        let cmds = load_commands_from_dir(dir.path());
        assert_eq!(cmds.len(), 1, "should skip malformed and load good one");
        assert_eq!(cmds[0].name, "good", "loaded command should be 'good'");
    }

    #[test]
    fn multi_step_command_preserves_order() {
        let yaml = r#"
name: build-and-test
description: Build and run tests
steps:
  - tool: exec
    args:
      command: "cargo build"
  - tool: exec
    args:
      command: "cargo test"
  - tool: exec
    args:
      command: "cargo clippy"
"#;
        let cmd = parse_command_yaml(yaml).expect("valid yaml");
        assert_eq!(cmd.steps.len(), 3, "should have 3 steps");
        assert_eq!(
            cmd.steps[0].args["command"], "cargo build",
            "first step should be build"
        );
        assert_eq!(
            cmd.steps[1].args["command"], "cargo test",
            "second step should be test"
        );
        assert_eq!(
            cmd.steps[2].args["command"], "cargo clippy",
            "third step should be clippy"
        );
    }

    #[test]
    fn max_reversibility_returns_worse() {
        assert_eq!(
            max_reversibility(Reversibility::FullyReversible, Reversibility::Reversible),
            Reversibility::Reversible,
            "Reversible > FullyReversible"
        );
        assert_eq!(
            max_reversibility(Reversibility::Irreversible, Reversibility::FullyReversible),
            Reversibility::Irreversible,
            "Irreversible > FullyReversible"
        );
        assert_eq!(
            max_reversibility(
                Reversibility::PartiallyReversible,
                Reversibility::PartiallyReversible
            ),
            Reversibility::PartiallyReversible,
            "same level returns same"
        );
    }
}
