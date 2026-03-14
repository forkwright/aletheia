//! Tool call sequence signatures for recurrence detection.
//!
//! Two sequences are "similar" when their normalized tool-name lists have a
//! Longest Common Subsequence / max-length ratio ≥ 0.8.
//!
//! ## Normalization steps
//!
//! 1. Extract tool names in order.
//! 2. Collapse consecutive duplicates (`Read, Read, Read` → `Read`).
//! 3. Produce a stable u64 hash for fast pre-filter.

use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::skills::ToolCallRecord;

/// A normalized, hashable fingerprint for a tool call sequence.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SequenceSignature {
    /// Ordered, deduplicated (consecutive) tool names.
    pub normalized: Vec<String>,
    /// Fast pre-filter hash of `normalized`.
    pub hash: u64,
}

impl SequenceSignature {
    /// Number of steps in the normalized sequence.
    pub fn len(&self) -> usize {
        self.normalized.len()
    }

    /// Returns `true` if the normalized sequence is empty.
    pub fn is_empty(&self) -> bool {
        self.normalized.is_empty()
    }
}

/// Compute a [`SequenceSignature`] for a tool call sequence.
///
/// 1. Extracts tool names in order.
/// 2. Collapses consecutive duplicates.
/// 3. Hashes with [`DefaultHasher`] for fast equality checks.
pub fn sequence_signature(tool_calls: &[ToolCallRecord]) -> SequenceSignature {
    let normalized = collapse_consecutive(tool_calls.iter().map(|tc| tc.tool_name.clone()));
    let hash = hash_tool_names(&normalized);
    SequenceSignature { normalized, hash }
}

/// Compare two signatures for similarity.
///
/// Returns a value in `[0.0, 1.0]` where:
/// - `1.0` = identical sequences
/// - `0.8` = threshold for "same pattern"
/// - `0.0` = completely different
///
/// Uses `LCS(a, b) / max(|a|, |b|)` (Longest Common Subsequence ratio).
#[expect(
    clippy::cast_precision_loss,
    reason = "sequence lengths are small; precision loss is impossible in practice"
)]
pub fn signature_similarity(a: &SequenceSignature, b: &SequenceSignature) -> f64 {
    if a.hash == b.hash && a.normalized == b.normalized {
        return 1.0;
    }
    let max_len = a.len().max(b.len());
    if max_len == 0 {
        return 1.0;
    }
    let lcs = lcs_length(&a.normalized, &b.normalized);
    lcs as f64 / max_len as f64
}

/// Collapse consecutive duplicate elements.
fn collapse_consecutive(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in names {
        if out.last() != Some(&name) {
            out.push(name);
        }
    }
    out
}

/// Stable hash of a tool-name slice using [`DefaultHasher`].
fn hash_tool_names(names: &[String]) -> u64 {
    let mut hasher = DefaultHasher::new();
    names.hash(&mut hasher);
    hasher.finish()
}

/// Classic DP Longest Common Subsequence length.
fn lcs_length(a: &[String], b: &[String]) -> usize {
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
    use crate::skills::ToolCallRecord;

    fn tc(name: &str) -> ToolCallRecord {
        ToolCallRecord::new(name, 10)
    }

    fn sig(names: &[&str]) -> SequenceSignature {
        let calls: Vec<ToolCallRecord> = names.iter().map(|n| tc(n)).collect();
        sequence_signature(&calls)
    }

    // ------------------------------------------------------------------
    // Normalization
    // ------------------------------------------------------------------

    #[test]
    fn consecutive_duplicates_collapsed() {
        let calls = vec![tc("Read"), tc("Read"), tc("Read"), tc("Edit"), tc("Bash")];
        let s = sequence_signature(&calls);
        assert_eq!(s.normalized, vec!["Read", "Edit", "Bash"]);
    }

    #[test]
    fn non_consecutive_duplicates_kept() {
        let calls = vec![tc("Read"), tc("Edit"), tc("Read"), tc("Bash")];
        let s = sequence_signature(&calls);
        assert_eq!(s.normalized, vec!["Read", "Edit", "Read", "Bash"]);
    }

    #[test]
    fn empty_sequence_produces_empty_signature() {
        let s = sequence_signature(&[]);
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
    }

    // ------------------------------------------------------------------
    // Hash stability
    // ------------------------------------------------------------------

    #[test]
    fn same_sequence_same_hash() {
        let a = sig(&["Read", "Edit", "Bash"]);
        let b = sig(&["Read", "Edit", "Bash"]);
        assert_eq!(a.hash, b.hash);
    }

    #[test]
    fn different_sequence_different_hash() {
        let a = sig(&["Read", "Edit", "Bash"]);
        let b = sig(&["Grep", "Write", "Bash"]);
        assert_ne!(a.hash, b.hash);
    }

    #[test]
    fn hash_stable_across_calls() {
        // Hash must not change between calls (no random seed)
        let a1 = sig(&["Read", "Grep", "Edit"]);
        let a2 = sig(&["Read", "Grep", "Edit"]);
        assert_eq!(a1.hash, a2.hash);
    }

    // ------------------------------------------------------------------
    // Similarity
    // ------------------------------------------------------------------

    #[test]
    fn identical_signatures_similarity_one() {
        let a = sig(&["Read", "Edit", "Bash"]);
        let b = sig(&["Read", "Edit", "Bash"]);
        assert!((signature_similarity(&a, &b) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn completely_different_similarity_zero() {
        let a = sig(&["Read", "Edit", "Bash"]);
        let b = sig(&["WebSearch", "WebFetch", "Write"]);
        // LCS of ["Read","Edit","Bash"] and ["WebSearch","WebFetch","Write"] = 0
        assert!(signature_similarity(&a, &b) < f64::EPSILON);
    }

    #[test]
    fn partially_similar_sequence() {
        let a = sig(&["Grep", "Read", "Edit", "Bash"]);
        let b = sig(&["Grep", "Read", "Write", "Bash"]);
        // LCS = ["Grep","Read","Bash"] = 3, max = 4 → 0.75
        let sim = signature_similarity(&a, &b);
        assert!((sim - 0.75).abs() < 0.001);
    }

    #[test]
    fn subset_similarity_below_threshold() {
        let a = sig(&["Read", "Edit", "Bash", "Read", "Edit", "Bash"]);
        let b = sig(&["Read", "Edit"]);
        // LCS = 2, max = 6 → ~0.33
        let sim = signature_similarity(&a, &b);
        assert!(sim < 0.8);
    }

    #[test]
    fn high_overlap_above_threshold() {
        let a = sig(&["Grep", "Read", "Read", "Edit", "Bash"]);
        let b = sig(&["Grep", "Read", "Edit", "Edit", "Bash"]);
        // After collapse: a=["Grep","Read","Edit","Bash"], b=["Grep","Read","Edit","Bash"]
        // LCS = 4, max = 4 → 1.0
        let sim = signature_similarity(&a, &b);
        assert!(sim >= 0.8, "expected sim >= 0.8, got {sim}");
    }

    #[test]
    fn empty_signatures_similarity_one() {
        let a = sequence_signature(&[]);
        let b = sequence_signature(&[]);
        assert!((signature_similarity(&a, &b) - 1.0).abs() < f64::EPSILON);
    }
}
