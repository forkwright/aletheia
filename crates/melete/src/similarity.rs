//! Similarity-based pruning to remove near-duplicate content before distillation.

use std::collections::HashSet;

use aletheia_hermeneus::types::{Content, ContentBlock, Message};

/// Minimum token length to include in similarity comparison.
const MIN_TOKEN_LEN: usize = 3;

/// Default Jaccard similarity threshold for near-duplicate detection.
pub(crate) const DEFAULT_SIMILARITY_THRESHOLD: f64 = 0.85;

/// Statistics from a similarity pruning pass.
#[derive(Debug, Clone)]
pub struct PruningStats {
    /// Total chunks evaluated.
    pub total_chunks: usize,
    /// Chunks removed as near-duplicates.
    pub pruned_count: usize,
}

impl PruningStats {
    /// Percentage of chunks pruned (0.0 to 100.0).
    #[must_use]
    pub(crate) fn reduction_percent(&self) -> f64 {
        if self.total_chunks == 0 {
            return 0.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            clippy::as_conversions,
            reason = "usize->f64: chunk counts always fit in f64 mantissa"
        )]
        let result = (self.pruned_count as f64 / self.total_chunks as f64) * 100.0;
        result
    }
}

/// Extract readable text content from a message for similarity comparison.
pub(crate) fn extract_text(message: &Message) -> String {
    match &message.content {
        Content::Text(s) => s.clone(),
        Content::Blocks(blocks) => {
            let mut text = String::new();
            for block in blocks {
                match block {
                    ContentBlock::Text { text: t, .. } => {
                        if !text.is_empty() {
                            text.push(' ');
                        }
                        text.push_str(t);
                    }
                    ContentBlock::Thinking { thinking, .. } => {
                        if !text.is_empty() {
                            text.push(' ');
                        }
                        text.push_str(thinking);
                    }
                    _ => {} // NOTE: Image, ToolUse, ToolResult blocks contain no extractable text
                }
            }
            text
        }
        // NOTE: Content is #[non_exhaustive]; future variants default to empty.
        _ => String::new(),
    }
}

/// Tokenize text into a set of lowercase words for Jaccard comparison.
///
/// Splits on whitespace and ASCII punctuation, discards tokens shorter than
/// [`MIN_TOKEN_LEN`] to reduce noise from articles and prepositions.
pub(crate) fn tokenize(text: &str) -> HashSet<String> {
    text.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
        .filter(|w| w.len() >= MIN_TOKEN_LEN)
        .map(str::to_lowercase)
        .collect()
}

/// Compute Jaccard similarity between two token sets.
///
/// Returns 1.0 when both sets are empty (trivially identical),
/// 0.0 when either set is empty (no overlap possible),
/// otherwise |A intersection B| / |A union B|.
#[must_use]
pub(crate) fn jaccard_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }

    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "usize->f64: set counts always fit in f64 mantissa"
    )]
    let result = intersection as f64 / union as f64;
    result
}

/// Remove near-duplicate messages by Jaccard similarity.
///
/// Compares each message pair and removes the earlier (older) message when
/// similarity exceeds `threshold`, keeping the more recent version.
///
/// Returns the filtered messages and pruning statistics.
pub(crate) fn prune_similar_messages(
    messages: &[Message],
    threshold: f64,
) -> (Vec<Message>, PruningStats) {
    let total_chunks = messages.len();
    if total_chunks <= 1 {
        return (
            messages.to_vec(),
            PruningStats {
                total_chunks,
                pruned_count: 0,
            },
        );
    }

    let token_sets: Vec<HashSet<String>> = messages
        .iter()
        .map(|m| tokenize(&extract_text(m)))
        .collect();

    let mut pruned = vec![false; total_chunks];
    for (i, tokens_i) in token_sets.iter().enumerate() {
        if *pruned.get(i).unwrap_or(&false) {
            continue;
        }
        for (j, tokens_j) in token_sets.iter().enumerate().skip(i + 1) {
            if *pruned.get(j).unwrap_or(&false) {
                continue;
            }
            let sim = jaccard_similarity(tokens_i, tokens_j);
            if sim >= threshold {
                // WHY: keep the more recent (higher index), prune the older
                if let Some(p) = pruned.get_mut(i) {
                    *p = true;
                }
                break;
            }
        }
    }

    let kept: Vec<Message> = messages
        .iter()
        .zip(pruned.iter())
        .filter(|(_, is_pruned)| !**is_pruned)
        .map(|(m, _)| m.clone())
        .collect();

    let pruned_count = total_chunks - kept.len();

    (
        kept,
        PruningStats {
            total_chunks,
            pruned_count,
        },
    )
}

