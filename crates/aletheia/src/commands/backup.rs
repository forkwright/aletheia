//! `aletheia backup`: database backup management.
//!
//! Operates on the fjall knowledge store. Session/auth storage also uses fjall;
//! `rusqlite` remains only in the legacy one-shot sessions migrator.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use fjall::Readable;
use snafu::prelude::*;

use crate::error::Result;

// ── CLI ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum BackupAction {
    /// Create a new backup (default when no subcommand is given)
    Create,
    /// List available backups
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Prune old backups
    Prune {
        /// Number of backups to keep
        #[arg(long, default_value_t = 5)]
        keep: usize,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Verify a backup directory
    Verify {
        /// Path to the backup directory
        path: PathBuf,
    },
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "CLI flags — each bool is a distinct switch"
)]
#[derive(Debug, Clone, Args)]
pub(crate) struct BackupArgs {
    #[command(subcommand)]
    pub action: Option<BackupAction>,

    // Legacy flags (used when no subcommand is given)
    /// List available backups
    #[arg(long)]
    pub list: bool,
    /// Prune old backups
    #[arg(long)]
    pub prune: bool,
    /// Number of backups to keep when pruning
    #[arg(long, default_value_t = 5)]
    pub keep: usize,
    /// Output as JSON (for --list)
    #[arg(long)]
    pub json: bool,
    /// Skip confirmation prompt when pruning
    #[arg(long)]
    pub yes: bool,
}

// ── Dispatch ───────────────────────────────────────────────────────────────

pub(crate) fn run(instance_root: Option<&PathBuf>, args: &BackupArgs) -> Result<()> {
    match &args.action {
        Some(BackupAction::Verify { path }) => run_verify(path),
        Some(BackupAction::List { json }) => {
            let oikos = super::resolve_oikos(instance_root)?;
            run_fjall(&oikos, true, false, 5, *json, false)
        }
        Some(BackupAction::Prune { keep, yes }) => {
            let oikos = super::resolve_oikos(instance_root)?;
            run_fjall(&oikos, false, true, *keep, false, *yes)
        }
        Some(BackupAction::Create) => {
            let oikos = super::resolve_oikos(instance_root)?;
            run_fjall(&oikos, false, false, 5, false, false)
        }
        None => {
            let oikos = super::resolve_oikos(instance_root)?;
            let &BackupArgs {
                list,
                prune,
                keep,
                json,
                yes,
                ..
            } = args;
            run_fjall(&oikos, list, prune, keep, json, yes)
        }
    }
}

// ── Verify ─────────────────────────────────────────────────────────────────

/// Result of verifying a single backup directory.
pub(crate) struct VerifyResult {
    pub(crate) partition_counts: Vec<(String, usize)>,
    pub(crate) first_error: Option<String>,
    pub(crate) total_keys: usize,
}

fn run_verify(path: &Path) -> Result<()> {
    if !path.exists() {
        whatever!("backup path does not exist: {}", path.display());
    }
    if !path.is_dir() {
        whatever!("backup path is not a directory: {}", path.display());
    }

    let result = verify_backup(path)?;

    // Summary to stdout
    println!("Backup: {}", path.display());
    println!();
    println!("{:<24} {:>10}", "Partition", "Keys");
    println!("{}", "-".repeat(36));
    for (name, count) in &result.partition_counts {
        println!("{name:<24} {count:>10}");
    }
    println!("{}", "-".repeat(36));
    println!("{:<24} {:>10}", "Total", result.total_keys);
    println!();

    if let Some(err) = &result.first_error {
        println!("Status: FAIL");
        println!("First error: {err}");
        whatever!("backup verification failed");
    }

    println!("Status: PASS");
    Ok(())
}

pub(crate) fn verify_backup(path: &Path) -> Result<VerifyResult> {
    let fdb = koina::fjall::FjallDb::open_existing(path)
        .map_err(|e| crate::error::Error::msg(format!("failed to open backup: {e}")))?;

    let mut result = VerifyResult {
        partition_counts: Vec::new(),
        first_error: None,
        total_keys: 0,
    };

    let names = fdb.db.list_keyspace_names();
    for name in names {
        let name_str = name.as_ref();
        let ks = fdb
            .db
            .keyspace(name_str, fjall::KeyspaceCreateOptions::default)
            .map_err(|e| {
                crate::error::Error::msg(format!("failed to open partition {name_str}: {e}"))
            })?;

        let snap = fdb.db.read_tx();
        let mut count = 0usize;

        for guard in snap.range::<&str, _>(&ks, ..) {
            let (key, value) = guard
                .into_inner()
                .map_err(|e| crate::error::Error::msg(format!("read error in {name_str}: {e}")))?;

            count += 1;
            result.total_keys += 1;

            if result.first_error.is_none()
                && let Err(e) = validate_kv(name_str, key.as_ref(), value.as_ref())
            {
                let key_display = String::from_utf8_lossy(key.as_ref());
                result.first_error = Some(format!("{name_str}/{key_display}: {e}"));
            }
        }

        result.partition_counts.push((name_str.to_owned(), count));
    }

    Ok(result)
}

