//! Garbage collection for stale task entries.
//!
//! WHY: Completed and failed tasks waste memory and clutter listings if retained
//! indefinitely. Periodic sweeps are simpler than per-task timers and avoid the
//! coordination overhead of expiry-based eviction.

use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use super::output;
use super::registry::TaskRegistry;

/// Default interval between GC sweeps.
const DEFAULT_GC_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Spawn a background GC task that periodically evicts stale entries.
///
/// The task runs until the `shutdown` token is cancelled. Output files for
/// evicted tasks are cleaned up from disk.
///
/// Returns a `JoinHandle` so the caller can await shutdown completion.
pub fn spawn_gc_task(
    registry: TaskRegistry,
    shutdown: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    spawn_gc_task_with_interval(registry, shutdown, DEFAULT_GC_INTERVAL)
}

/// Spawn a GC task with a custom sweep interval.
///
/// WHY: Exposed for testing with short intervals.
pub(crate) fn spawn_gc_task_with_interval(
    registry: TaskRegistry,
    shutdown: CancellationToken,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    let span = tracing::info_span!("task_gc");
    tokio::spawn(
        async move {
            let mut ticker = tokio::time::interval(interval);
            // WHY: Skip the immediate first tick -- tasks just registered
            // shouldn't be swept immediately.
            ticker.tick().await;

            loop {
                tokio::select! {
                    biased;
                    () = shutdown.cancelled() => {
                        debug!("GC task shutting down");
                        break;
                    }
                    _ = ticker.tick() => {
                        run_gc_sweep(&registry).await;
                    }
                }
            }
        }
        .instrument(span),
    )
}

/// Execute a single GC sweep: evict stale tasks and clean up output files.
async fn run_gc_sweep(registry: &TaskRegistry) {
    let evicted = match registry.gc_sweep() {
        Ok(evicted) => evicted,
        Err(e) => {
            warn!(error = %e, "GC sweep failed");
            return;
        }
    };

    if evicted.is_empty() {
        return;
    }

    debug!(count = evicted.len(), "GC evicted stale tasks");

    for (task_id, output_path) in evicted {
        if let Some(path) = output_path
            && let Err(e) = output::remove_output_file(&path).await
        {
            // NOTE: Best-effort cleanup. File may already be gone.
            warn!(%task_id, error = %e, "failed to remove output file during GC");
        }
    }
}

// WHY: `tracing::Instrument` must be in scope for `.instrument()` on futures.
use tracing::Instrument as _;

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::tasks::types::TaskType;

    #[tokio::test]
    async fn gc_evicts_completed_tasks_after_deadline() {
        // WHY: Zero deadline + short interval = immediate eviction on first sweep.
        let registry = TaskRegistry::new(Duration::from_secs(0));
        let shutdown = CancellationToken::new();

        let (id, _) = registry
            .register(
                TaskType::Shell {
                    command: "done".into(),
                },
                "stale task".into(),
            )
            .expect("register");

        registry
            .update_status(id, crate::tasks::TaskStatus::Running)
            .expect("to running");
        registry
            .update_status(id, crate::tasks::TaskStatus::Completed)
            .expect("to completed");

        let handle = spawn_gc_task_with_interval(
            registry.clone(),
            shutdown.clone(),
            Duration::from_millis(50),
        );

        // WHY: Wait long enough for at least one sweep.
        tokio::time::sleep(Duration::from_millis(200)).await;

        shutdown.cancel();
        handle.await.expect("gc task join");

        assert!(registry.get(id).is_err(), "task should have been evicted");
    }

    #[tokio::test]
    async fn gc_preserves_running_tasks() {
        let registry = TaskRegistry::new(Duration::from_secs(0));
        let shutdown = CancellationToken::new();

        let (id, _) = registry
            .register(
                TaskType::Agent {
                    agent_id: "alice".into(),
                    prompt: "work".into(),
                },
                "active task".into(),
            )
            .expect("register");

        registry
            .update_status(id, crate::tasks::TaskStatus::Running)
            .expect("to running");

        let handle = spawn_gc_task_with_interval(
            registry.clone(),
            shutdown.clone(),
            Duration::from_millis(50),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        shutdown.cancel();
        handle.await.expect("gc task join");

        assert!(registry.get(id).is_ok(), "running task should be preserved");
    }

    #[tokio::test]
    async fn gc_cleans_up_output_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let registry = TaskRegistry::new(Duration::from_secs(0));
        let shutdown = CancellationToken::new();

        let (id, _) = registry
            .register(
                TaskType::Shell {
                    command: "echo hi".into(),
                },
                "with output".into(),
            )
            .expect("register");

        // Create an output file and register its path.
        let mut writer = crate::tasks::OutputWriter::new(dir.path())
            .await
            .expect("create writer");
        writer.write_chunk(b"output data").await.expect("write");
        let output_path = writer.path().to_path_buf();

        registry
            .set_output_path(id, output_path.clone())
            .expect("set output path");

        registry
            .update_status(id, crate::tasks::TaskStatus::Running)
            .expect("to running");
        registry
            .update_status(id, crate::tasks::TaskStatus::Completed)
            .expect("to completed");

        assert!(output_path.exists(), "output file should exist before GC");

        let handle = spawn_gc_task_with_interval(
            registry.clone(),
            shutdown.clone(),
            Duration::from_millis(50),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        shutdown.cancel();
        handle.await.expect("gc task join");

        assert!(
            !output_path.exists(),
            "output file should be cleaned up after GC"
        );
    }

    #[tokio::test]
    async fn gc_shutdown_is_clean() {
        let registry = TaskRegistry::with_default_deadline();
        let shutdown = CancellationToken::new();

        let handle =
            spawn_gc_task_with_interval(registry, shutdown.clone(), Duration::from_millis(50));

        shutdown.cancel();
        // WHY: Should complete promptly without hanging.
        tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("gc task should shut down within 1s")
            .expect("gc task join");
    }
}
