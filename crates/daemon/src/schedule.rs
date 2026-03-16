//! Scheduling primitives for background tasks.

use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::error::{self, Result};

/// When a task should run.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Schedule {
    /// Cron expression (e.g., `"0 */45 8-23 * * *"` for every 45min 8am-11pm).
    Cron(String),
    /// Fixed interval.
    Interval(Duration),
    /// Run once at a specific time.
    Once(jiff::Timestamp),
    /// Run once at startup.
    Startup,
}

/// A registered background task definition.
#[derive(Debug, Clone)]
pub struct TaskDef {
    /// Unique task identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Which nous this task belongs to.
    pub nous_id: String,
    /// When to run.
    pub schedule: Schedule,
    /// What to run.
    pub action: TaskAction,
    /// Whether the task is currently enabled.
    pub enabled: bool,
    /// Active time window (optional): `(start_hour, end_hour)` in local time.
    pub active_window: Option<(u8, u8)>,
    /// Maximum duration a task may run before being considered hung.
    /// Default: 5 minutes.
    pub timeout: Duration,
    /// Whether to catch up missed cron windows on startup (within last 24h).
    /// Default: true for maintenance tasks, false for prosoche.
    pub catch_up: bool,
    /// Model override for bridge-dispatched tasks (e.g. prosoche heartbeats).
    ///
    /// When set, the bridge uses this model instead of the agent's conversation
    /// model, allowing cheap models for trivial daemon prompts.
    pub model_override: Option<String>,
}

impl Default for TaskDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            nous_id: String::new(),
            schedule: Schedule::Startup,
            action: TaskAction::Command(String::new()),
            enabled: true,
            active_window: None,
            timeout: Duration::from_secs(300),
            catch_up: true,
            model_override: None,
        }
    }
}

/// What a background task does.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum TaskAction {
    /// Execute a shell command.
    Command(String),
    /// Run a built-in maintenance function.
    Builtin(BuiltinTask),
}

/// Built-in maintenance tasks.
#[non_exhaustive]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BuiltinTask {
    /// Prosoche attention check.
    Prosoche,
    /// Rotate and compress old trace files.
    TraceRotation,
    /// Compare instance against template for configuration drift.
    DriftDetection,
    /// Monitor database file sizes against thresholds.
    DbSizeMonitor,
    /// Execute data retention policy cleanup.
    RetentionExecution,
    /// Refresh temporal decay scores for knowledge graph entities/edges.
    DecayRefresh,
    /// Find and merge duplicate entities in the knowledge graph.
    EntityDedup,
    /// Recompute graph-wide scores (`PageRank`, centrality, etc.).
    GraphRecompute,
    /// Re-embed entities whose embeddings are stale or missing.
    EmbeddingRefresh,
    /// Remove orphaned nodes, expired edges, and other detritus.
    KnowledgeGc,
    /// Rebuild or optimize knowledge graph indexes.
    IndexMaintenance,
    /// Run a diagnostic health check on the knowledge graph.
    GraphHealthCheck,
    /// Compute decay scores for skills and retire stale ones.
    SkillDecay,
}

