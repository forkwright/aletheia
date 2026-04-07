//! Event-driven activation: file watchers, webhooks, and event deduplication.
//!
//! WHY: KAIROS daemon needs to react to external events (file changes,
//! webhook calls, GitHub notifications) in addition to cron-scheduled tasks.
//! The trigger router multiplexes these event sources into a unified channel
//! that the task runner consumes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Trigger events
// ---------------------------------------------------------------------------

/// An event from any trigger source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TriggerEvent {
    /// A watched file or directory changed.
    FileChange {
        /// Path that changed.
        path: PathBuf,
        /// Kind of change.
        kind: FileChangeKind,
    },
    /// An HTTP webhook was received.
    Webhook {
        /// Webhook endpoint path (e.g., "/hooks/github").
        endpoint: String,
        /// Request body.
        payload: serde_json::Value,
    },
    /// Manual trigger from CLI or API.
    Manual {
        /// Trigger reason.
        reason: String,
    },
}

/// Kind of file system change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum FileChangeKind {
    /// File or directory created.
    Created,
    /// File content modified.
    Modified,
    /// File or directory removed.
    Removed,
}

// ---------------------------------------------------------------------------
// Trigger router
// ---------------------------------------------------------------------------

/// Routes external events to the task runner via a channel.
///
/// Manages file watchers and provides a webhook receiver endpoint.
/// Events are deduplicated within a configurable window to prevent
/// rapid-fire triggers from overwhelming the runner.
pub struct TriggerRouter {
    /// Channel sender for dispatching events.
    tx: mpsc::Sender<TriggerEvent>,
    /// Deduplication window: events with the same key within this
    /// duration are suppressed.
    dedup_window: Duration,
    /// Last seen time for each dedup key.
    last_seen: HashMap<String, Instant>,
    /// File watcher handle (kept alive to maintain the watch).
    _watcher: Option<notify::RecommendedWatcher>,
}

impl TriggerRouter {
    /// Create a new trigger router with the given event channel.
    ///
    /// `dedup_window`: minimum time between identical events (default 5s).
    #[must_use]
    pub fn new(tx: mpsc::Sender<TriggerEvent>, dedup_window: Duration) -> Self {
        Self {
            tx,
            dedup_window,
            last_seen: HashMap::new(),
            _watcher: None,
        }
    }

    /// Start watching a set of paths for file changes.
    ///
    /// Changes are sent to the event channel after deduplication.
    ///
    /// # Errors
    ///
    /// Returns an error if the file watcher cannot be initialized.
    pub fn watch_paths(&mut self, paths: &[PathBuf]) -> crate::error::Result<()> {
        use notify::{RecursiveMode, Watcher};

        let tx = self.tx.clone();
        let dedup_window = self.dedup_window;

        // WHY: notify's debounced watcher handles OS-level event coalescing.
        // We add application-level dedup on top for semantic deduplication
        // (e.g., multiple saves to the same file within the window).
        let mut last_seen: HashMap<String, Instant> = HashMap::new();

        let mut watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                let Ok(event) = res else {
                    warn!("file watcher error: {:?}", res.err());
                    return;
                };

                for path in &event.paths {
                    let key = path.display().to_string();
                    let now = Instant::now();

                    // Dedup: skip if we saw this path within the window.
                    if let Some(last) = last_seen.get(&key) {
                        if now.duration_since(*last) < dedup_window {
                            continue;
                        }
                    }
                    last_seen.insert(key, now);

                    let kind = match event.kind {
                        notify::EventKind::Create(_) => FileChangeKind::Created,
                        notify::EventKind::Modify(_) => FileChangeKind::Modified,
                        notify::EventKind::Remove(_) => FileChangeKind::Removed,
                        _ => continue,
                    };

                    let trigger = TriggerEvent::FileChange {
                        path: path.clone(),
                        kind,
                    };

                    // WHY: try_send avoids blocking the watcher thread.
                    // If the channel is full, we drop the event — the runner
                    // will catch up on the next cycle.
                    if tx.try_send(trigger).is_err() {
                        warn!(path = %path.display(), "trigger channel full, dropping event");
                    }
                }
            })
            .map_err(|e| {
                crate::error::TaskFailedSnafu {
                    task_id: "file_watcher",
                    reason: format!("failed to create file watcher: {e}"),
                }
                .build()
            })?;

        for path in paths {
            watcher
                .watch(path, RecursiveMode::Recursive)
                .map_err(|e| {
                    crate::error::TaskFailedSnafu {
                        task_id: "file_watcher",
                        reason: format!("failed to watch {}: {e}", path.display()),
                    }
                    .build()
                })?;
            info!(path = %path.display(), "watching for file changes");
        }

        self._watcher = Some(watcher);
        Ok(())
    }

    /// Dispatch a manual trigger event.
    ///
    /// # Errors
    ///
    /// Returns an error if the event channel is closed.
    pub async fn trigger_manual(&mut self, reason: String) -> crate::error::Result<()> {
        let key = format!("manual:{reason}");
        if self.is_deduped(&key) {
            info!(reason = %reason, "manual trigger deduped");
            return Ok(());
        }

        self.tx
            .send(TriggerEvent::Manual { reason })
            .await
            .map_err(|_| {
                crate::error::TaskFailedSnafu {
                    task_id: "trigger",
                    reason: "event channel closed",
                }
                .build()
            })
    }

    /// Dispatch a webhook trigger event.
    ///
    /// # Errors
    ///
    /// Returns an error if the event channel is closed.
    pub async fn trigger_webhook(
        &mut self,
        endpoint: String,
        payload: serde_json::Value,
    ) -> crate::error::Result<()> {
        let key = format!("webhook:{endpoint}");
        if self.is_deduped(&key) {
            info!(endpoint = %endpoint, "webhook trigger deduped");
            return Ok(());
        }

        self.tx
            .send(TriggerEvent::Webhook { endpoint, payload })
            .await
            .map_err(|_| {
                crate::error::TaskFailedSnafu {
                    task_id: "trigger",
                    reason: "event channel closed",
                }
                .build()
            })
    }

    /// Check dedup and update last_seen.
    fn is_deduped(&mut self, key: &str) -> bool {
        let now = Instant::now();
        if let Some(last) = self.last_seen.get(key) {
            if now.duration_since(*last) < self.dedup_window {
                return true;
            }
        }
        self.last_seen.insert(key.to_owned(), now);
        false
    }
}

