//! `aletheia backup`: whole-instance backup management.
//!
//! Operates on the instance backup set covering `knowledge.fjall`,
//! `sessions.db`, configuration, and workspace data. The legacy fjall-only
//! `backup verify <path>` path is still supported for existing backups.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use snafu::prelude::*;

use crate::error::Result;

// ── CLI ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum BackupAction {
    /// Create a new whole-instance backup (default when no subcommand is given)
    Create,
    /// List available whole-instance backups
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Prune old whole-instance backups
    Prune {
        /// Number of backups to keep. Defaults to the configured
        /// `maintenance.backup.backup_retention_count`. (#5136)
        #[arg(long)]
        keep: Option<usize>,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Verify a backup directory (instance set or legacy fjall snapshot)
    Verify {
        /// Path to the backup directory
        path: PathBuf,
    },
    /// Restore a whole-instance backup set into the current instance
    Restore {
        /// Backup set path or name under instance/data/backups/instance
        backup_set: PathBuf,
        /// Restore while the service may be live. Unsafe: writers can race restore.
        #[arg(long)]
        force_live: bool,
        /// Restore only matching manifest entry names, backup paths, or target paths
        #[arg(long = "include")]
        include: Vec<String>,
        /// Exclude matching manifest entry names, backup paths, or target paths
        #[arg(long = "exclude")]
        exclude: Vec<String>,
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
    /// Number of backups to keep when pruning. Defaults to the configured
    /// `maintenance.backup.backup_retention_count`. (#5136)
    #[arg(long)]
    pub keep: Option<usize>,
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
        Some(BackupAction::Restore {
            backup_set,
            force_live,
            include,
            exclude,
        }) => {
            let oikos = super::resolve_oikos(instance_root)?;
            run_restore(&oikos, backup_set, *force_live, include, exclude)
        }
        Some(BackupAction::List { json }) => {
            let oikos = super::resolve_oikos(instance_root)?;
            let keep = configured_retention_count(&oikos);
            run_instance(&oikos, true, false, keep, *json, false)
        }
        Some(BackupAction::Prune { keep, yes }) => {
            let oikos = super::resolve_oikos(instance_root)?;
            let keep = keep.unwrap_or_else(|| configured_retention_count(&oikos));
            run_instance(&oikos, false, true, keep, false, *yes)
        }
        Some(BackupAction::Create) => {
            let oikos = super::resolve_oikos(instance_root)?;
            let keep = configured_retention_count(&oikos);
            run_instance(&oikos, false, false, keep, false, false)
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
            let keep = keep.unwrap_or_else(|| configured_retention_count(&oikos));
            run_instance(&oikos, list, prune, keep, json, yes)
        }
    }
}

/// Resolve the configured whole-instance backup retention count. (#5136)
///
/// Falls back to the [`BackupSettings`](taxis::config::BackupSettings) default
/// (7) when the config cannot be loaded, so the CLI never silently keeps the
/// old hard-coded value of 5.
fn configured_retention_count(oikos: &taxis::oikos::Oikos) -> usize {
    taxis::loader::load_config(oikos).map_or_else(
        |_| taxis::config::BackupSettings::default().backup_retention_count,
        |config| config.maintenance.backup.backup_retention_count,
    )
}

// ── Verify ─────────────────────────────────────────────────────────────────

fn run_verify(path: &Path) -> Result<()> {
    if !path.exists() {
        whatever!("backup path does not exist: {}", path.display());
    }
    if !path.is_dir() {
        whatever!("backup path is not a directory: {}", path.display());
    }

    // Prefer whole-instance backup verification when a manifest is present.
    if path.join("manifest.json").is_file() {
        return run_verify_instance(path);
    }

    // Fall back to legacy fjall-only verification.
    run_verify_fjall(path)
}

pub(crate) fn verify_backup(path: &Path) -> Result<oikonomos::maintenance::FjallVerifyResult> {
    use oikonomos::maintenance::FjallBackup;

    FjallBackup::verify_store(path)
        .map_err(|e| crate::error::Error::msg(format!("failed to verify backup: {e}")))
}

