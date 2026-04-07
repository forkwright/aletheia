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
}
