//! Command-line parser and execution flow.

use std::io::Write as _;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use tracing::{info, warn};

use crate::commands::report;
use crate::migrate::{FIELD_MAPPING_DOC, run_dry_run, stage_migration};
use crate::verify::run_verification;

#[derive(Debug, Parser)]
#[command(
    name = "aletheia-sessions-migrate",
    about = "One-shot SQLite v32 -> fjall sessions-store importer for legacy aletheia 0.15.x instances.",
    long_about = "Reads the legacy SQLite sessions DB read-only and writes its content to a fresh fjall keyspace whose layout matches crates/graphe/src/store/fjall_store.rs. \
The migrator writes a staged destination, verifies it, and only then publishes it over the requested destination.",
    version
)]
// WHY: a one-shot migrator CLI exposes orthogonal mode flags
// (--dry-run, --verify, --verify-only, --replace-existing, confirmation,
// --print-mapping). Each is a flag, not a state-machine value, and clap does not support
// derive on multi-variant enums for non-positional flags without
// duplicating logic. Keeping them as bools is the idiomatic clap
// shape; the lint is generic-rust, not CLI-aware.
//
// `doc_markdown` is also relaxed here because the doc comments on each
// field are clap help text (printed verbatim by `--help`); backticks
// would surface to operators as literal characters.
#[expect(
    clippy::struct_excessive_bools,
    reason = "orthogonal CLI mode flags; idiomatic clap shape"
)]
#[expect(
    clippy::doc_markdown,
    reason = "field doc comments are clap --help text; backticks would surface as literal characters"
)]
struct Cli {
    /// Path to the SQLite source DB (read-only).
    #[arg(long)]
    source: PathBuf,

    /// Path to the fjall destination directory. Created if absent.
    #[arg(long)]
    dest: PathBuf,

    /// Read source and report a migration plan; don't write fjall.
    #[arg(long)]
    dry_run: bool,

    /// Print the mandatory verification report before publishing.
    #[arg(long)]
    verify: bool,

    /// Verify only - assumes a previous run already wrote dest.
    #[arg(long, conflicts_with = "dry_run")]
    verify_only: bool,

    /// Destructively replace a non-empty destination after staging verifies.
    #[arg(long, requires = "i_understand_this_replaces_destination")]
    replace_existing: bool,

    /// Confirm that replacement removes the current destination after publish succeeds.
    #[arg(long, requires = "replace_existing")]
    i_understand_this_replaces_destination: bool,

    /// Number of per-session samples to spot-check during verification.
    #[arg(long, default_value_t = 16)]
    samples: usize,

    /// Print the SQLite -> fjall field mapping document and exit.
    #[arg(long)]
    print_mapping: bool,
}

pub(crate) fn run_from_args() -> Result<()> {
    let cli = Cli::parse();

    if cli.print_mapping {
        std::io::stdout()
            .lock()
            .write_all(FIELD_MAPPING_DOC.as_bytes())?;
        return Ok(());
    }

    run(&cli)
}

fn run(cli: &Cli) -> Result<()> {
    if cli.verify_only {
        let report = run_verification(&cli.source, &cli.dest, cli.samples)?;
        report::print_verification(&report)?;
        if !report.ok() {
            anyhow::bail!(
                "verification failed: {} mismatch(es)",
                report.mismatches.len()
            );
        }
        return Ok(());
    }

    if cli.dry_run {
        let plan = run_dry_run(&cli.source)?;
        report::print_dry_run(&plan, &cli.dest)?;
        return Ok(());
    }

    if cli.replace_existing {
        warn!(
            dest = %cli.dest.display(),
            "replacement requested; current destination will be moved to a temporary backup during publish and deleted after successful publish"
        );
        writeln!(
            std::io::stderr().lock(),
            "warning: --replace-existing replaces {} after staging verifies; the temporary backup is deleted after successful publish",
            cli.dest.display()
        )?;
    }

    let staged = stage_migration(&cli.source, &cli.dest, cli.replace_existing)?;
    let report_preview = staged.report();
    info!(
        sessions = report_preview.counts.sessions,
        messages = report_preview.counts.messages,
        "migration staged"
    );

    info!("running verification pass against staged destination");
    let verification = staged.verify(&cli.source, cli.samples)?;
    if cli.verify || !verification.ok() {
        report::print_verification(&verification)?;
    }
    if !verification.ok() {
        anyhow::bail!(
            "verification failed: {} mismatch(es)",
            verification.mismatches.len()
        );
    }

    let report = staged.publish()?;
    report::print_migration(&report)?;

    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "CLI parse tests use direct assertions over fixture argv"
)]
mod tests {
    use super::*;

    #[test]
    fn replace_existing_requires_confirmation_flag() {
        let err = Cli::try_parse_from([
            "aletheia-sessions-migrate",
            "--source",
            "source.db",
            "--dest",
            "dest.fjall",
            "--replace-existing",
        ])
        .expect_err("confirmation is required");
        let message = err.to_string();
        assert!(
            message.contains("--i-understand-this-replaces-destination"),
            "expected confirmation flag in clap error, got: {message}"
        );
    }

    #[test]
    fn confirmation_requires_replace_existing_flag() {
        let err = Cli::try_parse_from([
            "aletheia-sessions-migrate",
            "--source",
            "source.db",
            "--dest",
            "dest.fjall",
            "--i-understand-this-replaces-destination",
        ])
        .expect_err("replacement flag is required");
        let message = err.to_string();
        assert!(
            message.contains("--replace-existing"),
            "expected replacement flag in clap error, got: {message}"
        );
    }

    #[test]
    fn replace_existing_with_confirmation_parses() {
        let cli = Cli::try_parse_from([
            "aletheia-sessions-migrate",
            "--source",
            "source.db",
            "--dest",
            "dest.fjall",
            "--replace-existing",
            "--i-understand-this-replaces-destination",
        ])
        .expect("replacement confirmation parses");
        assert!(cli.replace_existing);
        assert!(cli.i_understand_this_replaces_destination);
    }
}
