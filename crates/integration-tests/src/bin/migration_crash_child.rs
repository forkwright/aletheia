//! Child process for the migration crash-injection matrix (aletheia#5779
//! §8.4).
//!
//! Opens a seeded fjall knowledge store (running its pending migrations as
//! a side effect of `open_fjall`) and, if `ALETHEIA_MIGRATION_CRASH_AT` is
//! set, aborts the process the instant migration code reaches that step
//! number. A separate binary is required rather than `fork()`-ing from the
//! test process: `krites::Db` spawns rayon threads (`krites/src/lib.rs:332`
//! doc comment), and `fork()` is unsafe once a process holds live threads.
//!
//! Usage: `migration_crash_child <fjall-path> <dim>`
//! Env: `ALETHEIA_MIGRATION_CRASH_AT=<step 1-9>` to arm the crash hook.
//! Prints `MIGRATION_CHILD_OK` and exits 0 on a clean run with no crash
//! armed (or a step number that never fires); aborts (SIGABRT) if the
//! armed step fires.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: migration_crash_child <fjall-path> <dim>");
        return ExitCode::from(2);
    };
    let dim: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(4);

    if let Ok(step_str) = std::env::var("ALETHEIA_MIGRATION_CRASH_AT") {
        let Ok(target_step) = step_str.parse::<u32>() else {
            eprintln!("ALETHEIA_MIGRATION_CRASH_AT must be a step number 1-9, got: {step_str}");
            return ExitCode::from(2);
        };
        mneme::crash_injection::register_crash_hook(move |step| {
            if step == target_step {
                // WHY: this is the one legitimate call site for a real
                // process crash in the whole workspace — proving the
                // migration recovery sweep survives losing the process at
                // an exact sequence point (plan §8.4). `abort()`, never
                // `panic!()`: a panic unwinds and may flush buffered state
                // the crash is supposed to interrupt.
                #[expect(
                    clippy::disallowed_methods,
                    reason = "the crash-injection matrix's entire purpose is a real process abort at an exact migration step — this is the single sanctioned call site, gated behind an explicit opt-in env var no normal test run sets"
                )]
                std::process::abort();
            }
        });
    }

    let config = mneme::knowledge_store::KnowledgeConfig {
        dim,
        allow_assumed_embedding_meta: true,
        ..Default::default()
    };

    match mneme::knowledge_store::KnowledgeStore::open_fjall(&path, config) {
        Ok(_store) => {
            println!("MIGRATION_CHILD_OK");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("MIGRATION_CHILD_ERROR: {e}");
            ExitCode::FAILURE
        }
    }
}
