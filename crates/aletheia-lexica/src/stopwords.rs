//! Common English stopword lists.

/// English stopwords for terminology discovery and text filtering.
///
/// Covers prepositions, pronouns, auxiliary verbs, determiners, and common
/// conjunctions.
pub const ENGLISH_STOPWORDS: &[&str] = &[
    "a",
    "an",
    "the",
    "and",
    "but",
    "or",
    "nor",
    "for",
    "yet",
    "so",
    "in",
    "on",
    "at",
    "to",
    "from",
    "by",
    "with",
    "about",
    "into",
    "through",
    "during",
    "before",
    "after",
    "above",
    "below",
    "between",
    "out",
    "off",
    "over",
    "under",
    "again",
    "further",
    "then",
    "once",
    "is",
    "am",
    "are",
    "was",
    "were",
    "be",
    "been",
    "being",
    "have",
    "has",
    "had",
    "having",
    "do",
    "does",
    "did",
    "doing",
    "will",
    "would",
    "shall",
    "should",
    "may",
    "might",
    "must",
    "can",
    "could",
    "need",
    "dare",
    "ought",
    "used",
    "i",
    "me",
    "my",
    "myself",
    "we",
    "our",
    "ours",
    "ourselves",
    "you",
    "your",
    "yours",
    "yourself",
    "yourselves",
    "he",
    "him",
    "his",
    "himself",
    "she",
    "her",
    "hers",
    "herself",
    "it",
    "its",
    "itself",
    "they",
    "them",
    "their",
    "theirs",
    "themselves",
    "what",
    "which",
    "who",
    "whom",
    "this",
    "that",
    "these",
    "those",
    "here",
    "there",
    "when",
    "where",
    "why",
    "how",
    "all",
    "each",
    "every",
    "both",
    "few",
    "more",
    "most",
    "other",
    "some",
    "such",
    "only",
    "own",
    "same",
    "than",
    "too",
    "very",
    "just",
    "also",
    "not",
    "no",
];

/// Smaller stopword list for probe token-overlap comparison.
///
/// Focused on high-frequency function words.
pub const ENGLISH_PROBE_STOP_WORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "had", "her", "was", "one",
    "our", "out", "has", "his", "how", "its", "may", "new", "now", "old", "see", "way", "who",
    "did", "get", "let", "say", "she", "too", "use", "will", "with", "this", "that", "from",
    "have", "been", "some", "they", "were", "what", "when", "your", "each", "make", "like", "into",
    "just", "over", "such", "than", "them", "then", "also", "more", "should",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_stopword_lists_are_well_formed() {
        for list in [ENGLISH_STOPWORDS, ENGLISH_PROBE_STOP_WORDS] {
            assert!(!list.is_empty(), "list must be non-empty");
            let set: std::collections::HashSet<_> = list.iter().copied().collect();
            assert_eq!(set.len(), list.len(), "no duplicates");
            for entry in list {
                assert!(!entry.is_empty(), "no empty strings");
                assert_eq!(entry.trim(), *entry, "no leading/trailing whitespace");
                assert!(
                    entry
                        .chars()
                        .all(|c| !c.is_alphabetic() || c.is_lowercase()),
                    "expected lowercase: {entry}"
                );
            }
        }
    }

    #[test]
    fn stopwords_consumer_shape() {
        assert!(ENGLISH_STOPWORDS.contains(&"the"));
        assert!(ENGLISH_STOPWORDS.contains(&"and"));
        assert!(ENGLISH_PROBE_STOP_WORDS.contains(&"with"));
    }
}
