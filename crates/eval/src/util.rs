//! Small cross-cutting helpers shared by persistence and the benchmark runner.

use std::time::Duration;

/// Convert a duration to milliseconds, saturating at [`u64::MAX`] instead of
/// panicking or wrapping on overflow.
pub(crate) fn saturating_millis(duration: &Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
