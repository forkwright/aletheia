//! Channel identity redaction for logs, metrics, spans, and audit records.
//!
//! Phone numbers, Matrix user/room IDs, and Signal group IDs are personal
//! identifiers: they may appear in exactly one place — the routing decision
//! and the provider call — and everywhere else (traces, warnings, command
//! audit records, health payloads) they go through [`identifier`].
//!
//! Secret-shaped *values* (tokens, API keys) are a different class and live
//! in `koina::redact`; this module is for channel *identities*.

/// Redact a channel identity for output, keeping a short suffix for
/// correlation (`"+15550100"` → `"...0100"`).
///
/// The suffix is the last four characters; identifiers of four or fewer
/// characters redact fully. Redaction is deliberately not reversible or
/// hash-stable across channels — correlation within a log stream is the
/// goal, not lookup.
#[must_use]
pub fn identifier(value: &str) -> String {
    let mut chars: Vec<char> = value.chars().collect();
    if chars.len() <= 4 {
        return "****".to_owned();
    }
    let suffix: String = chars.split_off(chars.len() - 4).into_iter().collect();
    format!("...{suffix}")
}

/// Redact an optional identity, preserving absence as `"none"`.
#[must_use]
pub fn optional_identifier(value: Option<&str>) -> String {
    value.map_or_else(|| "none".to_owned(), identifier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_phone_numbers() {
        assert_eq!(identifier("+1234567890"), "...7890");
        assert_eq!(identifier("+15550100"), "...0100");
    }

    #[test]
    fn redacts_matrix_ids() {
        assert_eq!(identifier("@alice:example.org"), "...org");
        assert_eq!(identifier("!room:example.org"), "...org");
    }

    #[test]
    fn redacts_short_values_fully() {
        assert_eq!(identifier("1234"), "****");
        assert_eq!(identifier("12"), "****");
        assert_eq!(identifier(""), "****");
    }

    #[test]
    fn keeps_only_last_four_chars() {
        assert_eq!(identifier("12345"), "...2345");
    }

    #[test]
    fn handles_multibyte_suffix_safely() {
        let out = identifier("number-éé");
        assert!(out.starts_with("..."), "{out}");
        assert!(!out.contains("number"), "{out}");
    }

    #[test]
    fn optional_identifier_marks_absence() {
        assert_eq!(optional_identifier(None), "none");
        assert_eq!(optional_identifier(Some("+15550100")), "...0100");
    }
}
