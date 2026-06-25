#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(clippy::unwrap_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test: vec indices valid after asserting len"
)]

//! In-tree tests for `crate::runner`.

use super::*;

mod cron_and_output;
mod inflight_and_watchdog;
mod lifecycle_and_builders;
mod self_prompt_and_errors;

/// Build a minimal echo-command task used across the split test modules.
pub(super) fn make_echo_task(id: &str) -> TaskDef {
    TaskDef {
        id: id.to_owned(),
        name: format!("Test task {id}"),
        nous_id: "test-nous".to_owned(),
        schedule: Schedule::Interval(Duration::from_mins(1)),
        action: TaskAction::Command("echo hello".to_owned()),
        enabled: true,
        ..TaskDef::default()
    }
}
