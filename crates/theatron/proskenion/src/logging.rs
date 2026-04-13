//! Log-to-file initialisation for the desktop application.
//!
//! Redirects tracing output to a daily-rolling file at
//! `~/.local/share/aletheia/logs/proskenion.log` so the app runs cleanly
//! without a terminal attached. When the `RUST_LOG` environment variable is
//! set or `--verbose` is passed, a stderr layer is added on top.

use std::path::PathBuf;

use tracing_appender::rolling;
use tracing_subscriber::fmt::Layer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

/// Initialise the global tracing subscriber.
///
/// Always writes to a daily-rolling file at
/// `~/.local/share/aletheia/logs/proskenion.log`.
/// Also emits to stderr when `RUST_LOG` is set in the environment or
/// `verbose` is `true`.
///
/// The returned [`tracing_appender::non_blocking::WorkerGuard`] **must** be
/// bound to a variable in `main` and kept alive for the duration of the
/// process. Dropping it early flushes and closes the log file, silencing all
/// subsequent log output.
pub(crate) fn init(verbose: bool) -> tracing_appender::non_blocking::WorkerGuard {
    let log_dir = log_directory();

    // WHY: Create the directory here (not lazily) so any failure surfaces at
    // startup rather than silently swallowing log lines.
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!(
            "proskenion: failed to create log directory {}: {e}",
            log_dir.display()
        );
    }

    let file_appender = rolling::daily(&log_dir, "proskenion.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // File layer: always on, no ANSI, level from RUST_LOG or default info.
    let file_layer = Layer::new()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_filter(
            EnvFilter::from_default_env()
                .add_directive("proskenion=info".parse().unwrap_or_default()),
        );

    let subscriber = tracing_subscriber::registry().with(file_layer);

    // Stderr layer: only when RUST_LOG is set or --verbose was passed.
    let rust_log_set = std::env::var("RUST_LOG").is_ok();
    if verbose || rust_log_set {
        let stderr_layer = Layer::new()
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .with_filter(EnvFilter::from_default_env());
        subscriber.with(stderr_layer).init();
    } else {
        subscriber.init();
    }

    guard
}

/// Resolve the log directory: `~/.local/share/aletheia/logs/`.
fn log_directory() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("aletheia")
        .join("logs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_directory_ends_with_aletheia_logs() {
        let dir = log_directory();
        let s = dir.to_string_lossy();
        assert!(
            s.ends_with("aletheia/logs"),
            "unexpected log dir: {s}"
        );
    }
}
