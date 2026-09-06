//! `aletheia session-store`: schema-manifest inspection and legacy attestation.
//!
//! Operates directly on the on-disk session store, bypassing the HTTP API —
//! the same manifest primitives every `SessionStore::open` call site relies
//! on (issue #5031). `verify` never mutates and is safe to run against a
//! live store; `stamp` is the explicit, human-attested path that brings a
//! pre-manifest ("legacy") store forward, and is gated behind confirmation.
//! `tool-audit-check` is a third, also-read-only doctor query: it counts
//! `tool_audit` rows that fail to decode (aletheia#7217).

use std::path::{Path, PathBuf};

use clap::Subcommand;
use snafu::prelude::*;
use taxis::oikos::Oikos;

use mneme::store::{SchemaManifest, SessionStore};

use crate::error::Result;

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Action {
    /// Read-only schema-manifest compatibility check (does not open the store)
    Verify {
        /// Session store path (default: the instance's sessions.db)
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Attest a pre-manifest ("legacy") store as compatible with the schema
    /// this build expects, without opening the fjall keyspace.
    ///
    /// This is an attestation, not a migration: only run it once you have
    /// confirmed the store's on-disk data already matches the key layout
    /// documented in `graphe::store::fjall_store`. Refuses if a manifest is
    /// already present (whether compatible or a genuine version mismatch —
    /// stamping never overwrites an existing decision) or if the path has no
    /// existing store data at all.
    Stamp {
        /// Session store path (default: the instance's sessions.db)
        #[arg(long)]
        path: Option<PathBuf>,
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Read-only doctor check: scan the `tool_audit` partition and report
    /// how many rows decode cleanly versus how many are corrupt
    /// (aletheia#7217).
    ///
    /// Never mutates. Exits non-zero when any corrupt row is found, so it
    /// can be used as a monitoring probe. Opens the store directly
    /// (`SessionStore::open`, the same path every production call site
    /// uses) -- fjall is single-writer, so running this against a store a
    /// live `aletheia` process already has open may refuse to open rather
    /// than corrupt anything.
    ToolAuditCheck {
        /// Session store path (default: the instance's sessions.db)
        #[arg(long)]
        path: Option<PathBuf>,
    },
}

pub(crate) fn run(action: Action, instance_root: Option<&PathBuf>) -> Result<()> {
    match action {
        Action::Verify { path } => run_verify(&resolve_path(instance_root, path.as_deref())),
        Action::Stamp { path, yes } => {
            run_stamp(&resolve_path(instance_root, path.as_deref()), yes)
        }
        Action::ToolAuditCheck { path } => {
            run_tool_audit_check(&resolve_path(instance_root, path.as_deref()))
        }
    }
}

/// `--path` always wins; otherwise the store lives where every production
/// call site opens it — [`Oikos::sessions_db`] under the resolved instance.
fn resolve_path(instance_root: Option<&PathBuf>, path: Option<&Path>) -> PathBuf {
    if let Some(path) = path {
        return path.to_path_buf();
    }
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    oikos.sessions_db()
}

fn run_verify(path: &Path) -> Result<()> {
    // WHY: `with_whatever_context`, not `whatever_context` — the graphe
    // refusal text (which names the exact `aletheia session-store stamp`
    // invocation to run) IS the operator-actionable content, and only
    // `with_whatever_context`'s closure has access to `e` to fold it into
    // `Display`. `whatever_context` would replace it with a static string.
    let manifest = SessionStore::verify_schema_manifest(path)
        .with_whatever_context(|e| format!("schema manifest verification failed: {e}"))?;
    println!("Session store: {}", path.display());
    println!("Status:        compatible");
    print_manifest(&manifest);
    Ok(())
}

fn run_stamp(path: &Path, yes: bool) -> Result<()> {
    println!("Session store: {}", path.display());
    println!(
        "This attests that the on-disk data at this path already matches the schema and \
         key layout this build of aletheia expects. It is NOT a migration and will not \
         change any session data — it only writes the manifest file. Refuses if a manifest \
         is already present or if the store has no existing data."
    );
    if !yes && !confirm()? {
        println!("Aborted.");
        return Ok(());
    }

    // WHY: same reasoning as `run_verify` — keep the source refusal text
    // (e.g. "already has a schema manifest") in `Display` rather than
    // discarding it behind a static context string.
    let manifest = SessionStore::stamp_legacy_schema_manifest(path)
        .with_whatever_context(|e| format!("legacy schema-manifest stamp failed: {e}"))?;
    println!("Stamped.");
    print_manifest(&manifest);
    Ok(())
}

/// Read-only: scan the `tool_audit` partition and report how many rows
/// decode cleanly versus how many are corrupt (aletheia#7217).
///
/// Returns `Err` (non-zero exit) when any corrupt row is found, so this can
/// be used as a monitoring probe as well as an interactive check.
fn run_tool_audit_check(path: &Path) -> Result<()> {
    let store =
        SessionStore::open(path).with_whatever_context(|e| format!("failed to open store: {e}"))?;
    let scan = store
        .scan_tool_audit_records()
        .with_whatever_context(|e| format!("tool_audit scan failed: {e}"))?;

    println!("Session store: {}", path.display());
    println!("Decoded rows:  {}", scan.records.len());
    println!("Corrupt rows:  {}", scan.corrupt.len());

    if scan.corrupt.is_empty() {
        println!("Status:        clean");
        return Ok(());
    }

    println!("Status:        corrupt rows present");
    for corrupt in &scan.corrupt {
        println!("  key={} error={}", corrupt.key, corrupt.error);
    }
    Err(crate::error::Error::msg(format!(
        "{} tool_audit row(s) failed to decode; see keys above. Root cause: a row written \
         under an older schema had an `Option` field serialize as a literal JSON `null` that \
         a later, stricter schema no longer accepts on read. See aletheia#7217.",
        scan.corrupt.len()
    )))
}

fn confirm() -> Result<bool> {
    print!("Proceed? [y/N] ");
    std::io::Write::flush(&mut std::io::stdout()).whatever_context("failed to flush stdout")?;
    let mut input = String::new();
    std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut input)
        .whatever_context("failed to read confirmation")?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

fn print_manifest(manifest: &SchemaManifest) {
    println!("Store kind:         {}", manifest.store_kind);
    println!("Schema version:     {}", manifest.schema_version);
    println!("Key layout version: {}", manifest.key_layout_version);
    println!("Created:            {}", manifest.created_at);
    println!("Last verified:      {}", manifest.updated_at);
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    fn make_fresh_store(path: &Path) {
        SessionStore::open(path).unwrap();
    }

    #[test]
    fn resolve_path_prefers_explicit_flag() {
        let explicit = PathBuf::from("/tmp/explicit-store-xyz");
        let resolved = resolve_path(None, Some(explicit.as_path()));
        assert_eq!(resolved, explicit);
    }

    #[test]
    fn resolve_path_derives_from_instance_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let resolved = resolve_path(Some(&root), None);
        assert_eq!(resolved, Oikos::from_root(&root).sessions_db());
    }

    #[test]
    fn verify_reports_compatible_for_current_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions");
        make_fresh_store(&path);

        run_verify(&path).unwrap();
    }

    #[test]
    fn verify_fails_on_store_with_no_data_yet() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions");

        let result = run_verify(&path);
        assert!(result.is_err(), "verify on an empty path should fail");
    }

    #[test]
    fn verify_names_the_stamp_command_when_manifest_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions");
        make_fresh_store(&path);
        std::fs::remove_file(path.join("schema_manifest.json")).unwrap();

        let result = run_verify(&path);
        assert!(result.is_err(), "missing manifest on non-empty store fails");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("aletheia session-store stamp"),
            "refusal should name the CLI command an operator can run, got: {msg}"
        );
    }

    #[test]
    fn stamp_with_yes_attests_a_legacy_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions");
        make_fresh_store(&path);
        std::fs::remove_file(path.join("schema_manifest.json")).unwrap();

        run_stamp(&path, true).unwrap();

        // The store must now open normally.
        SessionStore::open(&path).unwrap();
    }

    #[test]
    fn stamp_refuses_when_manifest_already_present() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions");
        make_fresh_store(&path);

        let result = run_stamp(&path, true);
        assert!(result.is_err(), "stamping an already-stamped store fails");
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("already has a schema manifest"), "got: {msg}");
    }

    #[test]
    fn stamp_refuses_when_store_has_no_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions");

        let result = run_stamp(&path, true);
        assert!(result.is_err(), "stamping an empty path fails");
    }

    #[test]
    fn tool_audit_check_reports_clean_on_a_fresh_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions");
        make_fresh_store(&path);

        run_tool_audit_check(&path).unwrap();
    }

    #[test]
    fn tool_audit_check_fails_and_counts_a_corrupt_row() {
        // WHY(#7217): this is the CLI-side "how big is the backlog" doctor
        // check -- it must find and count a corrupt row, and exit non-zero
        // (an `Err` here) so it doubles as a monitoring probe.
        use mneme::store::test_support::inject_raw_tool_audit_row;
        use mneme::store::{FinalizeToolAuditRecord, FinalizeTurnRequest};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions");
        {
            let store = SessionStore::open(&path).unwrap();
            store
                .create_session("ses-1", "syn", "main", None, None)
                .unwrap();
            let audits = vec![FinalizeToolAuditRecord {
                turn_seq: 1,
                tool_call_id: "call-good",
                tool_name: "read_file",
                duration_ms: 5,
                is_error: false,
                outcome: "success",
                result: Some("ok"),
                approval: Some("auto_approved"),
                receipt: "receipt-good",
            }];
            store
                .finalize_turn(&FinalizeTurnRequest {
                    session_id: "ses-1",
                    nous_id: "syn",
                    session_key: "main",
                    model: None,
                    parent_session_id: None,
                    messages: &[],
                    usage: None,
                    tool_audit_records: &audits,
                    completion_note: None,
                    turn_record: None,
                })
                .unwrap();
            // `store` drops here, releasing the fjall lock, before the raw
            // injection below opens its own handle on the same path.
        }
        inject_raw_tool_audit_row(
            &path,
            "00000000000000099999",
            br#"{"id":99999,"session_id":"ses-1","nous_id":"syn","turn_seq":1,
                 "tool_call_id":"call-corrupt","tool_name":null,"duration_ms":1,
                 "is_error":false,"outcome":"error","result":null,"approval":null,
                 "receipt":"","created_at":"2026-09-06T00:00:00.000Z"}"#,
        )
        .unwrap();

        let result = run_tool_audit_check(&path);
        assert!(result.is_err(), "a corrupt row must exit non-zero");
        let msg = result.unwrap_err().to_string();
        assert!(
            msg.contains("1 tool_audit row(s) failed to decode"),
            "message should name the corrupt count, got: {msg}"
        );
        assert!(
            msg.contains("aletheia#7217"),
            "message should cite the issue, got: {msg}"
        );
    }
}
