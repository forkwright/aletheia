//! Guards shared agent prompt templates against unsupported shell-style
//! commands (aletheia#5419).
//!
//! Templates under `shared/templates/sections/` are injected verbatim into
//! agent context. Two failure modes were found there:
//! - inventing a command that has no implementation at all (`bb post`,
//!   `task-create`, `distill --nous`, ...)
//! - naming a real in-process tool but describing it as a CLI invocation
//!   (`sessions_send --sessionKey ... --message ...`) instead of the JSON
//!   tool-call contract the runtime actually accepts
//!
//! Both tests read the templates directly off disk (not embedded in the
//! binary) and cross-check the second failure mode against the live
//! [`ToolRegistry`], so they stay accurate as tools are added or renamed.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use organon::builtins;
use organon::registry::ToolRegistry;
use organon::sandbox::SandboxConfig;

fn templates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../shared/templates/sections")
}

fn template_files() -> Vec<PathBuf> {
    let dir = templates_dir();
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry").path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();
    assert!(
        !files.is_empty(),
        "expected .md templates under {}",
        dir.display()
    );
    files.sort();
    files
}

fn registered_tool_names() -> HashSet<String> {
    let mut registry = ToolRegistry::new();
    builtins::register_all_with_sandbox(&mut registry, SandboxConfig::default())
        .expect("register_all_with_sandbox");
    registry
        .definitions()
        .into_iter()
        .map(|def| def.name.as_str().to_owned())
        .collect()
}

/// Literal strings from the aletheia#5419 report: shell-style commands with
/// no implementation anywhere in the codebase. A template referencing any of
/// these again is the exact regression this test exists to catch.
const KNOWN_BOGUS_COMMANDS: &[&str] = &[
    "sessions_send --sessionKey",
    "bb post",
    "bb claim",
    "bb complete",
    "bb msg",
    "task-create",
    "task-send",
    "distill --nous",
];

#[test]
fn shared_templates_do_not_reference_bogus_commands() {
    for path in template_files() {
        let content =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for bogus in KNOWN_BOGUS_COMMANDS {
            assert!(
                !content.contains(bogus),
                "{} references unsupported command `{bogus}` (aletheia#5419) - \
                 use the real tool-call contract or a real binary, never an \
                 invented shell command",
                path.display(),
            );
        }
    }
}

#[test]
fn shared_templates_never_invoke_real_tools_as_shell_commands() {
    let tool_names = registered_tool_names();

    for path in template_files() {
        let content =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

        for block in content.split("```bash\n").skip(1) {
            let Some((body, _rest)) = block.split_once("```") else {
                continue;
            };
            for line in body.lines() {
                // WHY no .trim() here: split_whitespace already skips leading and trailing
                // whitespace, so trimming first does the same work twice. The
                // trim_start_matches('$') stays — stripping a sigil is not whitespace handling.
                let Some(token) = line.trim_start_matches('$').split_whitespace().next() else {
                    continue;
                };
                assert!(
                    !tool_names.contains(token),
                    "{}: bash fence invokes `{token}` as a shell command, but it is a \
                     registered in-process tool (JSON args, not a CLI) - use the real \
                     tool-call contract in the template instead",
                    path.display(),
                );
            }
        }
    }
}
