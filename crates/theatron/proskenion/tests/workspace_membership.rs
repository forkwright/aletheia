//! Manifest-level guard for the root-workspace membership wiring landed by
//! forkwright/aletheia#4726.
//!
//! proskenion is now a full root-workspace member — `cargo clippy --workspace`
//! and `cargo nextest run --workspace` cover it like any other crate — but it
//! is deliberately absent from `[workspace.default-members]` so a bare
//! `cargo build`/`cargo check` (no `-p`, no `--workspace`) on a GTK-less
//! machine never tries to compile it. It also must no longer declare its own
//! `[workspace]` table: a workspace member manifest that also declares
//! `[workspace]` is a Cargo error.

#![expect(clippy::expect_used, reason = "test assertions")]

use std::fs;

const MEMBER_PATH: &str = "crates/theatron/proskenion";

#[test]
fn proskenion_is_a_full_but_non_default_workspace_member() {
    let root_manifest_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../Cargo.toml");
    let root: toml::Table = fs::read_to_string(root_manifest_path)
        .expect("root Cargo.toml is readable")
        .parse()
        .expect("root Cargo.toml parses as TOML");
    let workspace = root
        .get("workspace")
        .and_then(toml::Value::as_table)
        .expect("root Cargo.toml has a [workspace] table");

    let members = workspace
        .get("members")
        .and_then(toml::Value::as_array)
        .expect("[workspace.members] is an array");
    assert!(
        members.iter().any(|m| m.as_str() == Some(MEMBER_PATH)),
        "{MEMBER_PATH} must be listed in root [workspace] members so `cargo \
         clippy --workspace` / `cargo nextest run --workspace` cover it"
    );

    assert!(
        workspace.get("exclude").is_none(),
        "root [workspace] must not carry an `exclude` list re-excluding {MEMBER_PATH}"
    );

    let default_members = workspace
        .get("default-members")
        .and_then(toml::Value::as_array)
        .expect("[workspace.default-members] is an array");
    assert!(
        !default_members
            .iter()
            .any(|m| m.as_str() == Some(MEMBER_PATH)),
        "{MEMBER_PATH} must stay out of default-members so a bare `cargo \
         build`/`cargo check` on a GTK-less machine does not compile it"
    );
}

#[test]
fn proskenion_manifest_declares_no_private_workspace_table() {
    let manifest_path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let manifest: toml::Table = fs::read_to_string(manifest_path)
        .expect("proskenion Cargo.toml is readable")
        .parse()
        .expect("proskenion Cargo.toml parses as TOML");
    assert!(
        manifest.get("workspace").is_none(),
        "{MEMBER_PATH}/Cargo.toml must not declare its own [workspace] table \
         now that it is a member of the root workspace"
    );
}
