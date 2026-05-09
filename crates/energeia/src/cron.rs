//! Cron scheduler for recurring dispatch tasks.
//!
//! Uses the standard `cron` crate with chrono datetimes, plus fjall-backed
//! distributed locking to prevent duplicate fires across restarts.
//!
//! # Observability
//!
//! ## Events
//! | Event | Level | Fields | Condition |
//! |-------|-------|--------|-----------|
//! | `cron.task.fired` | info | `task_name`, `scheduled` | Lock acquired and callback invoked |
//! | `cron.task.skipped` | debug | `task_name`, `scheduled` | Lock already held for this window |
//! | `cron.lock.failed` | error | `task_name`, `error` | Fjall I/O failure during lock acquisition |
//! | `cron.sleep` | info | `task_name`, `next`, `sleep_ms` | Scheduler computed next wake time |
//! | `cron.shutdown` | info | | Cancellation token triggered |

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use compact_str::CompactString;
use fjall::Readable;
use rand::RngExt;
use snafu::IntoError;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::error::{self, Result};
use crate::types::DispatchSpec;

// ---------------------------------------------------------------------------
// CronTask
// ---------------------------------------------------------------------------

/// A single recurring dispatch task.
#[derive(Debug, Clone)]
pub struct CronTask {
    /// Unique task identifier.
    pub name: CompactString,
    /// Cron schedule.
    pub cron: cron::Schedule,
    /// Maximum jitter to apply (+/- this duration).
    pub jitter: Duration,
    /// What to dispatch when this task fires.
    pub dispatch_spec: DispatchSpec,
}

impl CronTask {
    /// Create a new cron task, parsing the schedule expression.
    ///
    /// # Errors
    ///
    /// Returns [`Error::CronParse`] if `schedule` is not a valid cron expression.
    pub fn new(
        name: impl Into<CompactString>,
        schedule: &str,
        jitter: Duration,
        dispatch_spec: DispatchSpec,
    ) -> Result<Self> {
        let cron = schedule.parse().map_err(|e| {
            error::CronParseSnafu {
                expression: schedule.to_owned(),
            }
            .into_error(e)
        })?;
        Ok(Self {
            name: name.into(),
            cron,
            jitter,
            dispatch_spec,
        })
    }
}

// ---------------------------------------------------------------------------
// CronLockStore
// ---------------------------------------------------------------------------

/// Partition name for cron lock records.
const LOCK_PARTITION: &str = "cron_locks";

/// Fjall-backed lock store that persists the last-fired timestamp per task.
///
/// A mutex serializes lock acquisition within a single process; the fjall
/// write provides cross-restart deduplication.
pub struct CronLockStore {
    db: Arc<fjall::SingleWriterTxDatabase>,
    lock: parking_lot::Mutex<()>,
}

impl CronLockStore {
    /// Open the lock store inside the given fjall database.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Store`] if the partition cannot be opened.
    pub fn open(db: Arc<fjall::SingleWriterTxDatabase>) -> Result<Self> {
        db.keyspace(LOCK_PARTITION, fjall::KeyspaceCreateOptions::default)
            .map_err(|e| store_err("open cron_locks partition", e))?;
        Ok(Self {
            db,
            lock: parking_lot::Mutex::new(()),
        })
    }

    /// Attempt to acquire the fire lock for `task_name`.
    ///
    /// Returns `true` if the lock was acquired (no previous fire at or after
    /// `scheduled_time`). On success, persists `scheduled_time` as the last-fired
    /// timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Store`] on fjall I/O failure.
    pub fn try_acquire(&self, task_name: &str, scheduled_time: DateTime<Utc>) -> Result<bool> {
        let _guard = self.lock.lock();
        let existing = self.get_last_fired(task_name)?;
        if let Some(ts) = existing
            && ts >= scheduled_time
        {
            return Ok(false);
        }
        let partition = self
            .db
            .keyspace(LOCK_PARTITION, fjall::KeyspaceCreateOptions::default)
            .map_err(|e| store_err("open cron_locks partition", e))?;
        let value = scheduled_time.to_rfc3339();
        let mut tx = self.db.write_tx();
        tx.insert(&partition, task_name.as_bytes(), value.as_bytes());
        tx.commit().map_err(|e| store_err("commit cron lock", e))?;
        Ok(true)
    }

