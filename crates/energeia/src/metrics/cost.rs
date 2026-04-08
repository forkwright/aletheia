//! Cost and velocity reporting for energeia dispatch operations.
//!
//! Aggregates `DispatchRecord` and `SessionRecord` data into period reports
//! with per-project and per-day breakdowns.

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

/// Cost and velocity report for a time window.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CostReport {
    /// Start of the reporting window (inclusive).
    pub period_start: jiff::Timestamp,
    /// End of the reporting window (inclusive; the time the report was computed).
    pub period_end: jiff::Timestamp,
    /// Total cost across all dispatches in the window, in USD.
    pub total_cost_usd: f64,
    /// Total number of dispatches in the window.
    pub total_dispatches: u64,
    /// Total number of sessions across all dispatches in the window.
    pub total_sessions: u64,
    /// Average cost per dispatch (0.0 when `total_dispatches` is zero).
    pub avg_cost_per_dispatch: f64,
    /// Average cost per session (0.0 when `total_sessions` is zero).
    pub avg_cost_per_session: f64,
    /// Per-project cost breakdown, sorted by cost descending.
    pub by_project: Vec<ProjectCost>,
    /// Per-day velocity, sorted by date ascending.
    pub daily_velocity: Vec<DailyVelocity>,
}

/// Cost summary for a single project.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ProjectCost {
    /// Project identifier.
    pub project: String,
    /// Total cost in USD.
    pub cost_usd: f64,
    /// Number of dispatches.
    pub dispatches: u64,
    /// Total sessions across dispatches.
    pub sessions: u64,
    /// Fraction of completed dispatches (0.0–1.0).
    pub success_rate: f64,
}

/// Dispatch velocity and cost for a single calendar day.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DailyVelocity {
    /// Calendar date (UTC).
    pub date: jiff::civil::Date,
    /// Number of dispatches created on this date.
    pub dispatches: u64,
    /// Total sessions across those dispatches.
    pub sessions: u64,
    /// Total cost in USD.
    pub cost_usd: f64,
    /// Fraction of dispatches that completed successfully (0.0–1.0).
    pub success_rate: f64,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg(feature = "storage-fjall")]
/// Compute a cost and velocity report for the given number of past days.
///
/// Pass `window_days = 0` to include all available history.
/// Pass `window_days = 7` for the last week, `30` for the last month.
///
/// # Errors
///
/// Returns `Error::Store` if any underlying store read fails.
pub fn compute_cost_report(store: &EnergeiaStore, window_days: u32) -> Result<CostReport> {
    let now = jiff::Timestamp::now();

    let cutoff_ms: Option<i64> = if window_days > 0 {
        let span = jiff::SignedDuration::from_hours(i64::from(window_days) * 24);
        #[expect(
            clippy::expect_used,
            reason = "bounded subtraction from now is infallible for realistic day counts"
        )]
        let cutoff = now
            .checked_sub(span)
            .expect("timestamp subtraction within realistic day range");
        Some(cutoff.as_millisecond())
    } else {
        None
    };

    let period_start = cutoff_ms.map_or_else(
        || jiff::Timestamp::UNIX_EPOCH,
        |ms| {
            #[expect(
                clippy::expect_used,
                reason = "ms from cutoff calculation is a valid timestamp"
            )]
            jiff::Timestamp::from_millisecond(ms).expect("valid cutoff timestamp")
        },
    );

    let all_dispatches = store.list_dispatches(SCAN_LIMIT_DISPATCHES)?;
    let all_sessions = store.list_all_sessions(SCAN_LIMIT_SESSIONS)?;

    let dispatches: Vec<&DispatchRecord> = all_dispatches
        .iter()
        .filter(|d| cutoff_ms.is_none_or(|cutoff| d.created_at.as_millisecond() >= cutoff))
        .collect();

    let sessions: Vec<&SessionRecord> = all_sessions
        .iter()
        .filter(|s| cutoff_ms.is_none_or(|cutoff| s.created_at.as_millisecond() >= cutoff))
        .collect();

    // Count sessions per dispatch for aggregation.
    let sessions_per_dispatch: HashMap<&str, Vec<&SessionRecord>> = {
        let mut map: HashMap<&str, Vec<&SessionRecord>> = HashMap::new();
        for s in &sessions {
            map.entry(s.dispatch_id.as_str()).or_default().push(s);
        }
        map
    };

    let total_cost_usd: f64 = dispatches.iter().map(|d| d.total_cost_usd).sum();

    #[expect(
        clippy::as_conversions,
        reason = "dispatch/session counts are bounded by scan limits, fit u64"
    )]
    let total_dispatches = dispatches.len() as u64;

    #[expect(
        clippy::as_conversions,
        reason = "dispatch/session counts are bounded by scan limits, fit u64"
    )]
    let total_sessions = sessions.len() as u64;

    let avg_cost_per_dispatch = if total_dispatches > 0 {
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "total_dispatches is bounded; precision loss unreachable"
        )]
        {
            total_cost_usd / total_dispatches as f64
        }
    } else {
        0.0
    };

    let avg_cost_per_session = if total_sessions > 0 {
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "total_sessions is bounded; precision loss unreachable"
        )]
        {
            total_cost_usd / total_sessions as f64
        }
    } else {
        0.0
    };

    let by_project = build_project_costs(&dispatches, &sessions_per_dispatch);
    let daily_velocity = build_daily_velocity(&dispatches, &sessions_per_dispatch);

    Ok(CostReport {
        period_start,
        period_end: now,
        total_cost_usd,
        total_dispatches,
        total_sessions,
        avg_cost_per_dispatch,
        avg_cost_per_session,
        by_project,
        daily_velocity,
    })
}

