//! Pretty-printed report formatters for the binary surface.
//!
//! Kept in its own module so `main.rs` stays under the
//! `ARCHITECTURE/thick-binary` 200-line limit.

use std::io::{self, Write as _};
use std::path::Path;

use crate::migrate::{MigrationPlan, MigrationReport};
use crate::verify::VerificationReport;

/// Print the dry-run plan to stdout in operator-friendly form.
pub(crate) fn print_dry_run(plan: &MigrationPlan, dest: &Path) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(out, "──── aletheia-sessions-migrate :: DRY RUN ────")?;
    writeln!(out, " source: {}", plan.source.display())?;
    writeln!(out, " dest:   {} (would create)", dest.display())?;
    writeln!(out, " sessions:        {:>7}", plan.counts.sessions)?;
    writeln!(out, " messages:        {:>7}", plan.counts.messages)?;
    writeln!(out, " usage records:   {:>7}", plan.counts.usage)?;
    writeln!(out, " distillations:   {:>7}", plan.counts.distillations)?;
    writeln!(out, " agent_notes:     {:>7}", plan.counts.notes)?;
    writeln!(out, " blackboard rows: {:>7}", plan.counts.blackboard)?;
    writeln!(
        out,
        " legacy extras to preserve out-of-band: {} session(s)",
        plan.legacy_extras_present
    )?;
    writeln!(
        out,
        " legacy-only sidecar entries: {}",
        plan.legacy_sidecar_entries_present
    )?;
    writeln!(
        out,
        " orphan messages detected: {} across {} session_id(s) (will synthesise orphan-recovery sessions)",
        plan.orphan_messages_detected, plan.orphan_sessions_to_synthesise
    )?;
    if let Some(ref sid) = plan.sample_session_id {
        writeln!(out, " sample session: {sid}")?;
    }
    writeln!(out, "──────────────────────────────────────────────")?;
    Ok(())
}

/// Print the migration report to stdout.
pub(crate) fn print_migration(r: &MigrationReport) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(
        out,
        "──── aletheia-sessions-migrate :: MIGRATION COMPLETE ────"
    )?;
    writeln!(out, " source:   {}", r.source.display())?;
    writeln!(out, " dest:     {}", r.dest.display())?;
    writeln!(out, " sessions:        {:>7}", r.counts.sessions)?;
    writeln!(out, " messages:        {:>7}", r.counts.messages)?;
    writeln!(out, " usage records:   {:>7}", r.counts.usage)?;
    writeln!(out, " distillations:   {:>7}", r.counts.distillations)?;
    writeln!(out, " agent_notes:     {:>7}", r.counts.notes)?;
    writeln!(out, " blackboard rows: {:>7}", r.counts.blackboard)?;
    writeln!(
        out,
        " legacy extras preserved (migration_legacy partition): {} session(s)",
        r.legacy_extras_preserved
    )?;
    writeln!(
        out,
        " legacy-only sidecar entries preserved: {}",
        r.legacy_sidecar_entries_preserved
    )?;
    writeln!(
        out,
        " orphan messages recovered: {} (across {} synthesised orphan-recovery session(s))",
        r.orphan_messages_recovered, r.orphan_sessions_synthesised
    )?;
    writeln!(out, " elapsed: {:.3}s", r.elapsed_secs)?;
    writeln!(
        out,
        "─────────────────────────────────────────────────────────"
    )?;
    Ok(())
}

/// Print the verification report to stdout.
pub(crate) fn print_verification(v: &VerificationReport) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(out, "──── aletheia-sessions-migrate :: VERIFY ────")?;
    writeln!(
        out,
        " sessions: source={}, dest={}",
        v.source_session_count, v.dest_session_count
    )?;
    writeln!(
        out,
        " messages: source={}, dest={}",
        v.source_message_count, v.dest_message_count
    )?;
    writeln!(
        out,
        " message body sha256: source={}",
        v.source_message_body_sha256
    )?;
    writeln!(
        out,
        "                        dest={}",
        v.dest_message_body_sha256
    )?;
    writeln!(out, " hash match: {}", v.message_body_hash_match)?;
    writeln!(out, " partitions:")?;
    for check in &v.partition_checks {
        writeln!(
            out,
            "   - {}: source_entries={}, dest_entries={}, hash_match={}",
            check.partition, check.source_entry_count, check.dest_entry_count, check.ok
        )?;
    }
    writeln!(out, " samples checked: {}", v.samples_checked)?;
    if v.mismatches.is_empty() {
        writeln!(out, " status: OK")?;
    } else {
        writeln!(out, " status: MISMATCH ({})", v.mismatches.len())?;
        for mismatch in &v.mismatches {
            writeln!(out, "   - {mismatch}")?;
        }
    }
    writeln!(out, "─────────────────────────────────────────────")?;
    Ok(())
}
