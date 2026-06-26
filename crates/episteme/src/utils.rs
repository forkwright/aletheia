//! Internal string/set similarity helpers shared across the knowledge store and
//! skill extraction pipelines.

/// Compute Jaccard overlap between two tool lists.
///
/// Returns 1.0 for identical non-empty sets and 0.0 for disjoint sets or when
/// both lists are empty.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "tool set sizes are small; precision loss is impossible in practice"
)]
pub(crate) fn compute_tool_overlap(a: &[String], b: &[String]) -> f64 {
    // WHY: Two skills with no tools share no overlap; a 1.0 score would make
    // the dedup heuristic treat any pair of no-tool skills as duplicates.
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let set_a: std::collections::HashSet<&str> = a.iter().map(String::as_str).collect();
    let set_b: std::collections::HashSet<&str> = b.iter().map(String::as_str).collect();
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

/// Compute name similarity using longest common subsequence ratio.
///
/// Returns 1.0 for identical names, 0.0 for completely different.
#[expect(
    clippy::as_conversions,
    clippy::cast_precision_loss,
    reason = "string lengths are small; precision loss is impossible in practice"
)]
pub(crate) fn compute_name_similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();
    let a_chars: Vec<char> = a_lower.chars().collect();
    let b_chars: Vec<char> = b_lower.chars().collect();
    let max_len = a_chars.len().max(b_chars.len());
    if max_len == 0 {
        return 1.0;
    }
    let lcs = lcs_char_length(&a_chars, &b_chars);
    lcs as f64 / max_len as f64
}

