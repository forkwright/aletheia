//! `TokenEstimator` trait implementations.

use crate::budget::TokenEstimator;

/// Character-based token estimator: 1 token ≈ N characters (ceiling division).
///
/// Conservative estimate suitable for budget planning. Actual token counts
/// from the Anthropic API will be lower, giving natural headroom.
/// `chars_per_token` is configurable via `agents.defaults.chars_per_token`
/// in `aletheia.toml`; the default of 4 preserves prior behaviour.
pub struct CharEstimator {
    pub(crate) chars_per_token: u64,
}

impl CharEstimator {
    /// Create an estimator with an explicit characters-per-token divisor.
    #[must_use]
    pub fn new(chars_per_token: u64) -> Self {
        Self { chars_per_token }
    }
}

impl Default for CharEstimator {
    fn default() -> Self {
        // WHY: 4 chars per token is the classic heuristic for English text and
        //      matches the historical hardcoded value: no behaviour change.
        Self { chars_per_token: 4 }
    }
}

impl TokenEstimator for CharEstimator {
    fn estimate(&self, text: &str) -> u64 {
        #[expect(
            clippy::as_conversions,
            reason = "usize→u64: text length always fits in u64"
        )]
        {
            (text.len() as u64).div_ceil(self.chars_per_token) // kanon:ignore RUST/as-cast
        }
    }
}
