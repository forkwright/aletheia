//! Scoring metrics for benchmark answer comparison.
//!
//! Implements the three standard metrics used in memory benchmarks:
//! - **Exact match** (EM): lowercase whitespace-normalized string equality
//! - **Token F1**: word-level F1 score (precision + recall harmonic mean)
//! - **Contains**: whether the expected answer appears as a substring
//!
//! The runner picks the best score across all expected answers (benchmarks
//! often allow multiple valid forms of the same answer).

use serde::{Deserialize, Serialize};

/// Result of scoring an actual answer against one or more expected answers.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BenchmarkScore {
    /// Exact match: normalized strings are equal.
    pub exact_match: bool,
    /// Token-level F1 score in [0.0, 1.0].
    pub f1: f64,
    /// Whether any expected answer is a substring of the actual answer.
    pub contains: bool,
}

impl BenchmarkScore {
    /// A score of all-zeros (no match).
    #[must_use]
    pub fn zero() -> Self {
        Self {
            exact_match: false,
            f1: 0.0,
            contains: false,
        }
    }
}

/// Score an actual answer against a list of expected answers.
///
/// Returns the best score across all expected answers:
/// - `exact_match` is true if any expected answer matches exactly
/// - `f1` is the maximum F1 across all expected answers
/// - `contains` is true if any expected answer is a substring of actual
#[must_use]
pub fn score_answer(actual: &str, expected: &[String]) -> BenchmarkScore {
    if expected.is_empty() {
        return BenchmarkScore::zero();
    }

    let actual_norm = normalize(actual);
    let actual_tokens: Vec<&str> = actual_norm.split_whitespace().collect();

    let mut best = BenchmarkScore::zero();

    for exp in expected {
        let exp_norm = normalize(exp);
        let exact_match = actual_norm == exp_norm;

        let exp_tokens: Vec<&str> = exp_norm.split_whitespace().collect();
        let f1 = token_f1(&actual_tokens, &exp_tokens);

        let contains = if exp_norm.is_empty() {
            false
        } else {
            actual_norm.contains(&exp_norm)
        };

        if exact_match {
            best.exact_match = true;
        }
        if f1 > best.f1 {
            best.f1 = f1;
        }
        if contains {
            best.contains = true;
        }
    }

    best
}

/// Normalize a string for comparison: lowercase, collapse whitespace, strip punctuation.
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Compute token-level F1 between predicted and expected token lists.
///
/// F1 = 2 * precision * recall / (precision + recall)
/// where precision = common / |predicted| and recall = common / |expected|.
#[expect(
    clippy::cast_precision_loss,
    reason = "token counts are small (<1000); f64 mantissa handles them exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64 — token counts are bounded and small"
)]
fn token_f1(predicted: &[&str], expected: &[&str]) -> f64 {
    if predicted.is_empty() && expected.is_empty() {
        return 1.0;
    }
    if predicted.is_empty() || expected.is_empty() {
        return 0.0;
    }

    // Count common tokens (multiset intersection)
    let mut expected_counts = std::collections::HashMap::<&str, usize>::new();
    for t in expected {
        *expected_counts.entry(t).or_insert(0) += 1;
    }

    let mut common = 0_usize;
    for t in predicted {
        if let Some(count) = expected_counts.get_mut(t)
            && *count > 0
        {
            *count -= 1;
            common += 1;
        }
    }

    if common == 0 {
        return 0.0;
    }

    let precision = common as f64 / predicted.len() as f64; // SAFETY: token counts <1000; exact in f64 mantissa
    let recall = common as f64 / expected.len() as f64; // SAFETY: token counts <1000; exact in f64 mantissa
    2.0 * precision * recall / (precision + recall)
}

/// Build a normalized-content reference for fallback retrieval scoring.
#[must_use]
pub fn normalized_content_ref(content: &str) -> String {
    format!(
        "content_sha256:{}",
        crate::provenance::sha256_hex_str(&normalize(content))
    )
}

