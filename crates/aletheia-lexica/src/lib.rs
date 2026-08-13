//! Static lexicon and data constants for Aletheia.
//!
//! Houses string-typed word lists and pattern constants consumed by
//! `nous`, `melete`, and `poiesis`.

#![deny(missing_docs)]

// WARNING(#5576): `adjectives` stays `pub` despite zero cross-crate consumers,
// unlike its three siblings, which `nous`/`melete`/`poiesis` do consume.
// Demoting it does not compile: `UNFALSIFIABLE_ADJECTIVES` has no in-crate
// caller either, so `pub(crate)` makes it dead code. That is the real finding —
// the list is an orphaned vocabulary awaiting the lint that should read it, not
// a visibility mistake. Tracked in #6742; do not "fix" this by demoting it
// again, and do not delete the list without resolving that issue first.
pub mod adjectives;
pub mod keywords;
pub mod prefixes;
pub mod stopwords;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modules_are_reachable() {
        assert!(!adjectives::UNFALSIFIABLE_ADJECTIVES.is_empty());
        assert!(!keywords::CODING_KEYWORDS.is_empty());
        assert!(!prefixes::CORRECTION_PREFIXES.is_empty());
        assert!(!stopwords::ENGLISH_STOPWORDS.is_empty());
    }
}
