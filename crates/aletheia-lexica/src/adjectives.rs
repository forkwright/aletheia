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