/// Create a trigger channel pair (sender for router, receiver for runner).
#[must_use]
pub fn channel(buffer: usize) -> (mpsc::Sender<TriggerEvent>, mpsc::Receiver<TriggerEvent>) {
    mpsc::channel(buffer)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn manual_trigger_sends_event() {
        let (tx, mut rx) = mpsc::channel(16);
        let mut router = TriggerRouter::new(tx, Duration::from_millis(100));

        router.trigger_manual("test reason".to_owned()).await.unwrap();

        let event = rx.recv().await.unwrap();
        assert!(matches!(event, TriggerEvent::Manual { reason } if reason == "test reason"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn webhook_trigger_sends_event() {
        let (tx, mut rx) = mpsc::channel(16);
        let mut router = TriggerRouter::new(tx, Duration::from_millis(100));

        router
            .trigger_webhook(
                "/hooks/test".to_owned(),
                serde_json::json!({"action": "push"}),
            )
            .await
            .unwrap();

        let event = rx.recv().await.unwrap();
        assert!(matches!(event, TriggerEvent::Webhook { endpoint, .. } if endpoint == "/hooks/test"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn dedup_suppresses_rapid_events() {
        let (tx, mut rx) = mpsc::channel(16);
        let mut router = TriggerRouter::new(tx, Duration::from_secs(10)); // long window

        router.trigger_manual("same".to_owned()).await.unwrap();
        router.trigger_manual("same".to_owned()).await.unwrap(); // deduped
        router.trigger_manual("same".to_owned()).await.unwrap(); // deduped

        let event = rx.recv().await.unwrap();
        assert!(matches!(event, TriggerEvent::Manual { .. }));

        // Channel should be empty — only one event passed through dedup.
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn different_keys_not_deduped() {
        let (tx, mut rx) = mpsc::channel(16);
        let mut router = TriggerRouter::new(tx, Duration::from_secs(10));

        router.trigger_manual("reason-a".to_owned()).await.unwrap();
        router.trigger_manual("reason-b".to_owned()).await.unwrap();

        let _a = rx.recv().await.unwrap();
        let _b = rx.recv().await.unwrap();
        // Both should have passed through.
    }

    #[test]
    fn trigger_event_serde_roundtrip() {
        let event = TriggerEvent::FileChange {
            path: PathBuf::from("/tmp/test.rs"),
            kind: FileChangeKind::Modified,
        };
        let json = serde_json::to_string(&event).unwrap();
        let deserialized: TriggerEvent = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, TriggerEvent::FileChange { .. }));
    }
}
