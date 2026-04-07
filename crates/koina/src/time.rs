/// Time utilities shared across Aletheia crates.
///
/// Centralises timestamp helpers so every crate uses identical formatting
/// and the same underlying jiff dependency.

/// Return the current UTC time as an ISO 8601 string.
///
/// Uses [`jiff::Timestamp::now`] which produces full nanosecond precision,
/// e.g. `"2026-04-07T12:34:56.789012345Z"`.
#[must_use]
pub fn now_iso8601() -> String {
    jiff::Timestamp::now().to_string()
}
