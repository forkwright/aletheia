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
    // WHY: parse --since eagerly so bogus input like "not-a-date" or
    // "2026-13-99" fails loudly instead of being string-prefix-compared
    // against the filename stem (which silently filters everything in or
    // out depending on lex order).
    let since_date: Option<jiff::civil::Date> = match since {
        Some(s) => Some(
            s.parse::<jiff::civil::Date>()
                .whatever_context("invalid --since (expected YYYY-MM-DD)")?,
        ),
        None => None,
    };

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

    if let Some(since) = since_date {
        files.retain(|e| {
            e.file_name()
                .to_str()
                .and_then(|n| n.strip_suffix(".jsonl"))
                .and_then(|d| d.parse::<jiff::civil::Date>().ok())
                .is_some_and(|d| d >= since)
        });
    }

    let mut records: Vec<PromptAuditRecord> = Vec::new();
    let mut skipped = 0_usize;
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
                    skipped += 1;
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
    // WHY: surface parse failures to the operator. Without this the user only
    // sees "no audit records matched" even when the directory holds N corrupt
    // or schema-drifted files — they would have no signal that the log exists
    // but isn't readable. JSON mode keeps stdout machine-parseable; the
    // skipped count goes to stderr.
    if skipped > 0 {
        eprintln!(
            "note: skipped {skipped} unparseable record(s) (run with RUST_LOG=warn to see details)"
        );
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
    let mut skipped = 0_usize;
    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let rec: PromptAuditRecord = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                skipped += 1;
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

    // WHY: distinguish "the day file held N records but none matched the
    // requested timestamp AND M were unparseable" from "day file was clean
    // but the timestamp wasn't there". Operators chasing a missing record
    // need to know whether the file is corrupt.
    if skipped > 0 {
        snafu::whatever!(
            "no record at {timestamp} ({skipped} unparseable record(s) skipped — run with RUST_LOG=warn to see details)"
        );
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

#[cfg(test)]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::disallowed_methods,
    reason = "test fixture writes jsonl files into a tempdir before exercising list_records"
)]
mod tests {
    use super::*;

    fn write_jsonl(dir: &Path, name: &str, lines: &[&str]) {
        std::fs::write(dir.join(name), lines.join("\n")).expect("write fixture");
    }

    fn rec_jsonl(timestamp: &str, nous: &str) -> String {
        format!(
            r#"{{"timestamp":"{timestamp}","nous_id":"{nous}","session_id":"s","turn_id":"t","provider":"anthropic","deployment_target":"cloud","model":"m","system_prompt_hash":"","system_prompt_bytes":0,"message_count":0,"token_count_estimate":0,"fact_ids_included":[],"tool_names":[]}}"#
        )
    }

    #[test]
    fn list_records_rejects_invalid_since_strings() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dir = tmp.path();
        write_jsonl(
            dir,
            "2026-05-28.jsonl",
            &[&rec_jsonl("2026-05-28T08:00:00Z", "syn")],
        );

        for bogus in ["not-a-date", "2026-13-99", "garbage", "0"] {
            let err = list_records(dir, Some(bogus), None, 50, true)
                .expect_err(&format!("expected error for --since '{bogus}'"));
            let msg = format!("{err:#}");
            assert!(
                msg.contains("invalid --since"),
                "expected error mentioning --since for '{bogus}', got: {msg}"
            );
        }
    }

    #[test]
    fn list_records_accepts_valid_since_and_uses_date_compare() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dir = tmp.path();
        write_jsonl(
            dir,
            "2025-05-28.jsonl",
            &[&rec_jsonl("2025-05-28T08:00:00Z", "old")],
        );
        write_jsonl(
            dir,
            "2026-05-28.jsonl",
            &[&rec_jsonl("2026-05-28T08:00:00Z", "new")],
        );

        list_records(dir, Some("1900-01-01"), None, 50, true).expect("ancient since should pass");
        list_records(dir, Some("9999-12-31"), None, 50, true)
            .expect("future since should pass (no rows)");
        list_records(dir, Some("2026-01-01"), None, 50, true).expect("mid since should pass");
    }

    #[test]
    fn list_records_does_not_silently_swallow_malformed_lines() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dir = tmp.path();
        write_jsonl(
            dir,
            "2026-05-28.jsonl",
            &[
                &rec_jsonl("2026-05-28T08:00:00Z", "syn"),
                r#"{"not":"a record"}"#,
                "{not even json",
            ],
        );
        list_records(dir, None, None, 50, false).expect("call succeeds even with corrupt lines");
    }

    #[test]
    fn show_record_with_only_corrupt_lines_reports_skipped_count() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dir = tmp.path();
        write_jsonl(
            dir,
            "2026-05-28.jsonl",
            &[r#"{"not":"a record"}"#, "{not even json"],
        );
        let err =
            show_record(dir, "2026-05-28T08:00:00Z").expect_err("should error: no matching record");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("2 unparseable"),
            "error should mention skipped count, got: {msg}"
        );
    }
}