fn run_verify_instance(path: &Path) -> Result<()> {
    use oikonomos::maintenance::InstanceBackup;

    let result = InstanceBackup::verify_backup(path)
        .map_err(|e| crate::error::Error::msg(format!("failed to verify backup set: {e}")))?;

    println!("Backup set: {}", path.display());
    println!();
    println!("{:<24} {:>12}", "Store", "Keys / Bytes");
    println!("{}", "-".repeat(38));
    for (name, outcome) in &result.store_results {
        match outcome {
            Ok(n) => println!("{name:<24} {n:>12}"),
            Err(e) => println!("{name:<24} {:>12}", format!("FAIL: {e}")),
        }
    }
    println!("{}", "-".repeat(38));
    println!("{:<24} {:>12}", "Total keys", result.total_keys);
    println!();

    if let Some(err) = &result.first_error {
        println!("Status: FAIL");
        println!("First error: {err}");
        whatever!("backup verification failed");
    }

    println!("Status: PASS");
    Ok(())
}

fn run_verify_fjall(path: &Path) -> Result<()> {
    use oikonomos::maintenance::FjallBackup;

    let result = FjallBackup::verify_store(path)
        .map_err(|e| crate::error::Error::msg(format!("failed to verify backup: {e}")))?;

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

fn run_restore(
    oikos: &taxis::oikos::Oikos,
    backup_set: &Path,
    force_live: bool,
    include: &[String],
    exclude: &[String],
) -> Result<()> {
    use oikonomos::maintenance::{InstanceBackup, InstanceBackupConfig, InstanceRestoreOptions};

    let backup_path = resolve_backup_set_path(oikos, backup_set);
    let config = InstanceBackupConfig {
        enabled: true,
        instance_root: oikos.root().to_path_buf(),
        backup_dir: oikos.backups().join("instance"),
        interval_hours: 24,
        retention_count: configured_retention_count(oikos),
        additional_workspaces: Vec::new(),
    };
    let manager = InstanceBackup::new(config);
    let report = manager
        .restore_backup(&InstanceRestoreOptions {
            backup_path: backup_path.clone(),
            force_live,
            include: include.to_vec(),
            exclude: exclude.to_vec(),
        })
        .map_err(|e| crate::error::Error::msg(format!("failed to restore backup set: {e}")))?;

    println!(
        "Whole-instance backup restored: {} ({} entries, {} bytes, {} live entries replaced)",
        report.backup_path.display(),
        report.entries_restored,
        report.bytes_copied,
        report.live_entries_replaced,
    );
    if report.entries_skipped > 0 {
        println!("Skipped {} manifest entries.", report.entries_skipped);
    }
    Ok(())
}

fn resolve_backup_set_path(oikos: &taxis::oikos::Oikos, backup_set: &Path) -> PathBuf {
    if backup_set.exists() {
        return backup_set.to_path_buf();
    }
    if backup_set.components().count() == 1 {
        let named = oikos.backups().join("instance").join(backup_set);
        if named.exists() {
            return named;
        }
    }
    backup_set.to_path_buf()
}

// ── Instance backup operations ─────────────────────────────────────────────

#[expect(
    clippy::fn_params_excessive_bools,
    reason = "1:1 pass-through of CLI flags from clap; grouping into a struct adds no clarity"
)]
fn run_instance(
    oikos: &taxis::oikos::Oikos,
    list: bool,
    prune: bool,
    keep: usize,
    json: bool,
    yes: bool,
) -> Result<()> {
    use oikonomos::maintenance::{InstanceBackup, InstanceBackupConfig};

    let config = InstanceBackupConfig {
        enabled: true,
        instance_root: oikos.root().to_path_buf(),
        backup_dir: oikos.backups().join("instance"),
        interval_hours: 24,
        retention_count: keep,
        additional_workspaces: Vec::new(),
    };
    let manager = InstanceBackup::new(config);

    if list {
        let backups = manager
            .list_backups()
            .whatever_context("failed to list instance backups")?;
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
            println!("No instance backups found.");
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
            .whatever_context("failed to list instance backups")?;
        let to_remove: Vec<_> = backups.iter().skip(keep).collect();
        if to_remove.is_empty() {
            println!(
                "Nothing to prune: {} instance backup(s) found, keeping {keep}.",
                backups.len()
            );
            return Ok(());
        }
        if !yes {
            println!("The following instance backup(s) will be deleted:");
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
        println!("Pruned instance backups, kept {keep}.");
        return Ok(());
    }

    // Default: create a new whole-instance backup.
    let report = manager
        .create_backup()
        .whatever_context("failed to create whole-instance backup")?;
    match report.backup_path {
        Some(path) => println!(
            "Whole-instance backup created: {} ({} files, {} bytes)",
            path.display(),
            report.files_copied,
            report.bytes_copied,
        ),
        None => println!("Whole-instance backup skipped: required directories not found."),
    }
    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn make_fjall_store(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        let db = fjall::SingleWriterTxDatabase::builder(path).open().unwrap();
        let _ = db
            .keyspace("test", fjall::KeyspaceCreateOptions::default)
            .unwrap();
        drop(db);
    }

    fn write_text_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut file = std::fs::File::create(path).unwrap();
        file.write_all(contents.as_bytes()).unwrap();
    }

    #[test]
    fn verify_fjall_empty_db_passes() {
        let tmp = tempfile::tempdir().unwrap();
        make_fjall_store(tmp.path());

        let result = oikonomos::maintenance::FjallBackup::verify_store(tmp.path()).unwrap();
        assert_eq!(result.total_keys, 0);
        assert!(result.first_error.is_none());
    }

    #[test]
    fn verify_fjall_rejects_non_fjall_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let msg = match oikonomos::maintenance::FjallBackup::verify_store(tmp.path()) {
            Ok(_) => panic!("expected rejection of non-fjall dir"),
            Err(e) => e.to_string(),
        };
        assert!(msg.contains("not a fjall store"), "unexpected error: {msg}");
        // The pre-check must not create any fjall scaffolding.
        assert!(!tmp.path().join("version").exists());
        assert!(!tmp.path().join("keyspaces").exists());
    }

    #[test]
    fn verify_instance_backup_rejects_missing_sessions() {
        use oikonomos::maintenance::{BackupManifest, InstanceBackup, StoreEntry};

        let tmp = tempfile::tempdir().unwrap();
        let backup_path = tmp.path().join("bad-backup");
        std::fs::create_dir_all(&backup_path).unwrap();

        let manifest = BackupManifest {
            version: String::from("aletheia-instance-backup-v1"),
            created_at: jiff::Zoned::now().to_string(),
            source_root: tmp.path().join("instance"),
            stores: vec![StoreEntry {
                name: String::from("knowledge.fjall"),
                source_path: tmp
                    .path()
                    .join("instance")
                    .join("data")
                    .join("knowledge.fjall"),
                backup_path: PathBuf::from("stores/knowledge.fjall"),
                snapshot_time: jiff::Zoned::now().to_string(),
                byte_count: 0,
                status: String::from("ok"),
                agent_id: None,
                workspace_source_class: None,
                exclusion_reason: None,
                sha256: None,
            }],
            optional_stores: Vec::new(),
            workspace_omissions: Vec::new(),
            total_bytes: 0,
            snapshot_epoch: jiff::Zoned::now().to_string(),
            snapshot_protocol_version: String::from("aletheia-instance-backup-v1-snapshot-1"),
            quiesced: false,
            store_generations: std::collections::HashMap::new(),
        };
        write_text_file(
            &backup_path.join("manifest.json"),
            &serde_json::to_string_pretty(&manifest).unwrap(),
        );
        make_fjall_store(&backup_path.join("stores").join("knowledge.fjall"));

        let result = InstanceBackup::verify_backup(&backup_path).unwrap();
        assert!(result.first_error.is_some());
        let err = result.first_error.unwrap();
        assert!(
            err.contains("sessions.db"),
            "error should mention sessions.db: {err}"
        );
    }

    #[test]
    fn run_verify_nonexistent_path_fails() {
        let result = run_verify(Path::new("/tmp/nonexistent-instance-backup-xyz"));
        assert!(result.is_err());
    }
}
