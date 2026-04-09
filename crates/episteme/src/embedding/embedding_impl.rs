//! Embedding provider trait implementations.

#[cfg(any(test, feature = "test-support"))]
use tracing::instrument;

use crate::embedding::{DegradedEmbeddingProvider, EmbedFailedSnafu, EmbeddingProvider, EmbeddingResult};
#[cfg(any(test, feature = "test-support"))]
use crate::embedding::MockEmbeddingProvider;

// ── MockEmbeddingProvider implementation ─────────────────────────────────────

#[cfg(any(test, feature = "test-support"))]
impl EmbeddingProvider for MockEmbeddingProvider {
    #[cfg_attr(any(test, feature = "test-support"), instrument(skip(self, text)))]
    fn embed(&self, text: &str) -> EmbeddingResult<Vec<f32>> {
        let mut vec = vec![0.0f32; self.dim];
        let bytes = text.as_bytes();
        let mut hash: u64 = 5381;
        for &b in bytes {
            hash = hash.wrapping_mul(33).wrapping_add(u64::from(b));
        }
        for (i, v) in vec.iter_mut().enumerate() {
            #[expect(
                clippy::as_conversions,
                reason = "embedding dim is small, usize fits u64 safely"
            )]
            let idx = i as u64;
            let h = hash.wrapping_mul(idx + 1).wrapping_add(idx * 2_654_435_761);
            #[expect(
                clippy::as_conversions,
                reason = "h % 10000 is bounded to 0..=9999, safe for f32"
            )]
            #[expect(
                clippy::cast_precision_loss,
                reason = "h % 10000 fits within f32 mantissa exactly"
            )]
            let hf = (h % 10000) as f32;
            *v = (hf / 5000.0) - 1.0;
        }
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut vec {
                *v /= norm;
            }
        }
        Ok(vec)
    }

    fn dimension(&self) -> usize {
        self.dim
    }

    fn model_name(&self) -> &str {
        "mock-embedding"
    }
}

// ── DegradedEmbeddingProvider implementation ─────────────────────────────────

impl EmbeddingProvider for DegradedEmbeddingProvider {
    fn embed(&self, _text: &str) -> EmbeddingResult<Vec<f32>> {
        EmbedFailedSnafu {
            message: "embedding unavailable: server started in degraded mode \
                      (embedding model failed to load at startup)"
                .to_owned(),
        }
        .fail()
    }

    fn dimension(&self) -> usize {
        self.dim
    }

    fn model_name(&self) -> &'static str {
        "degraded-embedding"
    }
}
