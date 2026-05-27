//! Static lexicon and data constants for Aletheia.
//!
//! Houses string-typed word lists and pattern constants consumed by
//! `nous`, `melete`, and `poiesis`.

#![deny(missing_docs)]

pub mod adjectives;
pub mod keywords;
pub mod prefixes;
pub mod stopwords;

#[cfg(test)]
mod tests {

    #[test]
    fn modules_are_reachable() {
        assert!(!super::adjectives::UNFALSIFIABLE_ADJECTIVES.is_empty());
        assert!(!super::keywords::CODING_KEYWORDS.is_empty());
        assert!(!super::prefixes::CORRECTION_PREFIXES.is_empty());
        assert!(!super::stopwords::ENGLISH_STOPWORDS.is_empty());
    }
}
