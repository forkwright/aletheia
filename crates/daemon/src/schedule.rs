//! Scheduling primitives for background tasks.

use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use snafu::ResultExt;

use crate::error::{self, Result};

/// When a task should run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
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
}

/// What a background task does.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum TaskAction {
    /// Execute a shell command.
    Command(String),
    /// Call a tool by name with JSON arguments.
    Tool {
        name: String,
        args: serde_json::Value,
    },
    /// Send a prompt to the nous for processing.
    Prompt(String),
    /// Run a built-in maintenance function.
    Builtin(BuiltinTask),
}

/// Built-in maintenance tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum BuiltinTask {
    /// Prosoche attention check.
    Prosoche,
    /// Knowledge graph maintenance (dedup, orphan purge).
    GraphMaintenance,
    /// Memory consolidation.
    MemoryConsolidation,
}

impl Schedule {
    /// Calculate the next run time from now.
    ///
    /// Returns `None` for `Startup` (already ran) or `Once` with a past timestamp.
    ///
    /// # Errors
    ///
    /// - Returns an error if a `Cron` expression cannot be parsed into a valid schedule.
    pub fn next_run(&self) -> Result<Option<jiff::Timestamp>> {
        match self {
            Self::Cron(expr) => {
                let schedule = cron::Schedule::from_str(expr).context(
                    error::InvalidCronSnafu {
                        expression: expr.clone(),
                    },
                )?;
                let next = schedule.upcoming(chrono::Utc).next();
                Ok(next.map(|dt| {
                    jiff::Timestamp::from_second(dt.timestamp())
                        .expect("chrono timestamp in valid range")
                }))
            }
            Self::Interval(duration) => {
                let span = jiff::SignedDuration::from_nanos(
                    i64::try_from(duration.as_nanos())
                        .expect("interval fits in i64 nanos"),
                );
                let next = jiff::Timestamp::now().checked_add(span).expect("interval addition overflow");
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

    /// Check if the current time is within the active window.
    ///
    /// `None` window means always active. Handles overnight windows (e.g., 22-06).
    pub fn in_window(window: Option<(u8, u8)>) -> bool {
        let Some((start, end)) = window else {
            return true;
        };

        let now = jiff::Zoned::now();
        let hour = u8::try_from(now.hour()).expect("hour in u8 range");

        if start <= end {
            // Normal window: e.g., 9-17
            hour >= start && hour < end
        } else {
            // Overnight window: e.g., 22-06
            hour >= start || hour < end
        }
    }
}

/// Status snapshot of a registered task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatus {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub next_run: Option<String>,
    pub last_run: Option<String>,
    pub run_count: u64,
    pub consecutive_failures: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_next_run_returns_future() {
        let schedule = Schedule::Interval(Duration::from_secs(10));
        let next = schedule.next_run().expect("no error").expect("should have next");
        assert!(next > jiff::Timestamp::now());
    }

    #[test]
    fn once_future_returns_some() {
        let future = jiff::Timestamp::now()
            .checked_add(jiff::SignedDuration::from_secs(3600))
            .unwrap();
        let schedule = Schedule::Once(future);
        let next = schedule.next_run().expect("no error").expect("should have next");
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
        // 0-24 should always be active (every hour is >= 0 and < 24)
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
}
