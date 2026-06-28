//! Real-time status dashboard for energeia dispatch operations.
//!
//! Provides a point-in-time snapshot of:
//! - Active (running) dispatches and their session counts
//! - Queue depth (number of running dispatches)
//! - Recent dispatch outcomes (last N dispatches, newest first)
//! - Per-project summary of active and recent activity
//! - Crash-recovery counters for stale dispatches and cron fires

#[cfg(feature = "storage-fjall")]
use std::collections::HashMap;

#[cfg(feature = "storage-fjall")]
use crate::cron::{CronFireRecord, CronLockStore};
#[cfg(feature = "storage-fjall")]
use crate::error;
#[cfg(feature = "storage-fjall")]
use crate::error::Result;
#[cfg(feature = "storage-fjall")]
use crate::store::records::{DispatchRecord, DispatchStatus, SessionRecord};
#[cfg(feature = "storage-fjall")]
use crate::store::{
    EnergeiaStore, SCAN_LIMIT_DISPATCHES, SCAN_LIMIT_SESSIONS, stale_running_dispatch_threshold,
};

/// Point-in-time status dashboard.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct StatusDashboard {
    /// When this snapshot was taken.
    pub computed_at: jiff::Timestamp,
    /// Number of dispatches currently in `Running` state.
    pub active_dispatches: u64,
    /// Alias for `active_dispatches`; number of prompts awaiting completion.
    pub queue_depth: u64,
    /// Number of running dispatches older than the startup reconciliation threshold.
    pub stale_running_dispatches: u64,
    /// Recent dispatch outcomes (newest first), up to `RECENT_LIMIT`.
    pub recent_outcomes: Vec<RecentOutcome>,
    /// Per-project summary aggregated from active and recent dispatches.
    pub by_project: Vec<ProjectSummary>,
    /// Cron fire crash-recovery status when cron state is available.
    #[cfg(feature = "storage-fjall")]
    pub cron: Option<CronStatus>,
}

/// Cron fire crash-recovery status.
#[cfg(feature = "storage-fjall")]
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CronStatus {
    /// Number of unfinished cron fires older than the stale threshold.
    pub stale_fire_count: u64,
    /// Last recorded fire for each configured task.
    pub task_fires: Vec<CronTaskFireStatus>,
}

/// Last persisted fire state for one cron task.
#[cfg(feature = "storage-fjall")]
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CronTaskFireStatus {
    /// Cron task name.
    pub task_name: String,
    /// Last persisted fire record, if this task has ever acquired a fire lock.
    pub last_fire_record: Option<CronFireRecord>,
}

/// Summary of a single dispatch run for the recent history list.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RecentOutcome {
    /// Dispatch identifier.
    // kanon:ignore RUST/primitive-for-domain-id — public metrics summary type; changing to newtype would be a breaking API change
    pub dispatch_id: String,
    /// Project this dispatch belongs to.
    pub project: String,
    /// Current lifecycle status.
    pub status: String,
    /// When the dispatch was created.
    pub started_at: jiff::Timestamp,
    /// When the dispatch finished (`None` if still running).
    pub finished_at: Option<jiff::Timestamp>,
    /// Number of sessions in this dispatch.
    pub total_sessions: u32,
    /// Total cost in USD across all sessions.
    pub total_cost_usd: f64,
}

/// Per-project status summary.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ProjectSummary {
    /// Project identifier.
    pub project: String,
    /// Number of currently running dispatches.
    pub active_dispatches: u64,
    /// Total sessions across all dispatches in the recent window.
    pub total_sessions: u64,
    /// Total cost in USD across the recent window.
    pub total_cost_usd: f64,
    /// Fraction of completed dispatches in the recent window (0.0–1.0).
    pub success_rate: f64,
}

/// How many recent dispatches to include in [`StatusDashboard::recent_outcomes`].
#[cfg(feature = "storage-fjall")]
const RECENT_LIMIT: usize = 50;

#[cfg(feature = "storage-fjall")]
/// Build a real-time status dashboard snapshot.
///
/// Scans all dispatches and sessions. The `recent_outcomes` list contains the
/// most recent `50` dispatches sorted newest-first.
///
/// # Errors
///
/// Returns `Error::Store` if any underlying store read fails.
pub fn compute_status_dashboard(store: &EnergeiaStore) -> Result<StatusDashboard> {
    compute_status_dashboard_inner(store, None)
}

