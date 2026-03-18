//! `aletheia backup`: database backup management.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;

use aletheia_mneme::store::SessionStore;

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
}

pub(crate) fn run(instance_root: Option<&PathBuf>, args: &BackupArgs) -> Result<()> {
    let &BackupArgs {
        list,
        prune,
        keep,
        export_json,
        json,
        yes,
    } = args;
    let oikos = super::resolve_oikos(instance_root)?;

    let db_path = oikos.sessions_db();
    let store = SessionStore::open(&db_path)
        .with_context(|| format!("failed to open session store at {}", db_path.display()))?;

    let backup_dir = oikos.backups();
    let manager = aletheia_mneme::backup::BackupManager::new(store.conn(), &backup_dir);

    if list {
        let backups = manager.list_backups().context("failed to list backups")?;
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
            println!("{}", serde_json::to_string_pretty(&items)?);
        } else if backups.is_empty() {
            println!("No backups found.");
        } else {
            for b in &backups {
                println!("{} ({} bytes)", b.filename, b.size_bytes);
            }
        }
        return Ok(());
    }

    if prune {
        let backups = manager.list_backups().context("failed to list backups")?;
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
            std::io::Write::flush(&mut std::io::stdout()).context("failed to flush stdout")?;

            let mut input = String::new();
            std::io::BufRead::read_line(&mut std::io::stdin().lock(), &mut input)
                .context("failed to read confirmation")?;

            if !input.trim().eq_ignore_ascii_case("y") {
                println!("Aborted.");
                return Ok(());
            }
        }

        let removed = manager
            .prune_backups(keep)
            .context("failed to prune backups")?;
        println!("Pruned {removed} backup(s), kept {keep}.");
        return Ok(());
    }

    if export_json {
        let export_dir = oikos.archive().join("sessions");
        match manager
            .export_sessions_json(&export_dir)
            .context("failed to export sessions")?
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

    match manager.create_backup().context("failed to create backup")? {
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
