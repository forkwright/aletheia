//! Drift check for the generated MCP tool inventory in CLAUDE.md.
//!
//! This test shells out to `scripts/generate-diaporeia-mcp-inventory.py --check`
//! so that the generation logic lives in one place (the Python script) while the
//! gate runs as part of the normal `cargo test -p diaporeia` loop.

#![expect(clippy::expect_used, reason = "test assertions may panic on failure")]

use std::path::PathBuf;
use std::process::Command;

#[test]
fn mcp_inventory_matches_committed_claude_md() {
    let repo_root = env!("CARGO_MANIFEST_DIR")
        .split("/crates/diaporeia")
        .next()
        .expect("CARGO_MANIFEST_DIR must contain /crates/diaporeia");

    let script = PathBuf::from(repo_root)
        .join("scripts")
        .join("generate-diaporeia-mcp-inventory.py");

    let output = Command::new("python3")
        .arg(&script)
        .arg("--check")
        .current_dir(repo_root)
        .output()
        .expect(
            "failed to run inventory generator; is python3 available? \
             install it or run the generator manually",
        );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "diaporeia MCP tool inventory drift detected\n\
         stdout: {stdout}\n\
         stderr: {stderr}\n\
         run `python3 scripts/generate-diaporeia-mcp-inventory.py` to regenerate crates/diaporeia/CLAUDE.md"
    );
}