#[cfg(feature = "storage-fjall")]
/// Build a real-time status dashboard snapshot with cron fire state.
///
/// # Errors
///
/// Returns `Error::Store` if any underlying store read fails.
pub fn compute_status_dashboard_with_cron(
    store: &EnergeiaStore,
    cron_lock_store: &CronLockStore,
    cron_task_names: &[String],
) -> Result<StatusDashboard> {
    compute_status_dashboard_inner(store, Some((cron_lock_store, cron_task_names)))
}

#[cfg(feature = "storage-fjall")]
fn compute_status_dashboard_inner(
    store: &EnergeiaStore,
    cron: Option<(&CronLockStore, &[String])>,
) -> Result<StatusDashboard> {
    let now = jiff::Timestamp::now();

    let all_dispatches = store.list_dispatches(SCAN_LIMIT_DISPATCHES)?;
    let recent_dispatches = store.list_recent_dispatches(RECENT_LIMIT)?;
    let all_sessions = store.list_all_sessions(SCAN_LIMIT_SESSIONS)?;
    let stale_running_dispatches =
        u64::from(store.stale_running_dispatch_count(stale_running_dispatch_threshold())?);

    let mut sessions_by_dispatch: HashMap<&str, Vec<&SessionRecord>> = HashMap::new();
    for s in &all_sessions {
        sessions_by_dispatch
            .entry(s.dispatch_id.as_str())
            .or_default()
            .push(s);
    }

    let active_dispatch_records: Vec<&DispatchRecord> = all_dispatches
        .iter()
        .filter(|d| d.status == DispatchStatus::Running)
        .collect();

    #[expect(
        clippy::as_conversions,
        reason = "usize to u64: active dispatch count bounded by SCAN_LIMIT_DISPATCHES"
    )]
    let active_dispatches = active_dispatch_records.len() as u64;
    let queue_depth = active_dispatches;

    let recent_outcomes: Vec<RecentOutcome> = recent_dispatches
        .iter()
        .map(|d| RecentOutcome {
            dispatch_id: d.id.as_str().to_owned(),
            project: d.project.clone(),
            status: d.status.to_string(),
            started_at: d.created_at,
            finished_at: d.finished_at,
            total_sessions: d.total_sessions,
            total_cost_usd: d.total_cost_usd,
        })
        .collect();

    let by_project = build_project_summaries(&all_dispatches, &sessions_by_dispatch);
    let cron = cron
        .map(|(lock_store, task_names)| build_cron_status(now, lock_store, task_names))
        .transpose()?;

    Ok(StatusDashboard {
        computed_at: now,
        active_dispatches,
        queue_depth,
        stale_running_dispatches,
        recent_outcomes,
        by_project,
        cron,
    })
}

#[cfg(feature = "storage-fjall")]
fn build_cron_status(
    now: jiff::Timestamp,
    lock_store: &CronLockStore,
    task_names: &[String],
) -> Result<CronStatus> {
    let started_before = now
        .checked_sub(stale_running_dispatch_threshold())
        .map_err(|e| {
            error::StoreSnafu {
                message: format!("duration subtraction in cron status: {e}"),
            }
            .build()
        })?;
    let stale_fire_count = u64::from(lock_store.stale_fire_count(started_before)?);
    let mut task_fires = Vec::with_capacity(task_names.len());
    for task_name in task_names {
        task_fires.push(CronTaskFireStatus {
            task_name: task_name.clone(),
            last_fire_record: lock_store.last_fire_record(task_name)?,
        });
    }
    Ok(CronStatus {
        stale_fire_count,
        task_fires,
    })
}