    /// Read the last-fired timestamp for a task, if any.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Store`] on fjall I/O failure.
    pub fn last_fired(&self, task_name: &str) -> Result<Option<DateTime<Utc>>> {
        self.get_last_fired(task_name)
    }

    fn get_last_fired(&self, task_name: &str) -> Result<Option<DateTime<Utc>>> {
        let partition = self
            .db
            .keyspace(LOCK_PARTITION, fjall::KeyspaceCreateOptions::default)
            .map_err(|e| store_err("open cron_locks partition", e))?;
        let snap = self.db.read_tx();
        match snap
            .get(&partition, task_name.as_bytes())
            .map(|opt| opt.map(|s| s.to_vec()))
            .map_err(|e| store_err("read cron lock", e))?
        {
            Some(bytes) => {
                let s = std::str::from_utf8(&bytes).map_err(|e| {
                    error::StoreSnafu {
                        message: format!("invalid UTF-8 in cron lock for {task_name}: {e}"),
                    }
                    .build()
                })?;
                let dt = DateTime::parse_from_rfc3339(s)
                    .map_err(|e| {
                        error::StoreSnafu {
                            message: format!("invalid RFC 3339 in cron lock for {task_name}: {e}"),
                        }
                        .build()
                    })?
                    .with_timezone(&Utc);
                Ok(Some(dt))
            }
            None => Ok(None),
        }
    }
}

// ---------------------------------------------------------------------------
// CronScheduler
// ---------------------------------------------------------------------------

/// Scheduler that manages a set of [`CronTask`]s with fjall-backed locking.
pub struct CronScheduler {
    tasks: Vec<CronTask>,
    lock_store: Arc<CronLockStore>,
}

impl CronScheduler {
    /// Create a new scheduler.
    #[must_use]
    pub fn new(tasks: Vec<CronTask>, lock_store: Arc<CronLockStore>) -> Self {
        Self { tasks, lock_store }
    }

    /// Compute the next fire time for a task after `now`.
    ///
    /// Returns `None` if the schedule has no future occurrences.
    #[must_use]
    pub fn next_fire_after(&self, task: &CronTask, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        task.cron.after(&now).next()
    }

