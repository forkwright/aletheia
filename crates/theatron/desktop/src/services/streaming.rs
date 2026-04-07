//! Streaming service that wraps per-message fetch streams with timeout and abort.

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
