//! Similarity-based pruning to remove near-duplicate content before distillation.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use hermeneus::types::{Content, ContentBlock, Message};

/// Default minimum token length to include in similarity comparison.
///
/// Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::similarity_min_token_len`.
pub(crate) const DEFAULT_MIN_TOKEN_LEN: usize = 3;

/// Default Jaccard similarity threshold for near-duplicate detection.
///
/// Callers should prefer the value from `taxis::config::AgentBehaviorDefaults::similarity_threshold`.
pub const DEFAULT_SIMILARITY_THRESHOLD: f64 = 0.85;

/// Default maximum number of messages to compare for similarity in a single pass.
///
/// Older messages outside this window are always preserved. This bounds the
/// quadratic worst case of pairwise Jaccard comparison.
pub const DEFAULT_MAX_SIMILARITY_MESSAGES: usize = 150;

/// Number of `MinHash` signatures used for Locality-Sensitive Hashing.
const MINHASH_SIGNATURES: usize = 20;

/// Number of bands for `MinHash` LSH.
const MINHASH_BANDS: usize = 10;

/// Number of rows per band for `MinHash` LSH.
const MINHASH_ROWS: usize = 2;

/// Compile-time guard that the `MinHash` parameters form a consistent grid.
const _: () = assert!(
    MINHASH_SIGNATURES == MINHASH_BANDS * MINHASH_ROWS,
    "MinHash signature count must equal bands * rows"
);

/// Statistics from a similarity pruning pass.
#[derive(Debug, Clone)]
pub struct PruningStats {
    /// Total chunks evaluated.
    pub total_chunks: usize,
    /// Chunks removed as near-duplicates.
    pub pruned_count: usize,
    /// Exact Jaccard comparisons performed (after LSH and length filtering).
    pub candidate_pairs: usize,
}

