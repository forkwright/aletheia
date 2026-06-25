//! Hot-reload watcher for operator-editable Datalog rule files.
//!
//! Watches a directory for changes to `*.mnm` files and atomically swaps
//! the in-memory [`RuleSet`] via [`arc_swap::ArcSwap`]. Parse errors during
//! reload are fail-closed: the old ruleset is retained and an error is logged.
#![expect(
    clippy::redundant_closure_for_method_calls,
    reason = "poisoned lock recovery pattern used throughout krites runtime"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use notify::{RecommendedWatcher, Watcher};
use snafu::{ResultExt, Snafu};
use tokio::sync::mpsc;
use tracing::{error, info, instrument};

use crate::data::functions::current_validity;
use crate::parse::parse_script;

/// File extension for Datalog rule files loaded by the hot-reloader.
pub(crate) const RULE_EXTENSION: &str = "mnm";

/// Event emitted by the hot-reload watcher.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ReloadEvent {
    /// Rules were successfully reloaded.
    Reloaded {
        /// Number of source files loaded.
        count: usize,
    },
    /// A parse error prevented reload; old ruleset retained.
    ParseError {
        /// Human-readable error message.
        source: String,
    },
    /// The background watcher task terminated unexpectedly.
    TaskDied {
        /// Human-readable reason (panic message or abort signal).
        source: String,
    },
}

/// Metadata for a single loaded rule source.
#[derive(Debug, Clone)]
pub struct RuleSource {
    /// Filename (not full path) of the source file.
    pub filename: String,
    /// UTC timestamp of last successful load.
    pub last_loaded: jiff::Timestamp,
}

impl RuleSource {
    /// Create a new [`RuleSource`] with the given filename and the current UTC timestamp.
    pub fn new(filename: String) -> Self {
        Self {
            filename,
            last_loaded: jiff::Timestamp::now(),
        }
    }
}

/// An atomically-swappable set of Datalog rules loaded from disk.
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    /// Concatenated Datalog rule text from all source files.
    pub rules_text: Arc<str>,
    /// Per-source metadata for health/observability.
    pub sources: Vec<RuleSource>,
    /// Number of source files.
    pub source_count: usize,
}

impl RuleSet {
    /// Build a [`RuleSet`] from validated rule text and the per-file source metadata.
    pub fn new(rules_text: Arc<str>, sources: Vec<RuleSource>) -> Self {
        let source_count = sources.len();
        Self {
            rules_text,
            sources,
            source_count,
        }
    }
}

/// Error type for hot-reload operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum HotReloadError {
    /// Failed to initialize the file watcher.
    #[snafu(display("failed to initialize file watcher"))]
    WatcherInit {
        /// Underlying notify error.
        source: notify::Error,
    },
    /// Failed to read the rule directory.
    #[snafu(display("failed to read rule directory {path}"))]
    ReadDir {
        /// Directory path.
        path: String,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Failed to read a rule file.
    #[snafu(display("failed to read rule file {path}"))]
    ReadFile {
        /// File path.
        path: String,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Rule text failed to parse.
    #[snafu(display("rule parse error: {message}"))]
    Parse {
        /// Parse error message.
        message: String,
    },
}

impl From<HotReloadError> for crate::error::Error {
    fn from(e: HotReloadError) -> Self {
        crate::error::EngineSnafu {
            message: e.to_string(),
        }
        .build()
    }
}

/// Watches a directory for changes to `.mnm` rule files and hot-swaps
/// the in-memory [`RuleSet`] atomically.
#[expect(
    dead_code,
    reason = "fields retained for drop semantics, inspection, and test access"
)]
pub(crate) struct HotReloader {
    rule_dir: PathBuf,
    reload_tx: mpsc::Sender<ReloadEvent>,
    _watcher: notify::RecommendedWatcher,
    /// Handle for the background supervisor task.
    ///
    // WHY: dropped JoinHandles swallow panics silently. Storing the handle
    // keeps the task reachable for observation and ensures its Drop impl runs
    // when the HotReloader is dropped, aborting the supervisor.
    task_handle: tokio::task::JoinHandle<()>,
    /// Abort handle for the inner reload task.
    ///
    // WHY: allows callers (and tests) to terminate the reload loop and
    // observe the resulting TaskDied event through the supervisor.
    reload_abort: tokio::task::AbortHandle,
}

