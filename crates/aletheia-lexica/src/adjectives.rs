//! Vocabulary lists for quality, measurement, and evaluation.

/// Adjectives that are unfalsifiable without measurement context.
///
/// These appear in planning documents and vision statements but cannot be
/// tested without concrete metrics. Sourced from
/// `basanos/src/rules/planning.rs`.
pub const UNFALSIFIABLE_ADJECTIVES: &[&str] = &[
    "world-class",
    "production-grade",
    "best-in-class",
    "robust",
    "scalable",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfalsifiable_adjectives_are_well_formed() {
        assert!(!UNFALSIFIABLE_ADJECTIVES.is_empty());
        let set: std::collections::HashSet<_> = UNFALSIFIABLE_ADJECTIVES.iter().copied().collect();
        assert_eq!(set.len(), UNFALSIFIABLE_ADJECTIVES.len(), "no duplicates");
        for entry in UNFALSIFIABLE_ADJECTIVES {
            assert!(!entry.is_empty(), "no empty strings");
            assert_eq!(entry.trim(), *entry, "no leading/trailing whitespace");
        }
    }

    #[test]
    fn unfalsifiable_adjectives_consumer_shape() {
        assert!(UNFALSIFIABLE_ADJECTIVES.contains(&"robust"));
        assert!(UNFALSIFIABLE_ADJECTIVES.contains(&"scalable"));
    }
}