impl PruningStats {
    /// Percentage of chunks pruned (0.0 to 100.0).
    #[must_use]
    pub fn reduction_percent(&self) -> f64 {
        if self.total_chunks == 0 {
            return 0.0;
        }
        // WHY f64::from(u32): chunk counts fit in u32 (< 2^32); u32→f64 is
        // an exact conversion; `try_from` saturating to u32::MAX guards
        // the pathological case.
        let pruned_u32 = u32::try_from(self.pruned_count).unwrap_or(u32::MAX);
        let total_u32 = u32::try_from(self.total_chunks).unwrap_or(u32::MAX);
        (f64::from(pruned_u32) / f64::from(total_u32)) * 100.0
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
/// [`DEFAULT_MIN_TOKEN_LEN`] to reduce noise from articles and prepositions.
///
/// # Complexity
///
/// O(n) where n is the length of the input text.
pub(crate) fn tokenize(text: &str) -> HashSet<String> {
    text.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
        .filter(|w| w.len() >= DEFAULT_MIN_TOKEN_LEN)
        .map(str::to_lowercase)
        .collect()
}

/// Compute Jaccard similarity between two token sets.
///
/// Returns 1.0 when both sets are empty (trivially identical),
/// 0.0 when either set is empty (no overlap possible),
/// otherwise |A intersection B| / |A union B|.
///
/// # Complexity
///
/// O(min(a, b)) where a and b are the sizes of the two sets.
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

    // WHY f64::from(u32): set counts fit in u32 (< 2^32); u32→f64 is an
    // exact conversion; `try_from` saturating to u32::MAX guards the
    // pathological case.
    let i_u32 = u32::try_from(intersection).unwrap_or(u32::MAX);
    let u_u32 = u32::try_from(union).unwrap_or(u32::MAX);
    f64::from(i_u32) / f64::from(u_u32)
}

/// Remove near-duplicate messages by Jaccard similarity.
///
/// Compares each message pair and removes the earlier (older) message when
/// similarity exceeds `threshold`, keeping the more recent version.
///
/// Only the most recent `max_messages` are compared; older messages are
/// preserved verbatim. Candidate pairs are generated with `MinHash` LSH and
/// filtered by a length-ratio bound before the exact Jaccard comparison.
///
/// # Complexity
///
/// Expected O(n) for dissimilar inputs and O(n²) in the worst case of many
/// LSH collisions (e.g., all messages are identical).
///
/// Returns the filtered messages and pruning statistics.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    clippy::indexing_slicing,
    clippy::too_many_lines,
    reason = "token counts fit in f64 mantissa; LSH loop uses known-valid candidate indices"
)]
pub(crate) fn prune_similar_messages(
    messages: &[Message],
    threshold: f64,
    max_messages: usize,
) -> (Vec<Message>, PruningStats) {
    let total_chunks = messages.len();
    if total_chunks <= 1 {
        return (
            messages.to_vec(),
            PruningStats {
                total_chunks,
                pruned_count: 0,
                candidate_pairs: 0,
            },
        );
    }

    // WHY(#5681): cap the comparison window at the most recent messages.
    // Messages older than the cap are always kept.
    let candidate_start = total_chunks.saturating_sub(max_messages);
    #[expect(
        clippy::indexing_slicing,
        reason = "candidate_start ≤ total_chunks by saturating_sub"
    )]
    let prefix = &messages[..candidate_start];
    #[expect(
        clippy::indexing_slicing,
        reason = "candidate_start ≤ total_chunks by saturating_sub"
    )]
    let candidate = &messages[candidate_start..];
    let candidate_len = candidate.len();

    if candidate_len <= 1 {
        return (
            messages.to_vec(),
            PruningStats {
                total_chunks: candidate_len,
                pruned_count: 0,
                candidate_pairs: 0,
            },
        );
    }

    let token_sets: Vec<HashSet<String>> = candidate
        .iter()
        .map(|m| tokenize(&extract_text(m)))
        .collect();

    let signatures: Vec<Vec<u64>> = token_sets.iter().map(minhash_signatures).collect();

    let mut buckets: Vec<HashMap<[u64; MINHASH_ROWS], Vec<usize>>> =
        vec![HashMap::new(); MINHASH_BANDS];
    for (idx, sigs) in signatures.iter().enumerate() {
        for (band, bucket_map) in buckets.iter_mut().enumerate() {
            let mut key = [0_u64; MINHASH_ROWS];
            for (r, key_slot) in key.iter_mut().enumerate() {
                *key_slot = sigs[band * MINHASH_ROWS + r];
            }
            bucket_map.entry(key).or_default().push(idx);
        }
    }

    let mut pruned = vec![false; candidate_len];
    let mut candidate_pairs: usize = 0;
    let mut compared: HashSet<(usize, usize)> = HashSet::new();

    for bucket_map in &buckets {
        for members in bucket_map.values() {
            if members.len() < 2 {
                continue;
            }
            for (pos, &i) in members.iter().enumerate() {
                if pruned[i] {
                    continue;
                }
                for &j in members.iter().skip(pos + 1) {
                    if pruned[j] {
                        continue;
                    }
                    let pair = if i < j { (i, j) } else { (j, i) };
                    if !compared.insert(pair) {
                        continue;
                    }
                    let size_i = token_sets[i].len();
                    let size_j = token_sets[j].len();
                    let (small, large) = if size_i <= size_j {
                        (size_i, size_j)
                    } else {
                        (size_j, size_i)
                    };
                    if large > 0 && (small as f64 / large as f64) < threshold {
                        continue;
                    }
                    candidate_pairs += 1;
                    if jaccard_similarity(&token_sets[i], &token_sets[j]) >= threshold {
                        // WHY: keep the more recent (higher index), prune the older
                        let older = i.min(j);
                        if let Some(p) = pruned.get_mut(older) {
                            *p = true;
                        }
                    }
                }
            }
        }
    }

    let kept_candidates: Vec<Message> = candidate
        .iter()
        .zip(pruned.iter())
        .filter(|(_, is_pruned)| !**is_pruned)
        .map(|(m, _)| m.clone())
        .collect();

    let pruned_count = candidate_len - kept_candidates.len();

    let mut kept = prefix.to_vec();
    kept.extend(kept_candidates);

    (
        kept,
        PruningStats {
            total_chunks: candidate_len,
            pruned_count,
            candidate_pairs,
        },
    )
}

