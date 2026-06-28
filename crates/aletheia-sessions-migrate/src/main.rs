//! `aletheia-sessions-migrate` — one-shot `SQLite` v32 → fjall importer
//! for legacy aletheia 0.15.x session DBs.
//!
//! See `FIELD_MAPPING.md` for column-by-column mapping.

mod commands;

use std::path::PathBuf;
use std::process::ExitCode;

use aletheia_sessions_migrate::migrate::{FIELD_MAPPING_DOC, run_dry_run};
use aletheia_sessions_migrate::{run_verification, stage_migration};
use anyhow::Result;
use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use commands::report;

#[derive(Debug, Parser)]
#[command(
    name = "aletheia-sessions-migrate",
    about = "One-shot SQLite v32 → fjall sessions-store importer for legacy aletheia 0.15.x instances.",
    long_about = "Reads the legacy SQLite sessions DB read-only and writes its content to a fresh fjall keyspace whose layout matches crates/graphe/src/store/fjall_store.rs. \
The migrator writes a staged destination, verifies it, and only then publishes it over the requested destination.",
    version
)]
// WHY: a one-shot migrator CLI exposes orthogonal mode flags
// (--dry-run, --verify, --verify-only, --force, --print-mapping). Each
// is a flag, not a state-machine value, and clap does not support
// derive on multi-variant enums for non-positional flags without
// duplicating logic. Keeping them as bools is the idiomatic clap
// shape — the lint is generic-rust, not CLI-aware.
//
// `doc_markdown` is also relaxed here because the doc comments on each
// field are clap *help text* (printed verbatim by `--help`); backticks
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

    /// Verify only — assumes a previous run already wrote `dest`.
    #[arg(long, conflicts_with = "dry_run")]
    verify_only: bool,

    /// Replace a non-empty destination through the staged backup path.
    #[arg(long)]
    force: bool,

    /// Number of per-session samples to spot-check during verification.
    #[arg(long, default_value_t = 16)]
    samples: usize,

    /// Print the SQLite → fjall field mapping document and exit.
    #[arg(long)]
    print_mapping: bool,
}

fn main() -> ExitCode {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,aletheia_sessions_migrate=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let cli = Cli::parse();

    if cli.print_mapping {
        println!("{FIELD_MAPPING_DOC}");
        return ExitCode::SUCCESS;
    }

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            error!(error = ?err, "migration failed");
            eprintln!("error: {err:#}");
            ExitCode::from(1)
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    if cli.verify_only {
        let report = run_verification(&cli.source, &cli.dest, cli.samples)?;
        report::print_verification(&report);
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
        report::print_dry_run(&plan, &cli.dest);
        return Ok(());
    }

    let staged = stage_migration(&cli.source, &cli.dest, cli.force)?;

    info!("running verification pass against staged destination");
    let v = staged.verify(&cli.source, cli.samples)?;
    if cli.verify || !v.ok() {
        report::print_verification(&v);
    }
    if !v.ok() {
        anyhow::bail!("verification failed: {} mismatch(es)", v.mismatches.len());
    }

    let report = staged.publish()?;
    report::print_migration(&report);

    Ok(())
}
