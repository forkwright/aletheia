//! Daemon-specific errors.

use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
pub enum Error {
    /// Invalid cron expression.
    #[snafu(display("invalid cron expression: {expression}"))]
    InvalidCron {
        expression: String,
        source: cron::error::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Task execution failed.
    #[snafu(display("task execution failed for {task_id}: {reason}"))]
    TaskFailed {
        task_id: String,
        reason: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Shell command execution failed.
    #[snafu(display("command execution failed: {command}"))]
    CommandFailed {
        command: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Task disabled after consecutive failures.
    #[snafu(display("task {task_id} disabled after {failures} consecutive failures"))]
    TaskDisabled {
        task_id: String,
        failures: u32,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Shutdown signal received.
    #[snafu(display("shutdown signal received"))]
    Shutdown {
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
