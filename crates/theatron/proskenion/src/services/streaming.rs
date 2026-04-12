//! Streaming service that wraps per-message fetch streams with timeout and abort.

use std::time::Duration;

/// Maximum time to wait for a streaming response before timing out.
#[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
const STREAM_TIMEOUT: Duration = Duration::from_secs(600);

/// Interval for flushing buffered stream data to the UI.
#[cfg_attr(not(test), expect(dead_code, reason = "used in tests"))]
const FLUSH_INTERVAL: Duration = Duration::from_millis(100);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_constant_is_ten_minutes() {
        assert_eq!(STREAM_TIMEOUT, Duration::from_secs(600));
    }

    #[test]
    fn flush_interval_is_100ms() {
        assert_eq!(FLUSH_INTERVAL, Duration::from_millis(100));
    }
}