// ── Per-partition validation ───────────────────────────────────────────────

fn validate_kv(partition: &str, key: &[u8], value: &[u8]) -> std::result::Result<(), String> {
    match partition {
        "sessions" => validate_sessions(key, value),
        "messages" => validate_messages(key, value),
        "usage" => serde_json::from_slice::<mneme::types::UsageRecord>(value)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "distillations" | "ops:tasks" => validate_json(value),
        "notes" => validate_notes(key, value),
        "blackboard" => serde_json::from_slice::<mneme::types::BlackboardRow>(value)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "counters" => validate_u64(value),
        "users" => validate_users(key, value),
        "api_keys" => validate_api_keys(key, value),
        "revoked_tokens" => validate_utf8(value),
        // Known partitions with opaque/internal encoding, plus unknown partitions:
        // all verified by successful read (iteration implicitly verifies checksums).
        other => validate_opaque_or_unknown_partition(other),
    }
}

fn validate_opaque_or_unknown_partition(partition: &str) -> std::result::Result<(), String> {
    if partition.is_empty() {
        return Err("partition name must not be empty".into());
    }
    Ok(())
}

fn validate_sessions(key: &[u8], value: &[u8]) -> std::result::Result<(), String> {
    let key_str = std::str::from_utf8(key).map_err(|e| e.to_string())?;
    if key_str.starts_with("idx:nous:") {
        if !value.is_empty() {
            return Err("session nous index value should be empty".into());
        }
        Ok(())
    } else if key_str.starts_with("idx:key:") {
        std::str::from_utf8(value).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        serde_json::from_slice::<mneme::types::Session>(value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

fn validate_messages(key: &[u8], value: &[u8]) -> std::result::Result<(), String> {
    let key_str = std::str::from_utf8(key).map_err(|e| e.to_string())?;
    if key_str.starts_with("next_seq:") {
        if value.len() != 8 {
            return Err(format!(
                "next_seq value should be 8 bytes, got {}",
                value.len()
            ));
        }
        Ok(())
    } else if key_str.starts_with("distilled:") {
        if value != b"1" {
            return Err("distilled flag should be \"1\"".into());
        }
        Ok(())
    } else {
        serde_json::from_slice::<mneme::types::Message>(value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

fn validate_notes(key: &[u8], value: &[u8]) -> std::result::Result<(), String> {
    let key_str = std::str::from_utf8(key).map_err(|e| e.to_string())?;
    if key_str.starts_with("gid:") {
        std::str::from_utf8(value).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        serde_json::from_slice::<mneme::types::AgentNote>(value)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

fn validate_users(key: &[u8], value: &[u8]) -> std::result::Result<(), String> {
    let key_str = std::str::from_utf8(key).map_err(|e| e.to_string())?;
    if !key_str.starts_with("user:") {
        return Err(format!(
            "users key should start with 'user:', got {key_str}"
        ));
    }
    validate_json(value)
}

fn validate_api_keys(key: &[u8], value: &[u8]) -> std::result::Result<(), String> {
    let key_str = std::str::from_utf8(key).map_err(|e| e.to_string())?;
    if key_str.starts_with("hash:") {
        std::str::from_utf8(value).map_err(|e| e.to_string())?;
        Ok(())
    } else {
        validate_json(value)
    }
}

fn validate_u64(value: &[u8]) -> std::result::Result<(), String> {
    if value.len() != 8 {
        return Err(format!("u64 value should be 8 bytes, got {}", value.len()));
    }
    Ok(())
}

fn validate_json(value: &[u8]) -> std::result::Result<(), String> {
    serde_json::from_slice::<serde_json::Value>(value)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn validate_utf8(value: &[u8]) -> std::result::Result<(), String> {
    std::str::from_utf8(value)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// ── Legacy backup operations ───────────────────────────────────────────────

/// Handle fjall knowledge store backup operations.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "1:1 pass-through of CLI flags from clap; grouping into a struct adds no clarity"
)]
fn run_fjall(
    oikos: &taxis::oikos::Oikos,
    list: bool,
    prune: bool,
    keep: usize,
    json: bool,
    yes: bool,
) -> Result<()> {
    use oikonomos::maintenance::{FjallBackup, FjallBackupConfig};

    let config = FjallBackupConfig {
        enabled: true,
        source_dir: oikos.knowledge_db(),
        backup_dir: oikos.backups().join("fjall"),
        interval_hours: 24,
        retention_count: keep,
    };
    let manager = FjallBackup::new(config);

    if list {
        let backups = manager
            .list_backups()
            .whatever_context("failed to list fjall backups")?;
        if json {
            let items: Vec<serde_json::Value> = backups
                .iter()
                .map(|b| {
                    serde_json::json!({
                        "name": b.name,
                        "size_bytes": b.size_bytes,
                        "path": b.path.to_string_lossy(),
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&items)
                    .whatever_context("failed to serialize backups")?
            );
        } else if backups.is_empty() {
            println!("No fjall backups found.");
        } else {
            for b in &backups {
                let mb = b.size_bytes / (1024 * 1024);
                println!("{} ({mb} MB)", b.name);
            }
        }
        return Ok(());
    }

    if prune {
        let backups = manager
            .list_backups()
            .whatever_context("failed to list fjall backups")?;
        let to_remove: Vec<_> = backups.iter().skip(keep).collect();
        if to_remove.is_empty() {
            println!(
                "Nothing to prune: {} fjall backup(s) found, keeping {keep}.",
                backups.len()
            );
            return Ok(());
        }
        if !yes {
            println!("The following fjall backup(s) will be deleted:");
            for b in &to_remove {
                println!("  {} ({} bytes)", b.name, b.size_bytes);
            }
            print!("Proceed? [y/N] ");
            std::io::Write::flush(&mut std::io::stdout())
                .whatever_context("failed to flush stdout")?;
            let mut input = String::new();
            std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut input)
                .whatever_context("failed to read confirmation")?;
            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Aborted.");
                return Ok(());
            }
        }
        for entry in to_remove {
            std::fs::remove_dir_all(&entry.path).whatever_context("failed to remove backup")?;
        }
        println!("Pruned fjall backups, kept {keep}.");
        return Ok(());
    }

    // Default: create a new fjall backup.
    let report = manager
        .create_backup()
        .whatever_context("failed to create fjall backup")?;
    match report.backup_path {
        Some(path) => println!(
            "Fjall backup created: {} ({} files, {} bytes)",
            path.display(),
            report.files_copied,
            report.bytes_copied,
        ),
        None => println!("Fjall backup skipped: knowledge store directory not found."),
    }
    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn verify_backup_empty_db_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let db = fjall::SingleWriterTxDatabase::builder(tmp.path())
            .open()
            .unwrap();
        let _ = db
            .keyspace("test", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        drop(db);

        let result = verify_backup(tmp.path()).unwrap();
        assert_eq!(result.total_keys, 0);
        assert!(result.first_error.is_none());
    }

    #[test]
    fn verify_backup_with_data_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let db = fjall::SingleWriterTxDatabase::builder(tmp.path())
            .open()
            .unwrap();
        let ks = db
            .keyspace("sessions", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        let session = mneme::types::Session {
            id: "sess-1".into(),
            nous_id: "syn".into(),
            session_key: "default".into(),
            status: mneme::types::SessionStatus::Active,
            model: None,
            session_type: mneme::types::SessionType::Primary,
            created_at: "2024-01-01T00:00:00.000Z".into(),
            updated_at: "2024-01-01T00:00:00.000Z".into(),
            metrics: mneme::types::SessionMetrics {
                token_count_estimate: 0,
                message_count: 0,
                last_input_tokens: 0,
                bootstrap_hash: None,
                distillation_count: 0,
                last_distilled_at: None,
                computed_context_tokens: 0,
            },
            origin: mneme::types::SessionOrigin {
                parent_session_id: None,
                thread_id: None,
                transport: None,
                display_name: None,
            },
            artefact_meta: None,
        };
        ks.insert("sess-1", serde_json::to_vec(&session).unwrap().as_slice())
            .unwrap();
        drop(db);

        // WHY: fjall holds a file lock even after drop; verify a copy instead.
        let verify_tmp = tempfile::tempdir().unwrap();
        copy_dir(tmp.path(), verify_tmp.path());

        let result = verify_backup(verify_tmp.path()).unwrap();
        assert_eq!(result.total_keys, 1);
        assert!(result.first_error.is_none());
        assert_eq!(result.partition_counts.len(), 1);
        let first = result.partition_counts.first().unwrap();
        assert_eq!(first.0, "sessions");
        assert_eq!(first.1, 1);
    }

    #[test]
    fn verify_backup_detects_bad_json() {
        let tmp = tempfile::tempdir().unwrap();
        let db = fjall::SingleWriterTxDatabase::builder(tmp.path())
            .open()
            .unwrap();
        let ks = db
            .keyspace("sessions", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        ks.insert("bad-key", b"not json").unwrap();
        drop(db);

        // WHY: fjall holds a file lock even after drop; verify a copy instead.
        let verify_tmp = tempfile::tempdir().unwrap();
        copy_dir(tmp.path(), verify_tmp.path());

        let result = verify_backup(verify_tmp.path()).unwrap();
        assert_eq!(result.total_keys, 1);
        assert!(result.first_error.is_some());
        let err = result.first_error.unwrap();
        assert!(err.contains("bad-key"), "error should mention key: {err}");
    }

    fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if src_path.is_dir() {
                copy_dir(&src_path, &dst_path);
            } else {
                std::fs::copy(&src_path, &dst_path).unwrap();
            }
        }
    }

    #[test]
    fn verify_backup_nonexistent_path_fails() {
        let result = run_verify(Path::new("/tmp/nonexistent-fjall-backup-xyz"));
        assert!(result.is_err());
    }
}
