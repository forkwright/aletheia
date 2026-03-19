//! Property-based tests for the mock embedding provider.
//!
//! Enabled by `--features test-support`.
//! Failing seeds are stored in `tests/proptest-regressions/proptest_embedding.txt`.
//!
//! Run: `cargo test -p aletheia-mneme --features test-support --test proptest_embedding`

#![expect(
    clippy::unwrap_used,
    reason = "proptest test bodies may panic on failed assertions"
)]

use aletheia_mneme::embedding::{EmbeddingProvider, MockEmbeddingProvider};
use proptest::prelude::*;

proptest! {
    /// Embed always returns a vector of the specified dimension.
    #[test]
    fn embed_dimension_matches(
        dim in 4usize..=512usize,
        text in ".*".prop_map(|s| s.chars().take(256).collect::<String>()),
    ) {
        let provider = MockEmbeddingProvider::new(dim);
        let result = provider.embed(&text).unwrap();
        prop_assert_eq!(
            result.len(),
            dim,
            "embed must return exactly the configured number of floats"
        );
    }

    /// Embed returns the same vector for the same input (determinism).
    #[test]
    fn embed_is_deterministic(
        dim in 8usize..=128usize,
        text in "[a-zA-Z0-9 ]{1,64}",
    ) {
        let provider = MockEmbeddingProvider::new(dim);
        let first = provider.embed(&text).unwrap();
        let second = provider.embed(&text).unwrap();
        prop_assert_eq!(
            first, second,
            "embed must be deterministic for identical input"
        );
    }

    /// Embed produces a unit vector (L2 norm ≈ 1.0).
    #[test]
    fn embed_produces_unit_vector(
        dim in 4usize..=256usize,
        text in "[a-zA-Z]{1,32}",
    ) {
        let provider = MockEmbeddingProvider::new(dim);
        let vec = provider.embed(&text).unwrap();
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        let diff = (norm - 1.0_f32).abs();
        prop_assert!(
            diff < 1e-4,
            "L2 norm of embedded vector must be ≈ 1.0, got {norm} (diff {diff})"
        );
    }

    /// embed_batch dimension and count match single calls.
    #[test]
    fn embed_batch_matches_single(
        dim in 8usize..=64usize,
        texts in proptest::collection::vec("[a-zA-Z]{1,32}", 1..=8),
    ) {
        let provider = MockEmbeddingProvider::new(dim);
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let batch = provider.embed_batch(&refs).unwrap();
        prop_assert_eq!(
            batch.len(),
            texts.len(),
            "embed_batch must return one vector per input"
        );
        for (single_text, batch_vec) in texts.iter().zip(batch.iter()) {
            let single = provider.embed(single_text).unwrap();
            prop_assert_eq!(
                &single,
                batch_vec,
                "each embed_batch entry must match the corresponding individual embed() call"
            );
        }
    }
}