/// Compute `MinHash` signatures for a token set.
///
/// For each signature index, hashes every token with that seed and keeps the
/// minimum hash value. The probability that two sets share a signature equals
/// their Jaccard similarity.
fn minhash_signatures(tokens: &HashSet<String>) -> Vec<u64> {
    let mut sigs = vec![u64::MAX; MINHASH_SIGNATURES];
    if tokens.is_empty() {
        return sigs;
    }
    for (seed, min) in sigs.iter_mut().enumerate() {
        for token in tokens {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            hasher.write_usize(seed);
            token.hash(&mut hasher);
            let h = hasher.finish();
            if h < *min {
                *min = h;
            }
        }
    }
    sigs
}

#[cfg(test)]
mod tests {
    use hermeneus::types::{Content, Message, Role};

    use super::*;

    fn text_msg(text: &str) -> Message {
        Message {
            role: Role::Assistant,
            content: Content::Text(text.to_owned()),
            cache_breakpoint: false,
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
        let (kept, stats) = prune_similar_messages(&messages, 0.7, usize::MAX);
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
        let (kept, stats) = prune_similar_messages(&messages, 0.85, usize::MAX);
        assert_eq!(kept.len(), 1, "single message should be kept");
        assert_eq!(stats.pruned_count, 0, "nothing to prune");
    }

    #[test]
    fn prune_empty_messages() {
        let messages: Vec<Message> = vec![];
        let (kept, stats) = prune_similar_messages(&messages, 0.85, usize::MAX);
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
        let (kept, stats) = prune_similar_messages(&messages, 0.85, usize::MAX);
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
        let (kept, stats) = prune_similar_messages(&messages, 0.85, usize::MAX);
        assert_eq!(stats.pruned_count, 1, "exact duplicate should be pruned");
        assert_eq!(kept.len(), 2, "should keep 2 messages");
    }

    #[test]
    fn pruning_stats_reduction_percent() {
        let stats = PruningStats {
            total_chunks: 45,
            pruned_count: 12,
            candidate_pairs: 100,
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
            candidate_pairs: 0,
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
            cache_breakpoint: false,
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
            cache_breakpoint: false,
        };
        assert_eq!(extract_text(&msg), "first part second part");
    }

    #[test]
    fn prune_respects_max_messages_and_preserves_older() {
        let mut messages = Vec::new();
        for i in 0..5 {
            messages.push(text_msg(&format!("older unique message number {i}")));
        }
        messages.push(text_msg(
            "The server runs on port 8080 with default configuration",
        ));
        messages.push(text_msg("Something completely different about databases"));
        messages.push(text_msg(
            "The server runs on port 8080 with default configuration settings",
        ));

        let (kept, stats) = prune_similar_messages(&messages, 0.7, 3);

        assert_eq!(
            stats.total_chunks, 3,
            "should evaluate only the recent window"
        );
        // WHY: the older five are outside the cap and must survive unchanged.
        assert_eq!(
            kept.len(),
            7,
            "older prefix preserved plus recent survivors"
        );
        assert!(
            kept.iter()
                .any(|m| extract_text(m).starts_with("older unique")),
            "older prefix messages must be kept"
        );
    }

    #[test]
    fn minhash_lsh_reduces_comparisons_for_large_input() {
        let n = 500;
        let mut messages = Vec::with_capacity(n);
        for i in 0..n {
            // WHY: disjoint single-token sets keep Jaccard at 0, so LSH should
            // produce almost no candidate pairs.
            messages.push(text_msg(&format!("word{i:06}")));
        }

        let (_, stats) = prune_similar_messages(&messages, 0.85, n);

        // O(n) upper bound; the naive pairwise count is O(n²).
        let linear_bound = n * 10;
        assert!(
            stats.candidate_pairs <= linear_bound,
            "LSH should keep candidate pairs sub-quadratic, got {} (bound {})",
            stats.candidate_pairs,
            linear_bound
        );
    }
}
