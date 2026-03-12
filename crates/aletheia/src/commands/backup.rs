//! `aletheia backup` — database backup management.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;

use aletheia_mneme::store::SessionStore;
use aletheia_taxis::oikos::Oikos;

#[derive(Debug, Clone, Args)]
pub struct BackupArgs {
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
}

pub fn run(instance_root: Option<&PathBuf>, args: &BackupArgs) -> Result<()> {
    let &BackupArgs {
        list,
        prune,
        keep,
        export_json,
    } = args;
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };

    let db_path = oikos.sessions_db();
    let store = SessionStore::open(&db_path)
        .with_context(|| format!("failed to open session store at {}", db_path.display()))?;

    let backup_dir = oikos.backups();
    let manager = aletheia_mneme::backup::BackupManager::new(store.conn(), &backup_dir);

    if list {
        let backups = manager.list_backups().context("failed to list backups")?;
        if backups.is_empty() {
            println!("No backups found.");
        } else {
            for b in &backups {
                println!("{} ({} bytes)", b.filename, b.size_bytes);
            }
        }
        return Ok(());
    }

    if prune {
        let removed = manager
            .prune_backups(keep)
            .context("failed to prune backups")?;
        println!("Pruned {removed} backup(s), kept {keep}.");
        return Ok(());
    }

    if export_json {
        let export_dir = oikos.archive().join("sessions");
        let result = manager
            .export_sessions_json(&export_dir)
            .context("failed to export sessions")?;
        println!(
            "Exported {} session(s) to {}",
            result.sessions_exported,
            result.output_dir.display()
        );
        return Ok(());
    }

    // Default: create a backup
    let result = manager.create_backup().context("failed to create backup")?;
    println!(
        "Backup created: {} ({} bytes, {} sessions, {} messages)",
        result.path.display(),
        result.size_bytes,
        result.sessions_count,
        result.messages_count,
    );

    Ok(())
}
