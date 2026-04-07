//! Team coordination: child agent spawning with concurrency limits.
//!
//! WHY: Daemon-mode operations that exceed a single agent's scope (e.g.,
//! multi-file refactors, parallel knowledge graph updates) need controlled
//! child agent spawning with backpressure and failure isolation.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tracing::{info, warn};

use crate::bridge::DaemonBridge;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the team coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CoordinatorConfig {
    /// Maximum number of concurrent child agents.
    pub max_children: usize,
    /// Per-child timeout.
    pub child_timeout: Duration,
    /// Whether to abort all children on first failure.
    pub fail_fast: bool,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            max_children: 4,
            child_timeout: Duration::from_secs(300),
            fail_fast: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Child task
// ---------------------------------------------------------------------------

/// A child agent task to be coordinated.
#[derive(Debug, Clone)]
pub struct ChildTask {
    /// Unique task identifier.
    pub id: String,
    /// Nous agent to send the prompt to.
    pub nous_id: String,
    /// Session key for the child's conversation.
    pub session_key: String,
    /// Prompt to send.
    pub prompt: String,
}

/// Result of a coordinated child task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ChildResult {
    /// Task identifier.
    pub task_id: String,
    /// Whether the child succeeded.
    pub success: bool,
    /// Output text from the child.
    pub output: Option<String>,
    /// Wall-clock duration.
    pub duration: Duration,
    /// Error message if failed.
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Coordinator
// ---------------------------------------------------------------------------

/// Coordinator manages child agent lifecycle with concurrency limits.
///
/// Dispatches child tasks through the [`DaemonBridge`], enforces
/// concurrency via semaphore, and collects results.
pub struct Coordinator {
    config: CoordinatorConfig,
    semaphore: Arc<Semaphore>,
}

impl Coordinator {
    /// Create a new coordinator with the given configuration.
    #[must_use]
    pub fn new(config: CoordinatorConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_children));
        Self { config, semaphore }
    }

    /// Maximum number of concurrent child agents.
    #[must_use]
    pub fn max_children(&self) -> usize {
        self.config.max_children
    }

    /// Dispatch a batch of child tasks and collect results.
    ///
    /// Tasks execute sequentially, bounded by the semaphore. Each task
    /// acquires a permit before executing, providing backpressure when
    /// `max_children` tasks are already in flight.
    ///
    /// WHY sequential: the bridge is `&dyn DaemonBridge` (not `Arc`),
    /// so we can't send it to spawned tasks. Sequential execution with
    /// semaphore-bounded concurrency is correct for the daemon's use case
    /// (tasks are dispatched in plan order, not as a parallel batch).
    ///
    /// If `fail_fast` is true, remaining tasks are skipped on first failure.
    pub async fn dispatch_batch(
        &self,
        tasks: Vec<ChildTask>,
        bridge: &dyn DaemonBridge,
    ) -> Vec<ChildResult> {
        let mut results = Vec::with_capacity(tasks.len());

        for task in tasks {
            let _permit = self.semaphore.acquire().await;
            let Ok(_permit) = _permit else {
                warn!(task_id = %task.id, "semaphore closed, skipping task");
                results.push(ChildResult {
                    task_id: task.id,
                    success: false,
                    output: None,
                    duration: Duration::ZERO,
                    error: Some("semaphore closed".to_owned()),
                });
                continue;
            };

            let start = Instant::now();
            let timeout = self.config.child_timeout;

            info!(task_id = %task.id, nous_id = %task.nous_id, "dispatching child task");

            let result =
                tokio::time::timeout(timeout, bridge.send_prompt(&task.nous_id, &task.session_key, &task.prompt))
                    .await;

            let duration = start.elapsed();

            let child_result = match result {
                Ok(Ok(exec)) => ChildResult {
                    task_id: task.id,
                    success: exec.success,
                    output: exec.output,
                    duration,
                    error: None,
                },
                Ok(Err(e)) => ChildResult {
                    task_id: task.id,
                    success: false,
                    output: None,
                    duration,
                    error: Some(e.to_string()),
                },
                Err(_) => ChildResult {
                    task_id: task.id,
                    success: false,
                    output: None,
                    duration,
                    error: Some(format!("child timed out after {timeout:?}")),
                },
            };

            let failed = !child_result.success;
            results.push(child_result);

            if failed && self.config.fail_fast {
                warn!("child failed, fail_fast aborting remaining tasks");
                break;
            }
        }

        info!(
            total = results.len(),
            succeeded = results.iter().filter(|r| r.success).count(),
            failed = results.iter().filter(|r| !r.success).count(),
            "coordination batch complete"
        );

        results
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = CoordinatorConfig::default();
        assert_eq!(config.max_children, 4);
        assert!(!config.fail_fast);
    }

    #[test]
    fn coordinator_max_children() {
        let coord = Coordinator::new(CoordinatorConfig::default());
        assert_eq!(coord.max_children(), 4);
    }

    #[test]
    fn config_roundtrip() {
        let config = CoordinatorConfig {
            max_children: 8,
            child_timeout: Duration::from_secs(600),
            fail_fast: true,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: CoordinatorConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.max_children, 8);
        assert!(deserialized.fail_fast);
    }
}