// ---------------------------------------------------------------------------
// Aggregation helpers
// ---------------------------------------------------------------------------

#[cfg(feature = "storage-fjall")]
fn build_project_costs(
    dispatches: &[&DispatchRecord],
    sessions_per_dispatch: &HashMap<&str, Vec<&SessionRecord>>,
) -> Vec<ProjectCost> {
    #[derive(Default)]
    struct Acc {
        cost_usd: f64,
        dispatches: u64,
        sessions: u64,
        completed: u64,
    }

    let mut by_project: HashMap<&str, Acc> = HashMap::new();

    for d in dispatches {
        let acc = by_project.entry(d.project.as_str()).or_default();
        acc.cost_usd += d.total_cost_usd;

        acc.dispatches += 1;

        let session_count = sessions_per_dispatch
            .get(d.id.as_str())
            .map_or(0, Vec::len);

        #[expect(
            clippy::as_conversions,
            reason = "session count bounded, fits u64"
        )]
        {
            acc.sessions += session_count as u64;
        }

        if d.status == DispatchStatus::Completed {
            acc.completed += 1;
        }
    }

    let mut result: Vec<ProjectCost> = by_project
        .into_iter()
        .map(|(project, acc)| {
            let success_rate = if acc.dispatches > 0 {
                #[expect(
                    clippy::cast_precision_loss,
                    clippy::as_conversions,
                    reason = "counts bounded; precision loss unreachable"
                )]
                {
                    acc.completed as f64 / acc.dispatches as f64
                }
            } else {
                0.0
            };
            ProjectCost {
                project: project.to_owned(),
                cost_usd: acc.cost_usd,
                dispatches: acc.dispatches,
                sessions: acc.sessions,
                success_rate,
            }
        })
        .collect();

    // Sort by cost descending for easy reading.
    result.sort_by(|a, b| {
        b.cost_usd
            .partial_cmp(&a.cost_usd)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    result
}

#[cfg(feature = "storage-fjall")]
fn build_daily_velocity(
    dispatches: &[&DispatchRecord],
    sessions_per_dispatch: &HashMap<&str, Vec<&SessionRecord>>,
) -> Vec<DailyVelocity> {
    #[derive(Default)]
    struct DayAcc {
        dispatches: u64,
        sessions: u64,
        cost_usd: f64,
        completed: u64,
    }

    let mut by_day: HashMap<jiff::civil::Date, DayAcc> = HashMap::new();

    for d in dispatches {
        // Convert timestamp to UTC calendar date.
        let date = jiff::Timestamp::from_millisecond(d.created_at.as_millisecond())
            .unwrap_or(d.created_at)
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date();

        let acc = by_day.entry(date).or_default();

        acc.dispatches += 1;

        let session_count = sessions_per_dispatch
            .get(d.id.as_str())
            .map_or(0, Vec::len);

        #[expect(
            clippy::as_conversions,
            reason = "session count bounded, fits u64"
        )]
        {
            acc.sessions += session_count as u64;
        }

        acc.cost_usd += d.total_cost_usd;

        if d.status == DispatchStatus::Completed {
            acc.completed += 1;
        }
    }

    let mut result: Vec<DailyVelocity> = by_day
        .into_iter()
        .map(|(date, acc)| {
            let success_rate = if acc.dispatches > 0 {
                #[expect(
                    clippy::cast_precision_loss,
                    clippy::as_conversions,
                    reason = "counts bounded; precision loss unreachable"
                )]
                {
                    acc.completed as f64 / acc.dispatches as f64
                }
            } else {
                0.0
            };
            DailyVelocity {
                date,
                dispatches: acc.dispatches,
                sessions: acc.sessions,
                cost_usd: acc.cost_usd,
                success_rate,
            }
        })
        .collect();

    // Sort by date ascending.
    result.sort_by_key(|d| d.date);
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

    fn spec() -> DispatchSpec {
        DispatchSpec {
            prompt_numbers: vec![1, 2],
            project: "acme".to_owned(),
            dag_ref: None,
            max_parallel: Some(2),
        }
    }

    #[test]
    fn empty_store_returns_zero_report() {
        let (_dir, store) = setup();
        let report = compute_cost_report(&store, 30).unwrap();
        assert_eq!(report.total_dispatches, 0);
        assert_eq!(report.total_sessions, 0);
        assert_eq!(report.total_cost_usd, 0.0);
        assert!(report.by_project.is_empty());
        assert!(report.daily_velocity.is_empty());
    }

    #[test]
    fn aggregates_dispatch_cost() {
        let (_dir, store) = setup();
        let d1 = store.create_dispatch("acme", &spec()).unwrap();
        let d2 = store.create_dispatch("acme", &spec()).unwrap();
        let s1 = store.create_session(&d1, 1).unwrap();
        let s2 = store.create_session(&d2, 1).unwrap();
        store
            .update_session(
                &s1,
                SessionUpdate {
                    cost_usd: Some(1.0),
                    status: Some(SessionStatus::Success),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .update_session(
                &s2,
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
            .finish_dispatch(&d2, crate::store::records::DispatchStatus::Completed)
            .unwrap();

        let report = compute_cost_report(&store, 0).unwrap();
        assert_eq!(report.total_dispatches, 2);
        assert!((report.total_cost_usd - 3.0).abs() < 0.01);
        assert_eq!(report.by_project.len(), 1);
        assert_eq!(report.by_project[0].project, "acme");
        assert!((report.by_project[0].cost_usd - 3.0).abs() < 0.01);
    }

    #[test]
    fn by_project_sorted_cost_descending() {
        let (_dir, store) = setup();
        let d1 = store
            .create_dispatch(
                "cheap",
                &DispatchSpec {
                    project: "cheap".to_owned(),
                    prompt_numbers: vec![1],
                    dag_ref: None,
                    max_parallel: None,
                },
            )
            .unwrap();
        let d2 = store
            .create_dispatch(
                "expensive",
                &DispatchSpec {
                    project: "expensive".to_owned(),
                    prompt_numbers: vec![1],
                    dag_ref: None,
                    max_parallel: None,
                },
            )
            .unwrap();
        let s1 = store.create_session(&d1, 1).unwrap();
        let s2 = store.create_session(&d2, 1).unwrap();
        store
            .update_session(
                &s1,
                SessionUpdate {
                    cost_usd: Some(0.5),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .update_session(
                &s2,
                SessionUpdate {
                    cost_usd: Some(5.0),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .finish_dispatch(&d1, crate::store::records::DispatchStatus::Completed)
            .unwrap();
        store
            .finish_dispatch(&d2, crate::store::records::DispatchStatus::Completed)
            .unwrap();

        let report = compute_cost_report(&store, 0).unwrap();
        assert_eq!(report.by_project[0].project, "expensive");
        assert_eq!(report.by_project[1].project, "cheap");
    }

    #[test]
    fn avg_cost_per_dispatch_and_session() {
        let (_dir, store) = setup();
        let d = store.create_dispatch("acme", &spec()).unwrap();
        let s1 = store.create_session(&d, 1).unwrap();
        let s2 = store.create_session(&d, 2).unwrap();
        store
            .update_session(
                &s1,
                SessionUpdate {
                    cost_usd: Some(1.0),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .update_session(
                &s2,
                SessionUpdate {
                    cost_usd: Some(3.0),
                    ..Default::default()
                },
            )
            .unwrap();
        store
            .finish_dispatch(&d, crate::store::records::DispatchStatus::Completed)
            .unwrap();

        let report = compute_cost_report(&store, 0).unwrap();
        // 1 dispatch with cost 4.0, 2 sessions
        assert!((report.avg_cost_per_dispatch - 4.0).abs() < 0.01);
        assert!((report.avg_cost_per_session - 2.0).abs() < 0.01);
    }

    #[test]
    fn daily_velocity_groups_by_date() {
        let (_dir, store) = setup();
        let d = store.create_dispatch("acme", &spec()).unwrap();
        store
            .finish_dispatch(&d, crate::store::records::DispatchStatus::Completed)
            .unwrap();

        let report = compute_cost_report(&store, 0).unwrap();
        // Must have at least today's entry
        assert!(!report.daily_velocity.is_empty());
        let today = jiff::Timestamp::now()
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date();
        assert!(report.daily_velocity.iter().any(|dv| dv.date == today));
    }
}