impl Schedule {
    /// Calculate the next run time from now.
    ///
    /// Returns `None` for `Startup` (already ran) or `Once` with a past timestamp.
    #[expect(
        clippy::expect_used,
        reason = "timestamp conversions are within valid ranges; interval durations fit in i64 nanos for any reasonable schedule"
    )]
    pub fn next_run(&self) -> Result<Option<jiff::Timestamp>> {
        match self {
            Self::Cron(expr) => {
                let schedule = cron::Schedule::from_str(expr).context(error::InvalidCronSnafu {
                    expression: expr.clone(),
                })?;
                // WHY: chrono::Utc is required by the cron crate's iterator API.
                let next = schedule.upcoming(chrono::Utc).next();
                Ok(next.map(|dt| {
                    jiff::Timestamp::from_second(dt.timestamp())
                        .expect("cron timestamp within valid jiff range")
                }))
            }
            Self::Interval(duration) => {
                let span = jiff::SignedDuration::from_nanos(
                    i64::try_from(duration.as_nanos()).expect("interval fits in i64 nanos"),
                );
                let next = jiff::Timestamp::now()
                    .checked_add(span)
                    .expect("interval addition overflow");
                Ok(Some(next))
            }
            Self::Once(ts) => {
                if *ts > jiff::Timestamp::now() {
                    Ok(Some(*ts))
                } else {
                    Ok(None)
                }
            }
            Self::Startup => Ok(None),
        }
    }

    /// Check if a cron schedule was missed since `last_run`.
    ///
    /// Returns `true` if there was at least one scheduled run between `last_run`
    /// and `now` that was missed, and it's within the last 24 hours.
    #[expect(
        clippy::expect_used,
        reason = "timestamp conversions within valid ranges; 24h subtraction from current time cannot overflow"
    )]
    pub fn missed_since(&self, last_run: jiff::Timestamp) -> Result<bool> {
        let Self::Cron(expr) = self else {
            return Ok(false);
        };

        let now = jiff::Timestamp::now();
        let twenty_four_hours_ago = now
            .checked_sub(jiff::SignedDuration::from_hours(24))
            .expect("24h subtraction overflow");

        if last_run < twenty_four_hours_ago {
            return Ok(false);
        }

        let schedule = cron::Schedule::from_str(expr).context(error::InvalidCronSnafu {
            expression: expr.clone(),
        })?;

        // WHY: chrono::DateTime is required by the cron crate's after() API.
        let last_run_dt = chrono::DateTime::from_timestamp(last_run.as_second(), 0)
            .expect("jiff timestamp within valid range");

        let next_after_last = schedule.after(&last_run_dt).next();
        if let Some(next) = next_after_last {
            let next_ts = jiff::Timestamp::from_second(next.timestamp())
                .expect("cron timestamp within valid jiff range");
            Ok(next_ts < now)
        } else {
            Ok(false)
        }
    }

    /// Check if the current time is within the active window.
    ///
    /// `None` window means always active. Handles overnight windows (e.g., 22-06).
    #[expect(
        clippy::expect_used,
        reason = "hour() returns 0-23 which always fits in u8"
    )]
    pub fn in_window(window: Option<(u8, u8)>) -> bool {
        let Some((start, end)) = window else {
            return true;
        };

        let now = jiff::Zoned::now();
        let hour = u8::try_from(now.hour()).expect("hour in u8 range");

        if start <= end {
            hour >= start && hour < end
        } else {
            hour >= start || hour < end
        }
    }
}

/// Compute exponential backoff delay based on consecutive failure count.
///
/// Returns the delay to add before the next retry:
/// - 1st failure: 1 minute
/// - 2nd failure: 5 minutes
/// - 3rd+ failure: 15 minutes (but task will be auto-disabled at 3)
pub fn backoff_delay(consecutive_failures: u32) -> Duration {
    match consecutive_failures {
        0 => Duration::ZERO,
        1 => Duration::from_secs(60),
        2 => Duration::from_secs(300),
        _ => Duration::from_secs(900),
    }
}

/// Status snapshot of a registered task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatus {
    /// Unique task identifier.
    pub id: String,
    /// Human-readable task name.
    pub name: String,
    /// Whether the task is currently enabled (disabled after consecutive failures).
    pub enabled: bool,
    /// When the task is next scheduled to run (ISO 8601).
    pub next_run: Option<String>,
    /// When the task last ran (ISO 8601).
    pub last_run: Option<String>,
    /// Total successful executions.
    pub run_count: u64,
    /// Current streak of consecutive failures (resets on success).
    pub consecutive_failures: u32,
    /// Whether the task is currently in flight.
    pub in_flight: bool,
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test assertions")]
#[expect(clippy::expect_used, reason = "test assertions")]
mod tests {
    use super::*;

    #[test]
    fn interval_next_run_returns_future() {
        let schedule = Schedule::Interval(Duration::from_secs(10));
        let next = schedule
            .next_run()
            .expect("no error")
            .expect("should have next");
        assert!(next > jiff::Timestamp::now());
    }

    #[test]
    fn once_future_returns_some() {
        let future = jiff::Timestamp::now()
            .checked_add(jiff::SignedDuration::from_secs(3600))
            .unwrap();
        let schedule = Schedule::Once(future);
        let next = schedule
            .next_run()
            .expect("no error")
            .expect("should have next");
        assert_eq!(next, future);
    }

    #[test]
    fn once_past_returns_none() {
        let past = jiff::Timestamp::now()
            .checked_add(jiff::SignedDuration::from_secs(-3600))
            .unwrap();
        let schedule = Schedule::Once(past);
        assert!(schedule.next_run().expect("no error").is_none());
    }

    #[test]
    fn startup_returns_none() {
        let schedule = Schedule::Startup;
        assert!(schedule.next_run().expect("no error").is_none());
    }

    #[test]
    fn cron_valid_expression_parses() {
        let schedule = Schedule::Cron("0 0 * * * *".to_owned());
        let next = schedule.next_run().expect("no error");
        assert!(next.is_some(), "valid cron should produce a next run time");
    }

    #[test]
    fn cron_invalid_expression_errors() {
        let schedule = Schedule::Cron("not a cron expression".to_owned());
        assert!(schedule.next_run().is_err());
    }