#[cfg(test)]
mod tests {
    use aletheia_hermeneus::types::{Content, Message, Role};

    use super::*;

    fn text_msg(text: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: Content::Text(text.to_owned()),
        }
    }

    #[test]
    fn jaccard_identical_sets_returns_one() {
        let a: HashSet<String> = ["hello", "world"]
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        let b = a.clone();
        let sim = jaccard_similarity(&a, &b);
        assert!(
            (sim - 1.0).abs() < f64::EPSILON,
            "identical sets should have similarity 1.0, got {sim}"
        );
    }

    #[test]
    fn jaccard_disjoint_sets_returns_zero() {
        let a: HashSet<String> = ["alpha", "beta"].iter().map(|s| s.to_lowercase()).collect();
        let b: HashSet<String> = ["gamma", "delta"]
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        let sim = jaccard_similarity(&a, &b);
        assert!(
            sim.abs() < f64::EPSILON,
            "disjoint sets should have similarity 0.0, got {sim}"
        );
    }

    #[test]
    fn jaccard_partial_overlap() {
        let a: HashSet<String> = ["apple", "banana", "cherry"]
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        let b: HashSet<String> = ["banana", "cherry", "date"]
            .iter()
            .map(|s| s.to_lowercase())
            .collect();
        let sim = jaccard_similarity(&a, &b);
        // intersection = {banana, cherry} = 2, union = {apple, banana, cherry, date} = 4
        let expected = 2.0 / 4.0;
        assert!(
            (sim - expected).abs() < f64::EPSILON,
            "expected {expected}, got {sim}"
        );
    }

    #[test]
    fn jaccard_both_empty_returns_one() {
        let a: HashSet<String> = HashSet::new();
        let b: HashSet<String> = HashSet::new();
        let sim = jaccard_similarity(&a, &b);
        assert!(
            (sim - 1.0).abs() < f64::EPSILON,
            "two empty sets should have similarity 1.0, got {sim}"
        );
    }

    #[test]
    fn jaccard_one_empty_returns_zero() {
        let a: HashSet<String> = ["hello"].iter().map(|s| s.to_lowercase()).collect();
        let b: HashSet<String> = HashSet::new();
        assert!(
            jaccard_similarity(&a, &b).abs() < f64::EPSILON,
            "empty second set should yield 0.0"
        );
        assert!(
            jaccard_similarity(&b, &a).abs() < f64::EPSILON,
            "empty first set should yield 0.0"
        );
    }

    #[test]
    fn tokenize_splits_on_whitespace_and_punctuation() {
        let tokens = tokenize("Hello, world! This is a test.");
        assert!(
            tokens.contains("hello"),
            "should contain lowercased 'hello'"
        );
        assert!(tokens.contains("world"), "should contain 'world'");
        assert!(tokens.contains("this"), "should contain 'this'");
        assert!(tokens.contains("test"), "should contain 'test'");
        assert!(!tokens.contains("is"), "'is' is shorter than MIN_TOKEN_LEN");
        assert!(!tokens.contains("a"), "'a' is shorter than MIN_TOKEN_LEN");
    }

    #[test]
    fn tokenize_lowercases() {
        let tokens = tokenize("HELLO World MiXeD");
        assert!(tokens.contains("hello"), "should lowercase HELLO");
        assert!(tokens.contains("world"), "should lowercase World");
        assert!(tokens.contains("mixed"), "should lowercase MiXeD");
    }

    #[test]
    fn tokenize_empty_string() {
        let tokens = tokenize("");
        assert!(tokens.is_empty(), "empty string should produce empty set");
    }

    #[test]
    fn prune_removes_near_duplicate_keeps_recent() {
        let messages = vec![
            text_msg("The server runs on port 8080 with default configuration"),
            text_msg("Something completely different about databases"),
            text_msg("The server runs on port 8080 with default configuration settings"),
        ];
        let (kept, stats) = prune_similar_messages(&messages, 0.7);
        assert_eq!(stats.total_chunks, 3, "should evaluate all 3 messages");
        assert_eq!(stats.pruned_count, 1, "should prune 1 near-duplicate");
        assert_eq!(kept.len(), 2, "should keep 2 messages");
        // WHY: the first message (older) should be pruned, third (newer) kept
        assert_eq!(
            kept.first().map(extract_text),
            Some("Something completely different about databases".to_owned()),
            "second message should survive (not a duplicate)"
        );
    }

    #[test]
    fn prune_single_message_unchanged() {
        let messages = vec![text_msg("only one message here")];
        let (kept, stats) = prune_similar_messages(&messages, 0.85);
        assert_eq!(kept.len(), 1, "single message should be kept");
        assert_eq!(stats.pruned_count, 0, "nothing to prune");
    }

    #[test]
    fn prune_empty_messages() {
        let messages: Vec<Message> = vec![];
        let (kept, stats) = prune_similar_messages(&messages, 0.85);
        assert!(kept.is_empty(), "empty input should produce empty output");
        assert_eq!(stats.pruned_count, 0, "nothing to prune");
    }

    #[test]
    fn prune_no_duplicates_keeps_all() {
        let messages = vec![
            text_msg("The weather forecast says rain tomorrow"),
            text_msg("Database migration completed successfully"),
            text_msg("Review the pull request for the auth module"),
        ];
        let (kept, stats) = prune_similar_messages(&messages, 0.85);
        assert_eq!(kept.len(), 3, "no duplicates means all kept");
        assert_eq!(stats.pruned_count, 0, "no duplicates to prune");
    }

    #[test]
    fn prune_exact_duplicates_removed() {
        let messages = vec![
            text_msg("The configuration file needs updating for production"),
            text_msg("The configuration file needs updating for production"),
            text_msg("Something entirely different about testing"),
        ];
        let (kept, stats) = prune_similar_messages(&messages, 0.85);
        assert_eq!(stats.pruned_count, 1, "exact duplicate should be pruned");
        assert_eq!(kept.len(), 2, "should keep 2 messages");
    }

    #[test]
    fn pruning_stats_reduction_percent() {
        let stats = PruningStats {
            total_chunks: 45,
            pruned_count: 12,
        };
        let pct = stats.reduction_percent();
        let expected = 12.0 / 45.0 * 100.0;
        assert!(
            (pct - expected).abs() < 0.01,
            "expected ~{expected:.1}%, got {pct:.1}%"
        );
    }

    #[test]
    fn pruning_stats_zero_total_returns_zero() {
        let stats = PruningStats {
            total_chunks: 0,
            pruned_count: 0,
        };
        assert!(
            stats.reduction_percent().abs() < f64::EPSILON,
            "zero total should yield 0% reduction"
        );
    }

    #[test]
    fn extract_text_from_text_content() {
        let msg = Message {
            role: Role::User,
            content: Content::Text("hello world".to_owned()),
        };
        assert_eq!(extract_text(&msg), "hello world");
    }

    #[test]
    fn extract_text_from_blocks() {
        let msg = Message {
            role: Role::Assistant,
            content: Content::Blocks(vec![
                ContentBlock::Text {
                    text: "first part".to_owned(),
                    citations: None,
                },
                ContentBlock::Text {
                    text: "second part".to_owned(),
                    citations: None,
                },
            ]),
        };
        assert_eq!(extract_text(&msg), "first part second part");
    }
}
