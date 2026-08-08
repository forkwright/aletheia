//! Enforced gates for the generated MCP tool inventory in CLAUDE.md.
//!
//! Both gates shell out to the Python tooling under `scripts/` so that the
//! generation logic lives in one place while running as part of the normal
//! `cargo test -p diaporeia` loop:
//!
//! - `generate-diaporeia-mcp-inventory.py --check` catches committed docs that
//!   have drifted from the live `#[tool]` surface.
//! - `test-diaporeia-mcp-inventory.py` catches regressions in the generator
//!   itself. WHY(#5443): `--check` structurally cannot — a parser that silently
//!   drops entries emits a block that the same parser then agrees with, so the
//!   drift gate stays green while the inventory is wrong.

#![expect(clippy::expect_used, reason = "test assertions may panic on failure")]

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> &'static str {
    env!("CARGO_MANIFEST_DIR")
        .split("/crates/diaporeia")
        .next()
        .expect("CARGO_MANIFEST_DIR must contain /crates/diaporeia")
}

/// Run `script` from the repo root, returning success and the captured output.
fn run_script(root: &str, script: &Path, args: &[&str]) -> (bool, String) {
    let output = Command::new("python3")
        .arg(script)
        .args(args)
        .current_dir(root)
        .output()
        .expect(
            "failed to run inventory script; is python3 available? \
             install it or run the script manually",
        );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    (
        output.status.success(),
        format!("stdout: {stdout}\nstderr: {stderr}"),
    )
}

#[test]
fn mcp_inventory_matches_committed_claude_md() {
    let root = repo_root();
    let script = PathBuf::from(root)
        .join("scripts")
        .join("generate-diaporeia-mcp-inventory.py");

    let (ok, output) = run_script(root, &script, &["--check"]);

    assert!(
        ok,
        "diaporeia MCP tool inventory drift detected\n\
         {output}\n\
         run `python3 scripts/generate-diaporeia-mcp-inventory.py` to regenerate crates/diaporeia/CLAUDE.md"
    );
}

#[test]
fn mcp_inventory_generator_self_tests_pass() {
    let root = repo_root();
    let script = PathBuf::from(root)
        .join("scripts")
        .join("test-diaporeia-mcp-inventory.py");

    let (ok, output) = run_script(root, &script, &[]);

    assert!(
        ok,
        "diaporeia MCP inventory generator self-tests failed\n\
         {output}\n\
         the generator's parsing or check-mode behaviour regressed; \
         run `python3 scripts/test-diaporeia-mcp-inventory.py` for detail"
    );
}
