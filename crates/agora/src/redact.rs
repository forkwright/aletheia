//! Channel identity redaction for logs, metrics, spans, and audit records.
//!
//! Phone numbers, Matrix user/room IDs, and Signal group IDs are personal
//! identifiers: they may appear in exactly one place — the routing decision
//! and the provider call — and everywhere else (traces, warnings, command
//! audit records, health payloads) they go through [`identifier`].
//!
//! Secret-shaped *values* (tokens, API keys) are a different class and live
//! in `koina::redact`; this module is for channel *identities*.

use sha2::{Digest as _, Sha256};

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

/// Return a collision-resistant opaque handle for a channel identity.
///
/// Unlike [`identifier`], this is safe for durable correlation and map keys:
/// suffix-only redaction aliases unrelated phone numbers and Matrix IDs that
/// happen to share their last four characters. The domain separator prevents
/// these handles from being confused with hashes used for other purposes.
#[must_use]
pub fn opaque_identifier(domain: &str, value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"aletheia.agora.channel-identity.v1\0");
    hasher.update(domain.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(domain.as_bytes());
    hasher.update(b"\0");
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b"\0");
    hasher.update(value.as_bytes());
    format!("h:{}", crate::types::hex_lower(&hasher.finalize()))
}

/// Return an opaque handle for an optional channel identity.
#[must_use]
pub fn optional_opaque_identifier(domain: &str, value: Option<&str>) -> String {
    value.map_or_else(
        || "none".to_owned(),
        |value| opaque_identifier(domain, value),
    )
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
        // WHY: `identifier` keeps exactly the last 4 raw characters
        // (see `keeps_only_last_four_chars`), uniformly across ID shapes.
        // A Matrix id's domain suffix puts the '.' before "org" inside that
        // 4-character window, so the redacted form carries four dots, not
        // three -- this is not special-cased per domain.
        assert_eq!(identifier("@alice:example.org"), "....org");
        assert_eq!(identifier("!room:example.org"), "....org");
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

    #[test]
    fn opaque_handles_are_stable_and_do_not_alias_equal_suffixes() {
        let first = opaque_identifier("signal-account", "+15550100");
        assert_eq!(first, opaque_identifier("signal-account", "+15550100"));
        assert_ne!(first, opaque_identifier("signal-account", "+19990100"));
        assert_ne!(first, opaque_identifier("send-target", "+15550100"));
        assert!(!first.contains("0100"));
    }

    #[test]
    fn optional_opaque_identifier_marks_absence() {
        assert_eq!(optional_opaque_identifier("signal-account", None), "none");
        assert_eq!(
            optional_opaque_identifier("signal-account", Some("+15550100")),
            opaque_identifier("signal-account", "+15550100")
        );
    }
}
