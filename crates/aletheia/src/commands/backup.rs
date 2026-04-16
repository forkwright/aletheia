//! `aletheia backup`: database backup management.
//!
//! Operates on the fjall knowledge store — the only persistent store in the
//! stack since rusqlite removal (#3446).

use std::path::PathBuf;

use clap::Args;
use snafu::prelude::*;

use crate::error::Result;

#[expect(
    clippy::struct_excessive_bools,
    reason = "CLI flags — each bool is a distinct switch"
)]
#[derive(Debug, Clone, Args)]
pub(crate) struct BackupArgs {
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

pub(crate) fn run(instance_root: Option<&PathBuf>, args: &BackupArgs) -> Result<()> {
    let &BackupArgs {
        list,
        prune,
        keep,
        json,
        yes,
    } = args;
    let oikos = super::resolve_oikos(instance_root)?;

    run_fjall(&oikos, list, prune, keep, json, yes)
}

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