/// Classic DP Longest Common Subsequence length for char slices.
#[expect(
    clippy::indexing_slicing,
    reason = "DP table indices are bounded by the allocated (m+1)*(n+1) size"
)]
fn lcs_char_length(a: &[char], b: &[char]) -> usize {
    let m = a.len();
    let n = b.len();
    let mut dp = vec![0usize; (m + 1) * (n + 1)];
    let idx = |i: usize, j: usize| i * (n + 1) + j;
    for i in 1..=m {
        for j in 1..=n {
            dp[idx(i, j)] = if a[i - 1] == b[j - 1] {
                dp[idx(i - 1, j - 1)] + 1
            } else {
                dp[idx(i - 1, j)].max(dp[idx(i, j - 1)])
            };
        }
    }
    dp[idx(m, n)]
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── compute_tool_overlap ────────────────────────────────────────────────

    #[test]
    fn tool_overlap_identical_sets() {
        let a = vec!["read".to_owned(), "write".to_owned(), "bash".to_owned()];
        let b = vec!["read".to_owned(), "write".to_owned(), "bash".to_owned()];
        assert!(
            (compute_tool_overlap(&a, &b) - 1.0).abs() < f64::EPSILON,
            "identical sets should yield overlap 1.0"
        );
    }

    #[test]
    fn tool_overlap_disjoint_sets() {
        let a = vec!["read".to_owned(), "write".to_owned()];
        let b = vec!["bash".to_owned(), "grep".to_owned()];
        assert!(
            compute_tool_overlap(&a, &b).abs() < f64::EPSILON,
            "disjoint sets should yield overlap 0.0"
        );
    }

    #[test]
    fn tool_overlap_partial_intersection() {
        // 2 shared of 4 total = 0.5
        let a = vec!["read".to_owned(), "write".to_owned(), "bash".to_owned()];
        let b = vec!["read".to_owned(), "write".to_owned(), "grep".to_owned()];
        let result = compute_tool_overlap(&a, &b);
        assert!(
            (result - 0.5).abs() < f64::EPSILON,
            "expected 0.5, got {result}"
        );
    }

    #[test]
    fn tool_overlap_both_empty_returns_zero() {
        let a: Vec<String> = Vec::new();
        let b: Vec<String> = Vec::new();
        assert!(
            compute_tool_overlap(&a, &b).abs() < f64::EPSILON,
            "both-empty tool sets should yield overlap 0.0"
        );
    }

    #[test]
    fn tool_overlap_one_empty_returns_zero() {
        let a = vec!["read".to_owned()];
        let b: Vec<String> = Vec::new();
        assert!(
            compute_tool_overlap(&a, &b).abs() < f64::EPSILON,
            "one-empty tool set should yield overlap 0.0"
        );
    }

    #[test]
    fn tool_overlap_duplicates_deduplicated() {
        // Duplicates in input should be collapsed by the HashSet
        let a = vec!["read".to_owned(), "read".to_owned(), "write".to_owned()];
        let b = vec!["read".to_owned(), "write".to_owned()];
        assert!(
            (compute_tool_overlap(&a, &b) - 1.0).abs() < f64::EPSILON,
            "duplicates in sets should not affect overlap 1.0"
        );
    }

    // ── compute_name_similarity ─────────────────────────────────────────────

    #[test]
    fn name_similarity_identical() {
        assert!(
            (compute_name_similarity("Alice", "Alice") - 1.0).abs() < f64::EPSILON,
            "identical names should yield similarity 1.0"
        );
    }

    #[test]
    fn name_similarity_case_insensitive() {
        // LCS runs on lowercased strings
        assert!(
            (compute_name_similarity("Alice", "alice") - 1.0).abs() < f64::EPSILON,
            "case-insensitive match should yield similarity 1.0"
        );
    }

    #[test]
    fn name_similarity_completely_different() {
        // "abc" vs "xyz" — LCS length 0, ratio 0.0
        assert!(
            compute_name_similarity("abc", "xyz").abs() < f64::EPSILON,
            "disjoint names should yield similarity 0.0"
        );
    }

    #[test]
    fn name_similarity_substring_match() {
        // "kitten" vs "kit" — LCS = "kit" (3), max_len = 6, ratio = 0.5
        let result = compute_name_similarity("kitten", "kit");
        assert!(
            (result - 0.5).abs() < f64::EPSILON,
            "expected 0.5, got {result}"
        );
    }

    #[test]
    fn name_similarity_both_empty() {
        assert!(
            (compute_name_similarity("", "") - 1.0).abs() < f64::EPSILON,
            "both-empty names should yield similarity 1.0"
        );
    }

    #[test]
    fn name_similarity_one_empty() {
        assert!(
            compute_name_similarity("hello", "").abs() < f64::EPSILON,
            "one-empty name should yield similarity 0.0"
        );
    }

    // ── lcs_char_length ─────────────────────────────────────────────────────

    #[test]
    fn lcs_exact_match() {
        let a: Vec<char> = "abc".chars().collect();
        let b: Vec<char> = "abc".chars().collect();
        assert_eq!(
            lcs_char_length(&a, &b),
            3,
            "LCS of exact match should equal input length"
        );
    }

    #[test]
    fn lcs_partial_match() {
        // "abcde" vs "ace" → LCS = "ace" (3)
        let a: Vec<char> = "abcde".chars().collect();
        let b: Vec<char> = "ace".chars().collect();
        assert_eq!(
            lcs_char_length(&a, &b),
            3,
            "LCS of abcde vs ace should be 3"
        );
    }

    #[test]
    fn lcs_no_match() {
        let a: Vec<char> = "abc".chars().collect();
        let b: Vec<char> = "xyz".chars().collect();
        assert_eq!(
            lcs_char_length(&a, &b),
            0,
            "LCS of disjoint char sets should be 0"
        );
    }

    #[test]
    fn lcs_empty_inputs() {
        let empty: Vec<char> = Vec::new();
        let a: Vec<char> = "abc".chars().collect();
        assert_eq!(lcs_char_length(&empty, &a), 0, "LCS(empty, a) should be 0");
        assert_eq!(lcs_char_length(&a, &empty), 0, "LCS(a, empty) should be 0");
        assert_eq!(
            lcs_char_length(&empty, &empty),
            0,
            "LCS(empty, empty) should be 0"
        );
    }

    // ── consolidated path exercised through a real caller ───────────────────

    #[test]
    fn dedup_fallback_uses_consolidated_similarity() {
        use crate::skill::SkillContent;
        use crate::skills::extract::{DedupInput, DedupOutcome, check_dedup};

        let candidate = SkillContent {
            name: "rust-error-handling".to_owned(),
            description: "Handle Rust errors".to_owned(),
            steps: vec![],
            tools_used: vec!["Read".to_owned(), "Edit".to_owned()],
            domain_tags: vec![],
            origin: "test".to_owned(),
            triggers: vec![],
            always: false,
        };
        let existing = SkillContent {
            name: "rust-errors".to_owned(),
            description: "Rust errors".to_owned(),
            steps: vec![],
            tools_used: vec!["Read".to_owned()],
            domain_tags: vec![],
            origin: "test".to_owned(),
            triggers: vec![],
            always: false,
        };

        let input = DedupInput {
            candidate: &candidate,
            candidate_confidence: 0.8,
            candidate_usage: 1,
            existing: &existing,
            existing_confidence: 0.8,
            existing_usage: 1,
            existing_id: "fact-1",
            candidate_embedding: None,
            existing_embedding: None,
        };

        let outcome = check_dedup(&input);
        assert!(
            matches!(outcome, DedupOutcome::Unique),
            "partial overlap should not be considered a duplicate"
        );
    }
}
