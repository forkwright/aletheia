//! Iterative retrieval helpers: terminology discovery and gap detection.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use tracing::debug;

use aletheia_mneme::recall::ScoredResult;

static STOPWORDS: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    HashSet::from([
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
    ])
});

/// Check if a word is a common English stopword.
pub(super) fn is_stopword(word: &str) -> bool {
    STOPWORDS.contains(word)
}

/// Extract domain-specific terms from first-pass results not present in the original query.
///
/// Splits result content on whitespace, filters stopwords and short words,
/// then returns the top-5 most frequent novel terms.
pub(super) fn discover_terminology(results: &[ScoredResult], original_query: &str) -> Vec<String> {
    let query_words: HashSet<String> = original_query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();

    let mut term_freq: HashMap<String, usize> = HashMap::new();
    for result in results {
        for word in result.content.split_whitespace() {
            let cleaned = word
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            if cleaned.len() > 3 && !query_words.contains(&cleaned) && !is_stopword(&cleaned) {
                *term_freq.entry(cleaned).or_default() += 1;
            }
        }
    }

    let mut terms: Vec<_> = term_freq.into_iter().collect();
    terms.sort_by(|a, b| b.1.cmp(&a.1));
    terms.into_iter().take(5).map(|(t, _)| t).collect()
}

/// Detect entity references in results that aren't captured as result IDs.
///
/// Scans for capitalized multi-word phrases (2+ consecutive capitalized words)
/// and quoted strings. These represent referenced-but-unretrieved entities.
pub(super) fn detect_gaps(results: &[ScoredResult]) -> Vec<String> {
    let source_ids: HashSet<&str> = results.iter().map(|r| r.source_id.as_str()).collect();
    let mut gaps: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for result in results {
        let words: Vec<&str> = result.content.split_whitespace().collect();
        let mut i = 0;
        while i < words.len() {
            #[expect(
                clippy::indexing_slicing,
                reason = "i < words.len() is checked by the while guard above"
            )]
            if starts_with_uppercase(words[i]) {
                let start = i;
                while i < words.len() {
                    #[expect(
                        clippy::indexing_slicing,
                        reason = "i < words.len() is checked by the while guard"
                    )]
                    if !starts_with_uppercase(words[i]) {
                        break;
                    }
                    i += 1;
                }
                if i - start >= 2 {
                    #[expect(
                        clippy::indexing_slicing,
                        reason = "start and i are both bounded by words.len()"
                    )]
                    let phrase = words[start..i].join(" ");
                    if !source_ids.contains(phrase.as_str()) && seen.insert(phrase.clone()) {
                        gaps.push(phrase);
                    }
                }
            } else {
                i += 1;
            }
        }

        for quoted in extract_quoted_strings(&result.content) {
            if !source_ids.contains(quoted.as_str()) && seen.insert(quoted.clone()) {
                gaps.push(quoted);
            }
        }
    }

    debug!(count = gaps.len(), "detected gaps in recall results");
    gaps
}

fn starts_with_uppercase(word: &str) -> bool {
    word.chars().next().is_some_and(char::is_uppercase)
}

fn extract_quoted_strings(text: &str) -> Vec<String> {
    let parts: Vec<&str> = text.split('"').collect();
    parts
        .iter()
        .enumerate()
        .filter(|(i, part)| i % 2 == 1 && !part.is_empty() && part.len() < 100)
        .map(|(_, part)| (*part).to_owned())
        .collect()
}