    /// Run the scheduler loop until `cancel` is triggered.
    ///
    /// For each due task, the lock is acquired; if successful, `on_fire` is
    /// invoked. Jitter is applied to the computed next-fire time before
    /// sleeping.
    ///
    /// # Cancel safety
    ///
    /// Cancel-safe at the sleep boundary. Dropping the future during sleep
    /// has no side effects.
    pub async fn run<F, Fut>(&self, cancel: CancellationToken, on_fire: F) -> Result<()>
    where
        F: Fn(CronTask) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        loop {
            let now = Utc::now();
            let mut earliest: Option<(DateTime<Utc>, &CronTask, DateTime<Utc>)> = None;

            for task in &self.tasks {
                if let Some(base) = self.next_fire_after(task, now) {
                    let jittered = apply_jitter(base, task.jitter);
                    if let Some((ref best_time, _, _)) = earliest {
                        if jittered < *best_time {
                            earliest = Some((jittered, task, base));
                        }
                    } else {
                        earliest = Some((jittered, task, base));
                    }
                }
            }

            let Some((jittered_next, task, base_scheduled)) = earliest else {
                tracing::info!("no scheduled cron tasks; exiting scheduler loop");
                return Ok(());
            };

            let sleep_duration = (jittered_next - now).to_std().unwrap_or(Duration::ZERO);

            tracing::info!(
                task = %task.name,
                next = %jittered_next,
                base = %base_scheduled,
                sleep_ms = sleep_duration.as_millis(),
                "cron scheduler sleeping"
            );

            tokio::select! {
                biased;
                () = cancel.cancelled() => {
                    tracing::info!("cron scheduler shutting down");
                    return Ok(());
                }
                () = tokio::time::sleep(sleep_duration) => {}
            }

            let fire_now = Utc::now();
            if fire_now < jittered_next {
                // Time may have shifted backward or sleep fired early; recompute.
                continue;
            }

            match self
                .lock_store
                .try_acquire(task.name.as_str(), base_scheduled)
            {
                Ok(true) => {
                    tracing::info!(
                        task = %task.name,
                        scheduled = %base_scheduled,
                        "cron task fired"
                    );
                    let span = tracing::info_span!("cron_fire", task = %task.name);
                    let callback = on_fire.clone();
                    let task = task.clone();
                    tokio::spawn(async move {
                        callback(task).instrument(span).await;
                    });
                }
                Ok(false) => {
                    tracing::debug!(
                        task = %task.name,
                        scheduled = %base_scheduled,
                        "cron task skipped — lock held"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        task = %task.name,
                        error = %e,
                        "cron lock acquisition failed"
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn store_err(context: &str, e: impl std::fmt::Display) -> error::Error {
    error::StoreSnafu {
        message: format!("{context}: {e}"),
    }
    .build()
}

/// Apply a random signed jitter to a base timestamp.
///
/// The offset is uniformly distributed in `[-jitter, +jitter]`.
fn apply_jitter(base: DateTime<Utc>, jitter: Duration) -> DateTime<Utc> {
    if jitter.is_zero() {
        return base;
    }
    let max_secs = i64::try_from(jitter.as_secs()).unwrap_or(i64::MAX);
    let offset_secs = rand::rng().random_range(-max_secs..=max_secs);
    base + chrono::Duration::seconds(offset_secs)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::TimeZone;

    use super::*;

    fn parse_schedule(expr: &str) -> cron::Schedule {
        expr.parse().expect("valid cron expression")
    }

    fn dummy_lock_store() -> CronLockStore {
        let db = koina::fjall::FjallDb::open_temp(&[LOCK_PARTITION]).unwrap();
        CronLockStore::open(Arc::new(db.db)).unwrap()
    }

    #[test]
    fn cron_schedule_parses_standard_expressions() {
        let _ = parse_schedule("0 0 2 * * *");
        let _ = parse_schedule("0 */15 * * * *");
        let _ = parse_schedule("0 0 0 * * 1-5");
    }

    #[test]
    fn next_fire_after_produces_future_timestamp() {
        let task = CronTask {
            name: CompactString::new("test"),
            cron: parse_schedule("0 0 0 * * *"),
            jitter: Duration::ZERO,
            dispatch_spec: DispatchSpec::new("test".to_owned(), vec![]),
        };
        let now = Utc::now();
        let next =
            CronScheduler::new(vec![], Arc::new(dummy_lock_store())).next_fire_after(&task, now);
        assert!(next.is_some(), "daily cron should have a next occurrence");
        assert!(next.unwrap() > now);
    }

    #[test]
    fn next_fire_after_hourly_boundary() {
        let task = CronTask {
            name: CompactString::new("hourly"),
            cron: parse_schedule("0 0 * * * *"),
            jitter: Duration::ZERO,
            dispatch_spec: DispatchSpec::new("test".to_owned(), vec![]),
        };
        let scheduler = CronScheduler::new(vec![], Arc::new(dummy_lock_store()));
        let now = Utc.with_ymd_and_hms(2026, 4, 17, 12, 30, 0).unwrap();
        let next = scheduler.next_fire_after(&task, now);
        assert_eq!(
            next,
            Some(Utc.with_ymd_and_hms(2026, 4, 17, 13, 0, 0).unwrap())
        );
    }

    #[test]
    fn jitter_applies_signed_offset() {
        let base = Utc.with_ymd_and_hms(2026, 4, 17, 12, 0, 0).unwrap();
        let jitter = Duration::from_mins(5);
        let mut seen_different = false;
        for _ in 0..100 {
            let result = apply_jitter(base, jitter);
            let diff = (result - base).num_seconds().abs();
            assert!(diff <= 300, "jitter offset {diff} exceeds max 300");
            if result != base {
                seen_different = true;
            }
        }
        assert!(seen_different, "jitter should vary over 100 samples");
    }

    #[tokio::test]
    async fn lock_prevents_duplicate_fire() {
        let db = koina::fjall::FjallDb::open_temp(&[LOCK_PARTITION]).unwrap();
        let lock_store = Arc::new(CronLockStore::open(Arc::new(db.db)).unwrap());
        let task = CronTask {
            name: CompactString::new("dedup-test"),
            cron: parse_schedule("* * * * * *"),
            jitter: Duration::ZERO,
            dispatch_spec: DispatchSpec::new("test".to_owned(), vec![]),
        };

        let fired = Arc::new(AtomicUsize::new(0));
        let scheduler1 = CronScheduler::new(vec![task.clone()], Arc::clone(&lock_store));
        let scheduler2 = CronScheduler::new(vec![task.clone()], Arc::clone(&lock_store));

        let cancel1 = CancellationToken::new();
        let cancel2 = CancellationToken::new();

        let f1 = fired.clone();
        let handle1 = tokio::spawn(async move {
            scheduler1
                .run(cancel1, move |_task| {
                    let f = f1.clone();
                    async move {
                        f.fetch_add(1, Ordering::SeqCst);
                    }
                })
                .await
        });

        let f2 = fired.clone();
        let handle2 = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            scheduler2
                .run(cancel2, move |_task| {
                    let f = f2.clone();
                    async move {
                        f.fetch_add(1, Ordering::SeqCst);
                    }
                })
                .await
        });

        tokio::time::sleep(Duration::from_secs(2)).await;
        handle1.abort();
        handle2.abort();
        let _ = handle1.await;
        let _ = handle2.await;

        let total = fired.load(Ordering::SeqCst);
        assert!(total >= 1, "at least one scheduler should have fired");
        // With a 1-second cron and a 2-second window, each scheduler could fire
        // twice. Because they share the lock store, total fires should be
        // bounded (allowing one extra for edge timing).
        assert!(
            total <= 3,
            "lock should prevent excessive duplicate fires, got {total}"
        );
    }

    #[tokio::test]
    async fn slow_on_fire_does_not_block_scheduler_ticks() {
        let db = koina::fjall::FjallDb::open_temp(&[LOCK_PARTITION]).unwrap();
        let lock_store = Arc::new(CronLockStore::open(Arc::new(db.db)).unwrap());
        let task = CronTask {
            name: CompactString::new("slow-callback"),
            cron: parse_schedule("* * * * * *"),
            jitter: Duration::ZERO,
            dispatch_spec: DispatchSpec::new("test".to_owned(), vec![]),
        };
        let fired = Arc::new(AtomicUsize::new(0));
        let scheduler = CronScheduler::new(vec![task], lock_store);
        let cancel = CancellationToken::new();
        let cancel_for_task = cancel.clone();
        let fired_for_task = Arc::clone(&fired);

        let handle = tokio::spawn(async move {
            scheduler
                .run(cancel_for_task, move |_task| {
                    let fired = Arc::clone(&fired_for_task);
                    async move {
                        fired.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                })
                .await
        });

        tokio::time::sleep(Duration::from_millis(2_500)).await;
        cancel.cancel();
        handle.await.unwrap().unwrap();

        assert!(
            fired.load(Ordering::SeqCst) >= 2,
            "slow callback should not block later cron ticks"
        );
    }

    #[test]
    fn try_acquire_allows_first_fire() {
        let store = dummy_lock_store();
        let now = Utc::now();
        assert!(store.try_acquire("task-a", now).unwrap());
    }

    #[test]
    fn try_acquire_denies_repeat_within_same_window() {
        let store = dummy_lock_store();
        let now = Utc::now();
        assert!(store.try_acquire("task-b", now).unwrap());
        assert!(!store.try_acquire("task-b", now).unwrap());
    }

    #[test]
    fn try_acquire_allows_next_window() {
        let store = dummy_lock_store();
        let t1 = Utc::now();
        let t2 = t1 + chrono::Duration::hours(1);
        assert!(store.try_acquire("task-c", t1).unwrap());
        assert!(store.try_acquire("task-c", t2).unwrap());
    }

    #[test]
    fn last_fired_roundtrip() {
        let store = dummy_lock_store();
        let now = Utc.with_ymd_and_hms(2026, 4, 17, 10, 0, 0).unwrap();
        assert!(store.last_fired("task-d").unwrap().is_none());
        store.try_acquire("task-d", now).unwrap();
        let read = store.last_fired("task-d").unwrap();
        assert_eq!(read, Some(now));
    }
}
