//! `aletheia backup`: database backup management.

use std::path::PathBuf;

use clap::Args;
use snafu::prelude::*;

use mneme::store::SessionStore;

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
    /// Export sessions as JSON
    #[arg(long)]
    pub export_json: bool,
    /// Output as JSON (for --list)
    #[arg(long)]
    pub json: bool,
    /// Skip confirmation prompt when pruning
    #[arg(long)]
    pub yes: bool,
    /// Operate on fjall knowledge store instead of `SQLite` session store
    #[arg(long)]
    pub fjall: bool,
}

pub(crate) fn run(instance_root: Option<&PathBuf>, args: &BackupArgs) -> Result<()> {
    let &BackupArgs {
        list,
        prune,
        keep,
        export_json,
        json,
        yes,
        fjall,
    } = args;
    let oikos = super::resolve_oikos(instance_root)?;

    if fjall {
        return run_fjall(&oikos, list, prune, keep, json, yes);
    }

    let db_path = oikos.sessions_db();
    let store = SessionStore::open(&db_path).with_whatever_context(|_| {
        format!("failed to open session store at {}", db_path.display())
    })?;

    let backup_dir = oikos.backups();
    let manager = mneme::backup::BackupManager::new(store.conn(), &backup_dir);

    if list {
        return run_list(&manager, json);
    }
    if prune {
        return run_prune(&manager, keep, yes);
    }
    if export_json {
        let export_dir = oikos.archive().join("sessions");
        match manager
            .export_sessions_json(&export_dir)
            .whatever_context("failed to export sessions")?
        {
            Some(result) => println!(
                "Exported {} session(s) to {}",
                result.sessions_exported,
                result.output_dir.display()
            ),
            None => println!("Export skipped: disk space critical."),
        }
        return Ok(());
    }

    match manager
        .create_backup()
        .whatever_context("failed to create backup")?
    {
        Some(result) => println!(
            "Backup created: {} ({} bytes, {} sessions, {} messages)",
            result.path.display(),
            result.size_bytes,
            result.sessions_count,
            result.messages_count,
        ),
        None => println!("Backup skipped: disk space critical."),
    }

    Ok(())
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

fn run_list(manager: &mneme::backup::BackupManager<'_>, json: bool) -> Result<()> {
    let backups = manager
        .list_backups()
        .whatever_context("failed to list backups")?;
    if json {
        let items: Vec<serde_json::Value> = backups
            .iter()
            .map(|b| {
                serde_json::json!({
                    "filename": b.filename,
                    "size_bytes": b.size_bytes,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&items).whatever_context("failed to serialize backups")?
        );
    } else if backups.is_empty() {
        println!("No backups found.");
    } else {
        for b in &backups {
            println!("{} ({} bytes)", b.filename, b.size_bytes);
        }
    }
    Ok(())
}

fn run_prune(manager: &mneme::backup::BackupManager<'_>, keep: usize, yes: bool) -> Result<()> {
    let backups = manager
        .list_backups()
        .whatever_context("failed to list backups")?;
    let to_remove: Vec<_> = backups.iter().skip(keep).collect();

    if to_remove.is_empty() {
        println!(
            "Nothing to prune: {} backup(s) found, keeping {keep}.",
            backups.len()
        );
        return Ok(());
    }

    if !yes {
        println!("The following backup(s) will be deleted:");
        for b in &to_remove {
            println!("  {} ({} bytes)", b.filename, b.size_bytes);
        }
        print!("Proceed? [y/N] ");
        std::io::Write::flush(&mut std::io::stdout()).whatever_context("failed to flush stdout")?;

        let mut input = String::new();
        std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut input)
            .whatever_context("failed to read confirmation")?;

        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    let removed = manager
        .prune_backups(keep)
        .whatever_context("failed to prune backups")?;
    println!("Pruned {removed} backup(s), kept {keep}.");
    Ok(())
}
