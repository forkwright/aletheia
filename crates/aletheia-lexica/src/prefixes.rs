//! Text prefixes and phrase patterns for detection and classification.

/// Phrases that indicate the user is issuing a behavioral correction.
///
/// Simple keyword matching is intentionally conservative. False negatives
/// (missed corrections) are preferable to false positives (storing random
/// sentences as corrections).
///
/// Sourced from `nous/src/hooks/builtins/correction.rs`.
pub const CORRECTION_PREFIXES: &[&str] = &[
    "don't ",
    "do not ",
    "stop ",
    "never ",
    "always ",
    "from now on",
    "remember to ",
    "make sure to ",
    "please don't ",
    "please do not ",
    "please always ",
    "please never ",
    "you should always ",
    "you should never ",
    "you must always ",
    "you must never ",
    "i need you to always ",
    "i need you to never ",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correction_prefixes_are_well_formed() {
        assert!(!CORRECTION_PREFIXES.is_empty());
        let set: std::collections::HashSet<_> = CORRECTION_PREFIXES.iter().copied().collect();
        assert_eq!(set.len(), CORRECTION_PREFIXES.len(), "no duplicates");
        for entry in CORRECTION_PREFIXES {
            assert!(!entry.is_empty(), "no empty strings");
            assert_eq!(entry.trim_start(), *entry, "no leading whitespace");
        }
    }

    #[test]
    fn correction_prefixes_consumer_shape() {
        let text = "don't do that";
        assert!(CORRECTION_PREFIXES.iter().any(|&p| text.starts_with(p)));
        let text2 = "from now on be careful";
        assert!(CORRECTION_PREFIXES.iter().any(|&p| text2.starts_with(p)));
        assert!(CORRECTION_PREFIXES.contains(&"don't "));
    }
}
