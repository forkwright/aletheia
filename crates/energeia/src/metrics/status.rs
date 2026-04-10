//! Real-time status dashboard for energeia dispatch operations.
//!
//! Provides a point-in-time snapshot of:
//! - Active (running) dispatches and their session counts
//! - Queue depth (number of running dispatches)
//! - Recent dispatch outcomes (last N dispatches, newest first)
//! - Per-project summary of active and recent activity

#[cfg(feature = "storage-fjall")]
use std::collections::HashMap;

#[cfg(feature = "storage-fjall")]
use crate::error::Result;
#[cfg(feature = "storage-fjall")]
use crate::store::records::{DispatchRecord, DispatchStatus, SessionRecord};
#[cfg(feature = "storage-fjall")]
use crate::store::{EnergeiaStore, SCAN_LIMIT_DISPATCHES, SCAN_LIMIT_SESSIONS};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

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
    /// Recent dispatch outcomes (newest first), up to `RECENT_LIMIT`.
    pub recent_outcomes: Vec<RecentOutcome>,
    /// Per-project summary aggregated from active and recent dispatches.
    pub by_project: Vec<ProjectSummary>,
}

/// Summary of a single dispatch run for the recent history list.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RecentOutcome {
    /// Dispatch identifier.
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

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg(feature = "storage-fjall")]
/// Build a real-time status dashboard snapshot.
///
/// Scans all dispatches and sessions. The `recent_outcomes` list contains the
/// most recent `50` dispatches sorted newest-first.
///
/// # Errors
///
/// Returns `Error::Store` if any underlying store read fails.
pub(crate) fn compute_status_dashboard(store: &EnergeiaStore) -> Result<StatusDashboard> {
    let now = jiff::Timestamp::now();

    let all_dispatches = store.list_dispatches(SCAN_LIMIT_DISPATCHES)?;
    let all_sessions = store.list_all_sessions(SCAN_LIMIT_SESSIONS)?;

    // Count sessions per dispatch for summary aggregation.
    let mut sessions_by_dispatch: HashMap<&str, Vec<&SessionRecord>> = HashMap::new();
    for s in &all_sessions {
        sessions_by_dispatch
            .entry(s.dispatch_id.as_str())
            .or_default()
            .push(s);
    }

    // -----------------------------------------------------------------------
    // Active dispatches / queue depth
    // -----------------------------------------------------------------------

    let active_dispatch_records: Vec<&DispatchRecord> = all_dispatches
        .iter()
        .filter(|d| d.status == DispatchStatus::Running)
        .collect();

    let active_dispatches = u64::try_from(active_dispatch_records.len()).unwrap_or(u64::MAX);
    let queue_depth = active_dispatches;

    // -----------------------------------------------------------------------
    // Recent outcomes — last RECENT_LIMIT dispatches, newest first
    // -----------------------------------------------------------------------

    // Dispatches from the scan come out oldest-first (ULID order). Reverse to
    // get newest first, then take up to RECENT_LIMIT.
    let recent_outcomes: Vec<RecentOutcome> = all_dispatches
        .iter()
        .rev()
        .take(RECENT_LIMIT)
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

    // -----------------------------------------------------------------------
    // Per-project summary
    // -----------------------------------------------------------------------

    let by_project = build_project_summaries(&all_dispatches, &sessions_by_dispatch);

    Ok(StatusDashboard {
        computed_at: now,
        active_dispatches,
        queue_depth,
        recent_outcomes,
        by_project,
    })
}

// ---------------------------------------------------------------------------
// Aggregation helpers
// ---------------------------------------------------------------------------

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

        let session_count = sessions_by_dispatch
            .get(d.id.as_str())
            .map_or(0, Vec::len);

        acc.sessions += u64::try_from(session_count).unwrap_or(u64::MAX);
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
                    acc.completed as f64 / acc.total as f64
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

    // Sort alphabetically by project name for stable output.
    result.sort_by(|a, b| a.project.cmp(&b.project));
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[cfg(feature = "storage-fjall")]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::float_cmp, reason = "test assertions on exact float values")]
mod tests {
    use super::*;
    use tempfile::TempDir;

    use crate::store::EnergeiaStore;
    use crate::store::records::SessionUpdate;
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
        }
    }

    #[test]
    fn empty_store_zero_active() {
        let (_dir, store) = setup();
        let dashboard = compute_status_dashboard(&store).unwrap();
        assert_eq!(dashboard.active_dispatches, 0);
        assert_eq!(dashboard.queue_depth, 0);
        assert!(dashboard.recent_outcomes.is_empty());
        assert!(dashboard.by_project.is_empty());
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
}