#[cfg(feature = "storage-fjall")]
fn build_project_summaries(
    dispatches: &[DispatchRecord],
    sessions_by_dispatch: &HashMap<&str, Vec<&SessionRecord>>,
) -> Vec<ProjectSummary> {
    #[derive(Default)]
    struct Acc {
        active: u64,
        sessions: u64,
        cost_usd: f64,
        total: u64,
        completed: u64,
    }

    let mut by_project: HashMap<&str, Acc> = HashMap::new();

    for d in dispatches {
        let acc = by_project.entry(d.project.as_str()).or_default();
        acc.total += 1;
        acc.cost_usd += d.total_cost_usd;

        if d.status == DispatchStatus::Running {
            acc.active += 1;
        }
        if d.status == DispatchStatus::Completed {
            acc.completed += 1;
        }

        let session_count = sessions_by_dispatch.get(d.id.as_str()).map_or(0, Vec::len);

        #[expect(
            clippy::as_conversions,
            reason = "usize to u64: session count bounded by SCAN_LIMIT_SESSIONS"
        )]
        {
            acc.sessions += session_count as u64;
        }
    }

    let mut result: Vec<ProjectSummary> = by_project
        .into_iter()
        .map(|(project, acc)| {
            let success_rate = if acc.total > 0 {
                #[expect(
                    clippy::cast_precision_loss,
                    clippy::as_conversions,
                    reason = "u64→f64: both counts bounded by SCAN_LIMIT_DISPATCHES (10_000), well below f64 mantissa 2^53"
                )]
                {
                    acc.completed as f64 / acc.total as f64 // SAFETY: counts bounded by SCAN_LIMIT_DISPATCHES
                }
            } else {
                0.0
            };
            ProjectSummary {
                project: project.to_owned(),
                active_dispatches: acc.active,
                total_sessions: acc.sessions,
                total_cost_usd: acc.cost_usd,
                success_rate,
            }
        })
        .collect();

    result.sort_by(|a, b| a.project.cmp(&b.project));
    result
}

