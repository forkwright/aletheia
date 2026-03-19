//! Tracing initialisation, log retention, and shutdown signal.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, info};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

use aletheia_koina::redacting_layer::RedactingLayer;
use aletheia_taxis::oikos::Oikos;

/// Spawn a background task that prunes log files older than `retention_days`.
///
/// Runs immediately at startup (to clean up leftovers from previous server
/// runs) and then every 24 hours. Uses `TraceRotator` from
/// `aletheia-oikonomos` to avoid duplicating the age-based file cleanup logic.
///
/// Files are moved to `log_dir/archive/` and then pruned immediately
/// (`max_archives = 0`), which produces a net deletion of old log files without
/// requiring a separate archive housekeeping step.
pub(super) fn spawn_log_retention(log_dir: PathBuf, retention_days: u32, token: CancellationToken) {
    use aletheia_oikonomos::maintenance::{TraceRotationConfig, TraceRotator};

    tokio::spawn(
        async move {
            loop {
                let dir = log_dir.clone();
                let archive_dir = dir.join("archive");
                let cfg = TraceRotationConfig {
                    enabled: true,
                    trace_dir: dir,
                    archive_dir,
                    max_age_days: retention_days,
                    // No size-based eviction: only age matters for log retention.
                    max_total_size_mb: 1_000_000,
                    compress: false,
                    // Prune every archived file immediately: net effect is deletion.
                    max_archives: 0,
                };

                let result =
                    tokio::task::spawn_blocking(move || TraceRotator::new(cfg).rotate()).await;

                match result {
                    Ok(Ok(report)) if report.files_pruned > 0 => {
                        tracing::info!(
                            pruned = report.files_pruned,
                            "log retention: removed old log files"
                        );
                    }
                    // NOTE: no files pruned, nothing to report
                    Ok(Ok(_)) => {}
                    Ok(Err(e)) => {
                        tracing::warn!(error = %e, "log retention cleanup failed");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "log retention task join error");
                    }
                }

                tokio::select! {
                    biased;
                    () = token.cancelled() => break,
                    // NOTE: 24h interval elapsed, run next retention cycle
                    () = tokio::time::sleep(std::time::Duration::from_secs(24 * 3600)) => {}
                }
            }
        }
        .instrument(tracing::info_span!("log_retention")),
    );
}

/// Initialise the global tracing subscriber with dual output:
///
/// - **Console**: human-readable (or JSON) at `log_level`, respecting
///   `RUST_LOG` when set.
/// - **File**: always JSON at `file_level` (default `"warn"`), written to
///   `log_dir/aletheia.log.<date>` with daily rotation via `tracing_appender`.
///
/// Returns the [`WorkerGuard`] that must be kept alive for the entire process
/// lifetime; dropping it flushes and closes the non-blocking file writer.
pub(super) fn init_tracing(
    log_level: &str,
    json: bool,
    log_dir: &Path,
    file_level: &str,
    redaction: &aletheia_taxis::config::RedactionSettings,
) -> Result<WorkerGuard> {
    // Console filter: respect RUST_LOG env var, fall back to the CLI level.
    let console_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("aletheia={log_level},{log_level}")));

    // File filter: configured level, default "warn": captures WARN+ even
    // when the console is set to INFO.
    let file_filter = EnvFilter::try_new(file_level).with_context(|| {
        format!("invalid logging.level '{file_level}' — use a tracing directive such as 'warn'")
    })?;

    // Daily-rolling file appender. tracing_appender creates one file per day:
    //   aletheia.log.2026-03-14, aletheia.log.2026-03-15, …
    // The non_blocking wrapper offloads writes to a background thread.
    let file_appender = tracing_appender::rolling::daily(log_dir, "aletheia.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    // Console layers: exactly one is Some, the other None.
    // Option<L> implements Layer<S> as a no-op when None, so both arms compose
    // cleanly without type-erasing via Box<dyn Layer>.
    let console_filter_clone = console_filter.clone();
    let json_console = json.then(|| {
        fmt::layer()
            .json()
            .with_target(true)
            .with_filter(console_filter_clone)
    });
    let text_console = (!json).then(|| {
        fmt::layer()
            .with_target(true)
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false)
            .with_filter(console_filter)
    });

    // File layer: redacting or plain depending on config.
    if redaction.enabled {
        let redacting = RedactingLayer::new(
            non_blocking,
            redaction.redact_fields.iter().cloned(),
            redaction.truncate_fields.iter().cloned(),
            redaction.truncate_length,
        )
        .with_filter(file_filter);

        tracing_subscriber::registry()
            .with(json_console)
            .with(text_console)
            .with(redacting)
            .try_init()
            .context("failed to set global tracing subscriber")?;
    } else {
        let file_layer = fmt::layer()
            .json()
            .with_ansi(false)
            .with_target(true)
            .with_writer(non_blocking)
            .with_filter(file_filter);

        tracing_subscriber::registry()
            .with(json_console)
            .with(text_console)
            .with(file_layer)
            .try_init()
            .context("failed to set global tracing subscriber")?;
    }

    Ok(guard)
}

/// Resolve the absolute path of the log directory.
///
/// If `log_dir` is set in config, relative paths are joined to the instance
/// root; absolute paths are used as-is. Falls back to `{instance}/logs/`.
pub(super) fn resolve_log_dir(oikos: &Oikos, log_dir: Option<&str>) -> PathBuf {
    match log_dir {
        Some(dir) => {
            let path = PathBuf::from(dir);
            if path.is_absolute() {
                path
            } else {
                oikos.root().join(path)
            }
        }
        None => oikos.logs(),
    }
}

#[expect(
    clippy::expect_used,
    reason = "signal handler installation is infallible on supported platforms"
)]
pub(super) async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install ctrl+c handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("received ctrl+c"),
        () = terminate => info!("received SIGTERM"),
    }
}
