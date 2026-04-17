//! `aletheia prompt-audit`: operator-visible record of outbound LLM requests (#3411).
//!
//! The audit log lives at `{instance}/logs/prompt-audit/YYYY-MM-DD.jsonl` as
//! append-only JSONL. These commands let operators inspect that log without
//! writing JSON-parsing one-liners.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use snafu::prelude::*;

use nous::audit::PromptAuditRecord;
use taxis::loader::load_config;
use taxis::oikos::Oikos;

use crate::error::Result;

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Action {
    /// List audit records (newest first)
    List {
        /// Only show records at or after this date (YYYY-MM-DD).
        #[arg(long)]
        since: Option<String>,
        /// Only show records for this nous id.
        #[arg(long)]
        nous: Option<String>,
        /// Maximum records to print. Default: 50.
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Emit JSON instead of the human-readable table.
        #[arg(long)]
        json: bool,
    },
    /// Show the full record for a specific turn timestamp
    Show {
        /// Record timestamp (RFC3339, e.g. 2026-04-16T12:00:00Z).
        timestamp: String,
    },
}

pub(crate) fn run(action: Action, instance_root: Option<&PathBuf>) -> Result<()> {
    let oikos = match instance_root {
        Some(root) => Oikos::from_root(root),
        None => Oikos::discover(),
    };
    let config = load_config(&oikos).whatever_context("failed to load config")?;
    let log_dir = config
        .prompt_audit
        .log_dir
        .clone()
        .unwrap_or_else(|| oikos.logs().join("prompt-audit"));

    match action {
        Action::List {
            since,
            nous,
            limit,
            json,
        } => list_records(&log_dir, since.as_deref(), nous.as_deref(), limit, json),
        Action::Show { timestamp } => show_record(&log_dir, &timestamp),
    }
}

fn list_records(
    log_dir: &Path,
    since: Option<&str>,
    nous: Option<&str>,
    limit: usize,
    json: bool,
) -> Result<()> {
    if !log_dir.exists() {
        println!("no audit log directory at {}", log_dir.display());
        return Ok(());
    }

    let mut files: Vec<_> = std::fs::read_dir(log_dir)
        .whatever_context("failed to read audit log directory")?
        .filter_map(std::result::Result::ok)
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .is_some_and(|s| s == "jsonl")
        })
        .collect();
    // WHY: sort descending by filename (YYYY-MM-DD). Newest day first.
    files.sort_by_key(|e| std::cmp::Reverse(e.file_name()));

    if let Some(since) = since {
        files.retain(|e| {
            e.file_name()
                .to_str()
                .and_then(|n| n.strip_suffix(".jsonl"))
                .is_some_and(|d| d >= since)
        });
    }

    let mut records: Vec<PromptAuditRecord> = Vec::new();
    for entry in files {
        let content = std::fs::read_to_string(entry.path())
            .whatever_context("failed to read audit log file")?;
        // WHY: iterate lines in reverse so newest-within-day comes first.
        for line in content.lines().rev() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<PromptAuditRecord>(line) {
                Ok(rec) => {
                    if let Some(id) = nous
                        && rec.nous_id != id
                    {
                        continue;
                    }
                    records.push(rec);
                    if records.len() >= limit {
                        break;
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "skipping malformed audit record");
                }
            }
        }
        if records.len() >= limit {
            break;
        }
    }

    if json {
        let out = serde_json::to_string_pretty(&records)
            .whatever_context("failed to serialize records")?;
        println!("{out}");
    } else if records.is_empty() {
        println!("no audit records matched");
    } else {
        println!(
            "{:<26} {:<10} {:<12} {:<32} {:<6} {:>7}",
            "timestamp", "nous", "provider", "model", "tools", "tokens"
        );
        println!("{}", "-".repeat(100));
        for r in &records {
            let model = shorten(&r.model, 32);
            println!(
                "{:<26} {:<10} {:<12} {:<32} {:<6} {:>7}",
                r.timestamp.to_string(),
                shorten(&r.nous_id, 10),
                shorten(&r.provider, 12),
                model,
                r.tool_names.len(),
                r.token_count_estimate
            );
        }
    }
    Ok(())
}

fn show_record(log_dir: &Path, timestamp: &str) -> Result<()> {
    if !log_dir.exists() {
        snafu::whatever!("no audit log directory at {}", log_dir.display());
    }

    let target: jiff::Timestamp = timestamp
        .parse()
        .whatever_context("invalid timestamp (expected RFC3339, e.g. 2026-04-16T12:00:00Z)")?;

    // WHY: timestamp format matches the day the record was written, so open
    // the matching day file directly rather than scanning every day.
    let date = target.to_zoned(jiff::tz::TimeZone::UTC).date();
    let path = log_dir.join(format!("{date}.jsonl"));
    if !path.exists() {
        snafu::whatever!("no audit log for {date}");
    }

    let content =
        std::fs::read_to_string(&path).whatever_context("failed to read audit log file")?;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let rec: PromptAuditRecord = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "skipping malformed record");
                continue;
            }
        };
        if rec.timestamp == target {
            let out = serde_json::to_string_pretty(&rec).whatever_context("serialize record")?;
            println!("{out}");
            return Ok(());
        }
    }

    snafu::whatever!("no record at {timestamp}");
}

fn shorten(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_owned()
    } else {
        let keep = max.saturating_sub(1);
        let end = s.floor_char_boundary(keep);
        // WHY: `.get(..end)` cannot panic even if `end` somehow falls outside
        // the char boundary; falling back to the full string keeps the
        // human-readable output correct rather than aborting the list.
        let prefix = s.get(..end).unwrap_or(s);
        format!("{prefix}…")
    }
}