#[cfg(test)]
#[cfg(feature = "storage-fjall")]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::float_cmp, reason = "test assertions on exact float values")]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::cron::{CronFireRecord, CronLockStore};
    use crate::store::EnergeiaStore;
    use crate::store::records::{DispatchId, DispatchRecord, DispatchStatus, SessionUpdate};
    use crate::types::{DispatchSpec, SessionStatus};

    fn setup() -> (TempDir, EnergeiaStore) {
        let dir = TempDir::new().unwrap();
        let db = fjall::Database::builder(dir.path()).open().unwrap();
        (dir, EnergeiaStore::new(&db).unwrap())
    }

    fn spec(project: &str) -> DispatchSpec {
        DispatchSpec {
            prompt_numbers: vec![1],
            project: project.to_owned(),
            dag_ref: None,
            max_parallel: None,
            max_turns: None,
            budget_usd: None,
        }
    }

    #[test]
    fn empty_store_zero_active() {
        let (_dir, store) = setup();
        let dashboard = compute_status_dashboard(&store).unwrap();
        assert_eq!(dashboard.active_dispatches, 0);
        assert_eq!(dashboard.queue_depth, 0);
        assert_eq!(dashboard.stale_running_dispatches, 0);
        assert!(dashboard.recent_outcomes.is_empty());
        assert!(dashboard.by_project.is_empty());
        assert!(dashboard.cron.is_none());
    }

    #[test]
    fn running_dispatch_counted_as_active() {
        let (_dir, store) = setup();
        let _d = store.create_dispatch("acme", &spec("acme")).unwrap();
        let dashboard = compute_status_dashboard(&store).unwrap();
        assert_eq!(dashboard.active_dispatches, 1);
        assert_eq!(dashboard.queue_depth, 1);
    }

    #[test]
    fn completed_dispatch_not_active() {
        let (_dir, store) = setup();
        let d = store.create_dispatch("acme", &spec("acme")).unwrap();
        store
            .finish_dispatch(&d, crate::store::records::DispatchStatus::Completed)
            .unwrap();
        let dashboard = compute_status_dashboard(&store).unwrap();
        assert_eq!(dashboard.active_dispatches, 0);
    }

    #[test]
    fn recent_outcomes_contains_both_dispatches() {
        let (_dir, store) = setup();
        let d1 = store.create_dispatch("acme", &spec("acme")).unwrap();
        let d2 = store.create_dispatch("acme", &spec("acme")).unwrap();
        let dashboard = compute_status_dashboard(&store).unwrap();
        assert_eq!(dashboard.recent_outcomes.len(), 2);
        // Both dispatches appear; ULID ordering within the same millisecond is
        // non-deterministic, so we check presence rather than position.
        let ids: Vec<&str> = dashboard
            .recent_outcomes
            .iter()
            .map(|o| o.dispatch_id.as_str())
            .collect();
        assert!(ids.contains(&d1.as_str()));
        assert!(ids.contains(&d2.as_str()));
    }

    #[test]
    fn recent_outcomes_uses_newest_dispatches_beyond_scan_cap() {
        let (_dir, store) = setup();
        let total = crate::store::SCAN_LIMIT_DISPATCHES + 1;
        for i in 0..total {
            let id = DispatchId::new(format!("{i:026}"));
            let timestamp = jiff::Timestamp::from_millisecond(i64::try_from(i).unwrap()).unwrap();
            let record = DispatchRecord {
                id,
                project: "acme".to_owned(),
                spec: "{}".to_owned(),
                status: DispatchStatus::Completed,
                created_at: timestamp,
                finished_at: Some(timestamp),
                total_cost_usd: 0.0,
                total_sessions: 0,
            };
            store.insert_dispatch_record_for_test(&record).unwrap();
        }

        let dashboard = compute_status_dashboard(&store).unwrap();

        assert_eq!(dashboard.recent_outcomes.len(), RECENT_LIMIT);
        assert_eq!(
            dashboard.recent_outcomes[0].dispatch_id,
            format!("{:026}", total - 1)
        );
    }

    #[test]
    fn by_project_summary_aggregates_correctly() {
        let (_dir, store) = setup();
        let d1 = store.create_dispatch("acme", &spec("acme")).unwrap();
        let d2 = store.create_dispatch("other", &spec("other")).unwrap();
        let s1 = store.create_session(&d1, 1).unwrap();
        store
            .update_session(
                &s1,
                SessionUpdate {
                    cost_usd: Some(2.0),
                    status: Some(SessionStatus::Success),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .finish_dispatch(&d1, crate::store::records::DispatchStatus::Completed)
            .unwrap();
        store
            .finish_dispatch(&d2, crate::store::records::DispatchStatus::Failed)
            .unwrap();

        let dashboard = compute_status_dashboard(&store).unwrap();
        let acme = dashboard
            .by_project
            .iter()
            .find(|p| p.project == "acme")
            .unwrap();
        let other = dashboard
            .by_project
            .iter()
            .find(|p| p.project == "other")
            .unwrap();

        assert_eq!(acme.success_rate, 1.0);
        assert_eq!(other.success_rate, 0.0);
        assert_eq!(acme.total_sessions, 1);
        assert_eq!(acme.active_dispatches, 0);
    }

    #[test]
    fn active_dispatch_shows_in_project_summary() {
        let (_dir, store) = setup();
        let _d = store.create_dispatch("acme", &spec("acme")).unwrap();
        let dashboard = compute_status_dashboard(&store).unwrap();
        let acme = dashboard
            .by_project
            .iter()
            .find(|p| p.project == "acme")
            .unwrap();
        assert_eq!(acme.active_dispatches, 1);
    }

    #[test]
    fn status_dashboard_includes_stale_counts_and_last_fire_record() {
        let (_dir, store) = setup();
        let cron_dir = TempDir::new().unwrap();
        let lock_db =
            koina::fjall::FjallDb::open(cron_dir.path(), &["cron_locks", "cron_fire_state"])
                .unwrap();
        let partition = lock_db
            .db
            .keyspace("cron_fire_state", fjall::KeyspaceCreateOptions::default)
            .unwrap();

        let spec = spec("acme");
        let dispatch_id = store.create_dispatch("acme", &spec).unwrap();
        store
            .backdate_dispatch_for_test(&dispatch_id, jiff::SignedDuration::from_hours(2))
            .unwrap();

        let started_at = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(2))
            .unwrap();
        let record = CronFireRecord {
            scheduled_at: started_at,
            started_at,
            finished_at: None,
            succeeded: None,
        };
        let value = rmp_serde::to_vec(&record).unwrap();
        let mut tx = lock_db.db.write_tx();
        tx.insert(&partition, b"nightly", value.as_slice());
        tx.commit().unwrap();

        let lock_store = CronLockStore::open(std::sync::Arc::new(lock_db.db)).unwrap();
        let task_names = vec!["nightly".to_owned()];
        let dashboard =
            compute_status_dashboard_with_cron(&store, &lock_store, &task_names).unwrap();

        assert_eq!(dashboard.stale_running_dispatches, 1);
        let Some(cron) = dashboard.cron else {
            panic!("cron status included");
        };
        assert_eq!(cron.stale_fire_count, 1);
        assert_eq!(cron.task_fires.len(), 1);
        let Some(task_fire) = cron.task_fires.first() else {
            panic!("nightly task fire status included");
        };
        assert_eq!(task_fire.task_name, "nightly");
        let Some(last_fire) = task_fire.last_fire_record.as_ref() else {
            panic!("last fire record included");
        };
        assert_eq!(last_fire.scheduled_at, started_at);
        assert_eq!(last_fire.succeeded, None);
    }
}
