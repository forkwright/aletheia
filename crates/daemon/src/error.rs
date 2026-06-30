//! Daemon-specific errors.

use snafu::Snafu;

/// Errors from background task execution, scheduling, and maintenance operations.
#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
#[non_exhaustive]
#[expect(
    missing_docs,
    reason = "snafu error variant fields (source, location, context) are self-documenting via display format"
)]
// kanon:ignore RUST/non-exhaustive-enum — #[non_exhaustive] is present on line 8; #[expect] attribute between it and pub enum triggers false positive
// kanon:ignore RUST/pub-visibility — Error is this crate's public API, consumed by the aletheia binary crate
pub enum Error {
    /// Invalid cron expression.
    #[snafu(display("invalid cron expression '{expression}': {reason}"))]
    CronParse {
        expression: String,
        reason: String,
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

    /// Shell command was cancelled before completion.
    #[snafu(display("command cancelled: {command}"))]
    CommandCancelled {
        command: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Shell command exceeded its per-task timeout.
    #[snafu(display("command timed out after {timeout_secs}s: {command}"))]
    CommandTimedOut {
        command: String,
        timeout_secs: u64,
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

    /// Storage backend error (fjall task-state store).
    #[snafu(display("task-state storage error: {message}"))]
    Storage {
        message: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// JSON serialization/deserialization error within stored task state.
    #[snafu(display("task-state JSON error: {source}"))]
    StoredJson {
        source: serde_json::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// I/O error during maintenance operation.
    #[snafu(display("maintenance I/O error: {context}"))]
    MaintenanceIo {
        context: String,
        source: std::io::Error,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Spawned blocking task failed.
    #[snafu(display("blocking task failed: {context}"))]
    BlockingJoin {
        context: String,
        source: tokio::task::JoinError,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Invariant violation during maintenance (situation the caller believed impossible).
    #[snafu(display("maintenance invariant violated: {context}"))]
    MaintenanceInvariant {
        context: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },

    /// Source traversal was refused by backup safety policy.
    #[snafu(display(
        "backup traversal refused {reason}: {relative_path} under source root {source_root}"
    ))]
    BackupTraversalPolicy {
        reason: String,
        relative_path: String,
        source_root: String,
        #[snafu(implicit)]
        location: snafu::Location,
    },
}

/// Convenience alias for `Result` with daemon's [`Error`] type.
// kanon:ignore RUST/pub-visibility — Result is this crate's public API, consumed by the aletheia binary crate
pub type Result<T> = std::result::Result<T, Error>;
