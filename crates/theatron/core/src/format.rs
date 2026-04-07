/// Format a token count with human-readable suffixes.
///
/// Returns `"1.5M"` for values ≥ 1,000,000, `"1.5K"` for values ≥ 1,000,
/// and the raw count as a string for smaller values.
#[expect(
    clippy::cast_precision_loss,
    reason = "display formatting; sub-unit precision is not meaningful"
)]
pub fn format_tokens(count: u64) -> String {
    const K: u64 = 1_000;
    const M: u64 = 1_000_000;
    if count >= M {
        format!("{:.1}M", count as f64 / M as f64)
    } else if count >= K {
        format!("{:.1}K", count as f64 / K as f64)
    } else {
        count.to_string()
    }
}

/// Format a duration in milliseconds as a human-readable string.
///
/// Returns `"150ms"` for sub-second durations, `"2.5s"` for seconds,
/// and `"2.5m"` for minutes.
#[expect(
    clippy::cast_precision_loss,
    reason = "display formatting; sub-unit precision is not meaningful"
)]
pub fn format_duration(ms: u64) -> String {
    if ms >= 60_000 {
        format!("{:.1}m", ms as f64 / 60_000.0)
    } else if ms >= 1_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else {
        format!("{ms}ms")
    }
}

/// Format a duration in seconds as a human-readable string.
///
/// Returns `"45s"` for sub-minute durations, `"2.5m"` for minutes,
/// and `"2.0h"` for hours.
pub fn format_duration_secs(secs: f64) -> String {
    if secs < 60.0 {
        format!("{secs:.0}s")
    } else if secs < 3_600.0 {
        let mins = secs / 60.0;
        format!("{mins:.1}m")
    } else {
        let hours = secs / 3_600.0;
        format!("{hours:.1}h")
    }
}

/// Truncate a string to `max_chars` characters, appending ellipsis if truncated.
///
/// Counts Unicode characters (not bytes), and uses `"…"` (U+2026) as the
/// ellipsis character.
///
/// # Example
///
/// ```
/// use theatron_core::format::truncate_str;
///
/// assert_eq!(truncate_str("hello world", 8), "hello w…");
/// assert_eq!(truncate_str("hello", 10), "hello");
/// ```
pub fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{truncated}\u{2026}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_tokens_small() {
        assert_eq!(format_tokens(0), "0");
        assert_eq!(format_tokens(999), "999");
    }

    #[test]
    fn format_tokens_kilo() {
        assert_eq!(format_tokens(1_000), "1.0K");
        assert_eq!(format_tokens(1_500), "1.5K");
    }

    #[test]
    fn format_tokens_mega() {
        assert_eq!(format_tokens(1_000_000), "1.0M");
        assert_eq!(format_tokens(2_500_000), "2.5M");
    }

    #[test]
    fn format_duration_milliseconds() {
        assert_eq!(format_duration(50), "50ms");
        assert_eq!(format_duration(999), "999ms");
    }

    #[test]
    fn format_duration_seconds() {
        assert_eq!(format_duration(1_000), "1.0s");
        assert_eq!(format_duration(2_500), "2.5s");
        assert_eq!(format_duration(10_000), "10.0s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(format_duration(60_000), "1.0m");
        assert_eq!(format_duration(150_000), "2.5m");
    }

    #[test]
    fn format_duration_secs_seconds() {
        assert_eq!(format_duration_secs(45.0), "45s");
    }

    #[test]
    fn format_duration_secs_minutes() {
        assert_eq!(format_duration_secs(150.0), "2.5m");
    }

    #[test]
    fn format_duration_secs_hours() {
        assert_eq!(format_duration_secs(7_200.0), "2.0h");
    }

    #[test]
    fn truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn truncate_str_long() {
        let result = truncate_str("hello world", 8);
        assert!(result.ends_with('\u{2026}'));
        assert_eq!(result.chars().count(), 8);
    }

    #[test]
    fn truncate_str_empty() {
        assert_eq!(truncate_str("", 5), "");
    }

    #[test]
    fn truncate_str_unicode() {
        // Unicode characters should be counted correctly, not bytes
        assert_eq!(truncate_str("héllo world", 8), "héllo w\u{2026}");
    }
}