/// Compute Recall@k: fraction of relevant refs found in the top-k retrieved.
///
/// `relevant` is the set of ground-truth refs. `retrieved` is the ordered list
/// of returned refs. The caller owns whether those refs are evidence IDs or
/// normalized-content fallback hashes.
///
/// # Panics
///
/// Panics if `k == 0`.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "k and relevant counts are small (<10000); f64 mantissa handles them exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64 — counts are bounded and small"
)]
pub fn recall_at_k(retrieved: &[String], relevant: &[String], k: usize) -> f64 {
    assert!(k > 0, "k must be > 0");
    if relevant.is_empty() {
        return 1.0;
    }
    let top_k = retrieved.get(..retrieved.len().min(k)).unwrap_or(retrieved);
    let found = relevant.iter().filter(|r| top_k.contains(r)).count();
    found as f64 / relevant.len() as f64 // SAFETY: counts <10_000 per function-level #[expect]; exact in f64 mantissa
}

/// Compute NDCG@k (Normalized Discounted Cumulative Gain).
///
/// Assumes binary relevance: an item is relevant if it appears in `relevant`.
/// `retrieved` is the ordered list of returned refs.
///
/// # Panics
///
/// Panics if `k == 0`.
#[must_use]
#[expect(
    clippy::cast_precision_loss,
    reason = "k and relevant counts are small (<10000); f64 mantissa handles them exactly"
)]
#[expect(
    clippy::as_conversions,
    reason = "usize to f64 — counts are bounded and small"
)]
pub fn ndcg_at_k(retrieved: &[String], relevant: &[String], k: usize) -> f64 {
    assert!(k > 0, "k must be > 0");
    if relevant.is_empty() {
        return 1.0;
    }

    let top_k = retrieved.get(..retrieved.len().min(k)).unwrap_or(retrieved);

    // DCG
    let dcg: f64 = top_k
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let rel = if relevant.contains(item) { 1.0 } else { 0.0 };
            rel / ((i + 2) as f64).log2() // SAFETY: index bounded by k (<10_000); exact in f64 mantissa
        })
        .sum();

    // Ideal DCG: all relevant items in top positions
    let ideal_count = relevant.len().min(k);
    let idcg: f64 = (0..ideal_count)
        .map(|i| 1.0 / ((i + 2) as f64).log2()) // SAFETY: index bounded by ideal_count (<=k<10_000); exact in f64 mantissa
        .sum();

    if idcg == 0.0 { 0.0 } else { dcg / idcg }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_normalized() {
        let score = score_answer("Hello World", &["hello world".to_owned()]);
        assert!(score.exact_match);
        assert!((score.f1 - 1.0).abs() < f64::EPSILON);
        assert!(score.contains);
    }

    #[test]
    fn exact_match_ignores_punctuation() {
        let score = score_answer("Hello, World!", &["hello world".to_owned()]);
        assert!(score.exact_match);
    }

    #[test]
    fn no_match_returns_zero() {
        let score = score_answer("blue", &["red".to_owned()]);
        assert!(!score.exact_match);
        assert!(score.f1.abs() < f64::EPSILON);
        assert!(!score.contains);
    }

    #[test]
    fn partial_token_overlap_gives_partial_f1() {
        // Predicted: "alice is a data scientist"
        // Expected:  "alice is a software engineer"
        // Common tokens: {alice, is, a} = 3
        // Precision: 3/5 = 0.6
        // Recall: 3/5 = 0.6
        // F1: 0.6
        let score = score_answer(
            "Alice is a data scientist",
            &["alice is a software engineer".to_owned()],
        );
        assert!(!score.exact_match);
        assert!(
            (score.f1 - 0.6).abs() < 0.01,
            "expected ~0.6, got {}",
            score.f1
        );
    }

    #[test]
    fn contains_detects_substring() {
        let score = score_answer(
            "The answer is San Francisco by the way",
            &["san francisco".to_owned()],
        );
        assert!(score.contains);
        assert!(!score.exact_match); // not an exact match, but substring
    }

    #[test]
    fn multiple_expected_picks_best() {
        let score = score_answer(
            "blue",
            &["red".to_owned(), "green".to_owned(), "blue".to_owned()],
        );
        assert!(score.exact_match);
        assert!((score.f1 - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn empty_expected_returns_zero() {
        let score = score_answer("anything", &[]);
        assert!(!score.exact_match);
        assert!(score.f1.abs() < f64::EPSILON);
    }

    #[test]
    fn empty_actual_returns_zero() {
        let score = score_answer("", &["expected".to_owned()]);
        assert!(!score.exact_match);
        assert!(score.f1.abs() < f64::EPSILON);
    }

    #[test]
    fn both_empty_exact_match() {
        let score = score_answer("", &[String::new()]);
        assert!(score.exact_match);
    }

    #[test]
    fn token_f1_handles_duplicates() {
        // Predicted: "the the cat"   → 3 tokens
        // Expected:  "the cat the"   → 3 tokens
        // Common multiset: {the, the, cat} = 3
        // Precision: 3/3, Recall: 3/3, F1: 1.0
        let score = score_answer("the the cat", &["the cat the".to_owned()]);
        assert!((score.f1 - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn token_f1_penalizes_extra_tokens() {
        // Predicted: "alice" (1 token)
        // Expected:  "alice is a data scientist" (5 tokens)
        // Common: {alice} = 1
        // Precision: 1/1 = 1.0, Recall: 1/5 = 0.2
        // F1: 2 * 1.0 * 0.2 / 1.2 ≈ 0.333
        let score = score_answer("alice", &["alice is a data scientist".to_owned()]);
        assert!(
            (score.f1 - 0.333).abs() < 0.01,
            "expected ~0.333, got {}",
            score.f1
        );
    }

    #[test]
    fn recall_at_k_finds_all_relevant() {
        let retrieved = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        let relevant = vec!["a".to_owned(), "b".to_owned()];
        assert!((recall_at_k(&retrieved, &relevant, 3) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn recall_at_k_partial() {
        let retrieved = vec!["a".to_owned(), "x".to_owned(), "c".to_owned()];
        let relevant = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        // k=2 finds only "a" ("c" is at rank 3, outside top-2)
        assert!((recall_at_k(&retrieved, &relevant, 2) - 1.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn recall_at_k_empty_relevant_is_one() {
        let retrieved = vec!["a".to_owned()];
        let relevant: Vec<String> = vec![];
        assert!((recall_at_k(&retrieved, &relevant, 1) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ndcg_at_k_perfect_ordering() {
        let retrieved = vec!["a".to_owned(), "b".to_owned(), "x".to_owned()];
        let relevant = vec!["a".to_owned(), "b".to_owned()];
        assert!((ndcg_at_k(&retrieved, &relevant, 3) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ndcg_at_k_zero_when_none_relevant() {
        let retrieved = vec!["x".to_owned(), "y".to_owned()];
        let relevant = vec!["a".to_owned(), "b".to_owned()];
        assert!(ndcg_at_k(&retrieved, &relevant, 2).abs() < f64::EPSILON);
    }

    #[test]
    fn ndcg_at_k_empty_relevant_is_one() {
        let retrieved = vec!["a".to_owned()];
        let relevant: Vec<String> = vec![];
        assert!((ndcg_at_k(&retrieved, &relevant, 1) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn recall_scores_evidence_ids_without_text_matching() {
        let retrieved = vec!["fact-a".to_owned(), "fact-b".to_owned()];
        let relevant = vec!["fact-b".to_owned()];
        assert!((recall_at_k(&retrieved, &relevant, 2) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn normalized_content_ref_hashes_normalized_text() {
        let left = normalized_content_ref("Blue, whale");
        let right = normalized_content_ref("blue whale");
        assert_eq!(left, right);
        assert!(left.starts_with("content_sha256:"));
    }
}