impl HotReloader {
    /// Start watching `rule_dir` for changes to `*.mnm` files.
    ///
    /// Returns the [`HotReloader`] handle, a [`Receiver`] of [`ReloadEvent`]s,
    /// and the atomically-swappable [`RuleSet`].
    ///
    /// The caller should retain the returned `HotReloader` for the lifetime of
    /// the watch; dropping it stops the background watcher task and cleans up
    /// OS-level file watches.
    ///
    /// # Errors
    ///
    /// Returns an error if the file watcher cannot be initialized.
    #[expect(
        clippy::type_complexity,
        reason = "fixed_rules mirror type from Db runtime"
    )]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "called from tests; not yet wired into non-test Db startup"
        )
    )]
    pub fn start(
        rule_dir: impl AsRef<Path>,
        fixed_rules: &Arc<
            crossbeam::sync::ShardedLock<BTreeMap<String, Arc<Box<dyn crate::FixedRule>>>>,
        >,
    ) -> Result<(Self, mpsc::Receiver<ReloadEvent>, Arc<ArcSwap<RuleSet>>), HotReloadError> {
        let rule_dir = rule_dir.as_ref().to_path_buf();

        let initial_ruleset = Self::load_ruleset(&rule_dir, fixed_rules)?;
        let rule_store = Arc::new(ArcSwap::new(Arc::new(initial_ruleset)));

        let (reload_tx, reload_rx) = mpsc::channel::<ReloadEvent>(8);
        // WHY: capacity 1 is enough for debounced coalescing; duplicate tokens
        // are noise and can be dropped without losing a reload signal.
        let (notify_tx, mut notify_rx) = mpsc::channel::<()>(1);
        let supervisor_reload_tx = reload_tx.clone();

        let watcher = {
            let notify_tx = notify_tx.clone();
            RecommendedWatcher::new(
                move |res: Result<notify::Event, notify::Error>| {
                    if let Ok(event) = res
                        && (event.kind.is_modify()
                            || event.kind.is_create()
                            || event.kind.is_remove())
                    {
                        // WHY: drop duplicate tokens on a full channel. The
                        // debounce loop coalesces all tokens into one reload,
                        // so an already-pending signal is sufficient.
                        let _ = notify_tx.try_send(());
                    }
                },
                notify::Config::default(),
            )
            .context(WatcherInitSnafu)?
        };

        let mut watcher = watcher;
        watcher
            .watch(&rule_dir, notify::RecursiveMode::NonRecursive)
            .context(WatcherInitSnafu)?;

        let rule_store_clone = Arc::clone(&rule_store);
        let rule_dir_clone = rule_dir.clone();
        let fixed_rules_clone = Arc::clone(fixed_rules);
        let reload_tx_clone = reload_tx.clone();

        let reload_task = tokio::spawn(async move {
            let debounce = Duration::from_millis(500);

            while notify_rx.recv().await.is_some() {
                // WHY: coalesce bursts of notify events into a single reload.
                tokio::time::sleep(debounce).await;
                while notify_rx.try_recv().is_ok() {}

                match HotReloader::load_ruleset(&rule_dir_clone, &fixed_rules_clone) {
                    Ok(ruleset) => {
                        let count = ruleset.source_count;
                        rule_store_clone.store(Arc::new(ruleset));
                        info!(rules = %count, "krites hot-reload complete");
                        let _ = reload_tx_clone.send(ReloadEvent::Reloaded { count }).await;
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        error!(error = %msg, "krites hot-reload failed; keeping old ruleset");
                        let _ = reload_tx_clone
                            .send(ReloadEvent::ParseError { source: msg })
                            .await;
                    }
                }
            }
        });
        let reload_abort = reload_task.abort_handle();

        // WHY: supervisor observes the reload task so panics are surfaced
        // through the ReloadEvent channel instead of vanishing silently.
        let task_handle = tokio::spawn(async move {
            if let Err(e) = reload_task.await {
                error!(error = %e, "krites hot-reload task died");
                let msg = e.to_string();
                let _ = supervisor_reload_tx
                    .send(ReloadEvent::TaskDied { source: msg })
                    .await;
            }
        });

        Ok((
            Self {
                rule_dir,
                reload_tx,
                _watcher: watcher,
                task_handle,
                reload_abort,
            },
            reload_rx,
            rule_store,
        ))
    }

    #[cfg(test)]
    pub(crate) fn abort_for_test(&self) {
        // Abort the inner reload task so the supervisor observes the JoinError
        // and emits a TaskDied event.
        self.reload_abort.abort();
    }

    /// Load and validate all `*.mnm` files in `rule_dir`.
    ///
    /// # Errors
    ///
    /// Returns an error if directory or file reading fails, or if the
    /// concatenated rule text does not parse.
    #[instrument(skip(fixed_rules))]
    fn load_ruleset(
        rule_dir: &Path,
        fixed_rules: &crossbeam::sync::ShardedLock<
            BTreeMap<String, Arc<Box<dyn crate::FixedRule>>>,
        >,
    ) -> Result<RuleSet, HotReloadError> {
        let mut texts = Vec::new();
        let mut sources = Vec::new();

        let mut entries = std::fs::read_dir(rule_dir)
            .context(ReadDirSnafu {
                path: rule_dir.display().to_string(),
            })?
            .collect::<Result<Vec<_>, _>>()
            .context(ReadDirSnafu {
                path: rule_dir.display().to_string(),
            })?;

        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            let Some(ext) = path.extension() else {
                continue;
            };
            if ext != RULE_EXTENSION {
                continue;
            }

            let text = std::fs::read_to_string(&path).context(ReadFileSnafu {
                path: path.display().to_string(),
            })?;

            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            texts.push(text);
            sources.push(RuleSource::new(filename));
        }

        let rules_text: Arc<str> = texts.join("\n").into();

        // WHY: parse to validate before the swap; a parse failure keeps the old ruleset.
        if !texts.is_empty() {
            let fixed_guard = fixed_rules.read().unwrap_or_else(|e| e.into_inner());
            parse_script(
                &rules_text,
                &BTreeMap::new(),
                &fixed_guard,
                current_validity(),
            )
            .map_err(|e| HotReloadError::Parse {
                message: e.to_string(),
            })?;
        }

        Ok(RuleSet::new(rules_text, sources))
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
#[expect(
    clippy::disallowed_methods,
    reason = "test-only temp file creation outside storage abstraction"
)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use tokio::time::timeout;

    use super::{HotReloader, ReloadEvent};
    use crate::runtime::db::Db;
    use crate::storage::mem::MemStorage;

    fn fresh_mem_db() -> Db<MemStorage> {
        crate::storage::mem::new_mem_db().unwrap()
    }

    async fn wait_for_event(rx: &mut tokio::sync::mpsc::Receiver<ReloadEvent>) -> ReloadEvent {
        timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("timed out waiting for reload event")
            .expect("reload channel closed")
    }

    #[tokio::test]
    async fn hot_reload_swaps_ruleset_on_file_change() {
        let dir = tempfile::tempdir().unwrap();
        let rule_dir = dir.path();

        std::fs::write(rule_dir.join("test.mnm"), "rule_a[x] := x = 1\n").unwrap();

        let db = fresh_mem_db();
        let fixed_rules = Arc::clone(&db.fixed_rules);
        let (_reloader, mut rx, store) = HotReloader::start(rule_dir, &fixed_rules).unwrap();

        assert_eq!(store.load().source_count, 1);
        assert!(store.load().rules_text.contains("rule_a"));

        // WHY: some notify implementations emit an event on start; drain it first.
        let _ = timeout(Duration::from_millis(100), rx.recv()).await;

        std::fs::write(rule_dir.join("test.mnm"), "rule_b[x] := x = 2\n").unwrap();

        let event = wait_for_event(&mut rx).await;
        match event {
            ReloadEvent::Reloaded { count } => assert_eq!(count, 1),
            ReloadEvent::ParseError { source } => panic!("unexpected parse error: {source}"),
            ReloadEvent::TaskDied { source } => panic!("unexpected task death: {source}"),
        }

        assert!(store.load().rules_text.contains("rule_b"));
    }

    #[tokio::test]
    async fn hot_reload_preserves_old_ruleset_on_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let rule_dir = dir.path();

        std::fs::write(rule_dir.join("good.mnm"), "good_rule[x] := x = 1\n").unwrap();

        let db = fresh_mem_db();
        let fixed_rules = Arc::clone(&db.fixed_rules);
        let (_reloader, mut rx, store) = HotReloader::start(rule_dir, &fixed_rules).unwrap();

        let _ = timeout(Duration::from_millis(100), rx.recv()).await;

        std::fs::write(rule_dir.join("good.mnm"), "this is not valid datalog!!!\n").unwrap();

        let event = wait_for_event(&mut rx).await;
        match event {
            ReloadEvent::Reloaded { .. } => panic!("expected parse error"),
            ReloadEvent::ParseError { .. } => {}
            ReloadEvent::TaskDied { source } => panic!("unexpected task death: {source}"),
        }

        assert!(store.load().rules_text.contains("good_rule"));
    }

    #[tokio::test]
    async fn debounce_coalesces_rapid_changes() {
        let dir = tempfile::tempdir().unwrap();
        let rule_dir = dir.path();

        std::fs::write(rule_dir.join("test.mnm"), "rule[x] := x = 1\n").unwrap();

        let db = fresh_mem_db();
        let fixed_rules = Arc::clone(&db.fixed_rules);
        let (_reloader, mut rx, store) = HotReloader::start(rule_dir, &fixed_rules).unwrap();

        let _ = timeout(Duration::from_millis(100), rx.recv()).await;

        // Write all three files without sleeping — they all land inside the same
        // debounce window. Virtual time is advanced automatically by Tokio once
        // every task is blocked on a timer.
        for i in 0..3 {
            std::fs::write(rule_dir.join("test.mnm"), format!("rule[x] := x = {i}\n")).unwrap();
        }

        let event = wait_for_event(&mut rx).await;
        match event {
            ReloadEvent::Reloaded { count } => assert_eq!(count, 1),
            ReloadEvent::ParseError { source } => panic!("unexpected parse error: {source}"),
            ReloadEvent::TaskDied { source } => panic!("unexpected task death: {source}"),
        }

        // Because of debounce, the final value should reflect the last write.
        assert!(store.load().rules_text.contains("x = 2"));

        let second = timeout(Duration::from_millis(300), rx.recv()).await;
        assert!(
            second.is_err() || second.unwrap().is_none(),
            "expected no second event after debounce"
        );
    }

    #[tokio::test]
    async fn aborting_reloader_stops_background_task_and_does_not_respawn() {
        let dir = tempfile::tempdir().unwrap();
        let rule_dir = dir.path();

        std::fs::write(rule_dir.join("test.mnm"), "rule[x] := x = 0\n").unwrap();

        let db = fresh_mem_db();
        let fixed_rules = Arc::clone(&db.fixed_rules);
        let (reloader, mut rx, _store) = HotReloader::start(rule_dir, &fixed_rules).unwrap();

        let _ = timeout(Duration::from_millis(100), rx.recv()).await;

        // WHY: abort simulates task death; the stored handle makes this
        // observable and guarantees cleanup instead of leaking the task.
        reloader.abort_for_test();

        // Drain the TaskDied event that the supervisor emits on abort.
        let event = timeout(Duration::from_millis(200), rx.recv()).await;
        assert!(
            matches!(event, Ok(Some(ReloadEvent::TaskDied { .. }))),
            "expected TaskDied after abort, got {event:?}"
        );

        // After task death, further file changes must not produce reloads.
        std::fs::write(rule_dir.join("test.mnm"), "rule[x] := x = 999\n").unwrap();
        let late = timeout(Duration::from_millis(300), rx.recv()).await;
        assert!(
            late.is_err() || late.unwrap().is_none(),
            "expected no reload events after the reloader task was aborted"
        );
    }
}