    #[test]
    fn in_window_none_always_active() {
        assert!(Schedule::in_window(None));
    }

    #[test]
    fn in_window_full_day() {
        assert!(Schedule::in_window(Some((0, 24))));
    }

    #[test]
    fn in_window_overnight_covers_late_or_early() {
        // Overnight window 22-06: hour >= 22 OR hour < 6
        let now_hour = u8::try_from(jiff::Zoned::now().hour()).unwrap();
        let result = Schedule::in_window(Some((22, 6)));
        let expected = !(6..22).contains(&now_hour);
        assert_eq!(result, expected);
    }

    #[test]
    fn in_window_daytime_range() {
        // Daytime window 9-17: hour >= 9 AND hour < 17
        let now_hour = u8::try_from(jiff::Zoned::now().hour()).unwrap();
        let result = Schedule::in_window(Some((9, 17)));
        let expected = (9..17).contains(&now_hour);
        assert_eq!(result, expected);
    }

    #[test]
    fn interval_short_duration() {
        let schedule = Schedule::Interval(Duration::from_millis(1));
        let next = schedule
            .next_run()
            .expect("no error")
            .expect("should have next");
        // Should be very close to now (within a few seconds).
        let diff = next
            .since(jiff::Timestamp::now())
            .expect("since should work");
        assert!(diff.get_seconds() < 2, "1ms interval should be near-future");
    }

    #[test]
    fn cron_hourly_expression() {
        let schedule = Schedule::Cron("0 0 * * * *".to_owned());
        let next = schedule.next_run().expect("no error");
        assert!(next.is_some(), "hourly cron should produce next_run");
    }

    #[test]
    fn cron_complex_expression() {
        let schedule = Schedule::Cron("0 */15 9-17 * * MON-FRI".to_owned());
        let next = schedule.next_run().expect("no error");
        assert!(
            next.is_some(),
            "complex cron expression should parse and produce next_run"
        );
    }

    #[test]
    fn in_window_same_start_end() {
        // (10, 10): start <= end path, hour >= 10 && hour < 10 is always false.
        assert!(
            !Schedule::in_window(Some((10, 10))),
            "same start and end should always be false"
        );
    }

    #[test]
    fn schedule_debug_format() {
        let schedule = Schedule::Interval(Duration::from_secs(60));
        let debug_str = format!("{schedule:?}");
        assert!(
            debug_str.contains("Interval"),
            "Debug should contain variant name"
        );
    }

    #[test]
    fn task_status_fields() {
        let status = TaskStatus {
            id: "test-id".to_owned(),
            name: "Test Task".to_owned(),
            enabled: true,
            next_run: Some("2026-01-01T00:00:00Z".to_owned()),
            last_run: None,
            run_count: 42,
            consecutive_failures: 0,
            in_flight: false,
        };
        assert_eq!(status.id, "test-id");
        assert_eq!(status.name, "Test Task");
        assert!(status.enabled);
        assert!(status.next_run.is_some());
        assert!(status.last_run.is_none());
        assert_eq!(status.run_count, 42);
        assert_eq!(status.consecutive_failures, 0);
    }

    #[test]
    fn backoff_delay_values() {
        assert_eq!(backoff_delay(0), Duration::ZERO);
        assert_eq!(backoff_delay(1), Duration::from_secs(60));
        assert_eq!(backoff_delay(2), Duration::from_secs(300));
        assert_eq!(backoff_delay(3), Duration::from_secs(900));
        assert_eq!(backoff_delay(10), Duration::from_secs(900));
    }

    #[test]
    fn missed_since_non_cron_returns_false() {
        let schedule = Schedule::Interval(Duration::from_secs(60));
        let last_run = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(1))
            .unwrap();
        assert!(!schedule.missed_since(last_run).expect("no error"));
    }

    #[test]
    fn missed_since_stale_returns_false() {
        // Last run more than 24h ago — too stale to catch up.
        let schedule = Schedule::Cron("0 0 * * * *".to_owned());
        let last_run = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(25))
            .unwrap();
        assert!(!schedule.missed_since(last_run).expect("no error"));
    }

    #[test]
    fn missed_since_recent_cron_returns_true() {
        // Hourly cron, last run 2 hours ago — should have missed at least one window.
        let schedule = Schedule::Cron("0 0 * * * *".to_owned());
        let last_run = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_hours(2))
            .unwrap();
        assert!(schedule.missed_since(last_run).expect("no error"));
    }

    #[test]
    fn task_def_default() {
        let def = TaskDef::default();
        assert_eq!(def.timeout, Duration::from_secs(300));
        assert!(def.catch_up);
        assert!(def.enabled);
    }
}
