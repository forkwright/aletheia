//! Pretty-printed report formatters for the binary surface.
//!
//! Kept in its own module so `main.rs` stays under the
//! `ARCHITECTURE/thick-binary` 200-line limit.

use std::path::Path;

use aletheia_sessions_migrate::{MigrationPlan, MigrationReport, VerificationReport};

/// Print the dry-run plan to stdout in operator-friendly form.
pub fn print_dry_run(plan: &MigrationPlan, dest: &Path) {
    println!("──── aletheia-sessions-migrate :: DRY RUN ────");
    println!(" source: {}", plan.source.display());
    println!(" dest:   {} (would create)", dest.display());
    println!(" sessions:        {:>7}", plan.counts.sessions);
    println!(" messages:        {:>7}", plan.counts.messages);
    println!(" usage records:   {:>7}", plan.counts.usage);
    println!(" distillations:   {:>7}", plan.counts.distillations);
    println!(" agent_notes:     {:>7}", plan.counts.notes);
    println!(" blackboard rows: {:>7}", plan.counts.blackboard);
    println!(
        " legacy extras to preserve out-of-band: {} session(s)",
        plan.legacy_extras_present
    );
    println!(
        " orphan messages detected: {} across {} session_id(s) (will synthesise orphan-recovery sessions)",
        plan.orphan_messages_detected, plan.orphan_sessions_to_synthesise
    );
    if let Some(ref sid) = plan.sample_session_id {
        println!(" sample session: {sid}");
    }
    println!("──────────────────────────────────────────────");
}

/// Print the migration report to stdout.
pub fn print_migration(r: &MigrationReport) {
    println!("──── aletheia-sessions-migrate :: MIGRATION COMPLETE ────");
    println!(" source:   {}", r.source.display());
    println!(" dest:     {}", r.dest.display());
    println!(" sessions:        {:>7}", r.counts.sessions);
    println!(" messages:        {:>7}", r.counts.messages);
    println!(" usage records:   {:>7}", r.counts.usage);
    println!(" distillations:   {:>7}", r.counts.distillations);
    println!(" agent_notes:     {:>7}", r.counts.notes);
    println!(" blackboard rows: {:>7}", r.counts.blackboard);
    println!(
        " legacy extras preserved (migration_legacy partition): {} session(s)",
        r.legacy_extras_preserved
    );
    println!(
        " orphan messages recovered: {} (across {} synthesised orphan-recovery session(s))",
        r.orphan_messages_recovered, r.orphan_sessions_synthesised
    );
    println!(" elapsed: {:.3}s", r.elapsed_secs);
    println!("─────────────────────────────────────────────────────────");
}

/// Print the verification report to stdout.
pub fn print_verification(v: &VerificationReport) {
    println!("──── aletheia-sessions-migrate :: VERIFY ────");
    println!(
        " sessions: source={}, dest={}",
        v.source_session_count, v.dest_session_count
    );
    println!(
        " messages: source={}, dest={}",
        v.source_message_count, v.dest_message_count
    );
    println!(
        " message body sha256: source={}",
        v.source_message_body_sha256
    );
    println!(
        "                        dest={}",
        v.dest_message_body_sha256
    );
    println!(" hash match: {}", v.message_body_hash_match);
    println!(" partitions:");
    for check in &v.partition_checks {
        println!(
            "   - {}: source_entries={}, dest_entries={}, hash_match={}",
            check.partition, check.source_entry_count, check.dest_entry_count, check.ok
        );
    }
    println!(" samples checked: {}", v.samples_checked);
    if v.mismatches.is_empty() {
        println!(" status: OK");
    } else {
        println!(" status: MISMATCH ({})", v.mismatches.len());
        for m in &v.mismatches {
            println!("   - {m}");
        }
    }
    println!("─────────────────────────────────────────────");
}
