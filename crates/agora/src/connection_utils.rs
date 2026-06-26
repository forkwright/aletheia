//! Shared connection utilities used by channel providers.

use std::time::Duration;

/// Exponential backoff delay for reconnection attempts.
///
/// 1s, 2s, 4s, 8s, 16s, 32s, 60s (capped).
#[must_use]
pub(crate) fn reconnect_delay(consecutive_failures: u32) -> Duration {
    let secs = 1u64.checked_shl(consecutive_failures.min(6)).unwrap_or(64);
    Duration::from_secs(secs.min(60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_delay_values() {
        assert_eq!(reconnect_delay(0), Duration::from_secs(1));
        assert_eq!(reconnect_delay(1), Duration::from_secs(2));
        assert_eq!(reconnect_delay(2), Duration::from_secs(4));
        assert_eq!(reconnect_delay(3), Duration::from_secs(8));
        assert_eq!(reconnect_delay(4), Duration::from_secs(16));
        assert_eq!(reconnect_delay(5), Duration::from_secs(32));
        assert_eq!(reconnect_delay(6), Duration::from_mins(1));
        assert_eq!(reconnect_delay(7), Duration::from_mins(1));
        assert_eq!(reconnect_delay(100), Duration::from_mins(1));
    }
}
