//! Static lexicon and data constants for Aletheia.
//!
//! Houses string-typed word lists and pattern constants consumed by
//! `nous`, `melete`, and `poiesis`.

#![deny(missing_docs)]

// WHY(#5576): zero cross-crate consumers (`stopwords`/`keywords`/`prefixes` are
// the modules `nous`/`melete`/`poiesis` actually consume).
pub(crate) mod adjectives;
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
