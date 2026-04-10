//! Bayesian surprise for episode boundary detection.
//!
//! Based on EM-LLM (ICLR 2025, arXiv 2407.09450). Uses KL divergence between
//! a running character n-gram distribution and the distribution after observing
//! new text to detect topic shifts. High surprise signals an episode boundary.
//!
//! The running distribution uses an exponential moving average so it adapts to
//! gradual topic drift without flagging every minor variation.

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default surprise threshold (in nats) above which a turn is classified as an
/// episode boundary. Empirically, bigram KL divergence on conversational text
/// clusters around 0.5-1.5 for same-topic turns and 2.0+ for topic shifts.
const DEFAULT_THRESHOLD: f64 = 2.0;

/// Exponential moving average decay factor. Controls how quickly the running
/// distribution forgets old observations. 0.3 = new observation gets 30% weight.
const EMA_ALPHA: f64 = 0.3;

/// Laplace smoothing constant added to all n-gram counts to avoid zero
/// probabilities (and thus infinite KL divergence).
const SMOOTHING: f64 = 1e-10;

/// N-gram size. Bigrams balance granularity with vocabulary sparsity.
const NGRAM_SIZE: usize = 2;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A detected (or non-detected) episode boundary at a specific turn.
#[derive(Debug, Clone)]
pub struct EpisodeBoundary {
    /// Zero-based turn index within the sequence.
    pub position: usize,
    /// KL divergence (surprise) measured at this turn, in nats.
    pub surprise_score: f64,
    /// Whether the surprise exceeded the threshold.
    pub is_boundary: bool,
}

/// Maintains a running n-gram distribution and computes Bayesian surprise for
/// incoming text segments.
///
/// # Example
///
/// ```
/// use aletheia_episteme::surprise::SurpriseCalculator;
///
/// let mut calc = SurpriseCalculator::new();
/// let score = calc.compute_surprise("hello world");
/// assert!(score >= 0.0);
/// ```
#[derive(Debug, Clone)]
pub struct SurpriseCalculator {
    /// Running (prior) bigram distribution, values are normalized frequencies.
    prior: HashMap<[u8; NGRAM_SIZE], f64>,
    /// Total observation mass in the prior (for re-normalization after EMA).
    total_mass: f64,
}

impl Default for SurpriseCalculator {
    fn default() -> Self {
        Self::new()
    }
}

impl SurpriseCalculator {
    /// Create a calculator with an empty prior distribution.
    ///
    /// The first call to [`compute_surprise`](Self::compute_surprise) will
    /// return 0.0 because there is no prior to diverge from.
    ///
    /// # Complexity
    ///
    /// O(1).
    #[must_use]
    pub fn new() -> Self {
        Self {
            prior: HashMap::new(),
            total_mass: 0.0,
        }
    }

    /// Compute the Bayesian surprise (KL divergence) introduced by `text`.
    ///
    /// Returns the KL divergence `D_KL(posterior || prior)` in nats. The prior
    /// is then updated via exponential moving average so subsequent calls
    /// reflect the adapted distribution.
    ///
    /// Returns 0.0 for empty text or when the prior is empty (first call).
    ///
    /// # Complexity
    ///
    /// O(n) where n = `text.len()`.
    pub fn compute_surprise(&mut self, text: &str) -> f64 {
        let observed = bigram_frequencies(text);
        if observed.is_empty() {
            return 0.0;
        }

        // First observation: bootstrap the prior, no surprise.
        if self.prior.is_empty() {
            self.prior = observed;
            self.total_mass = 1.0;
            return 0.0;
        }

        let surprise = kl_divergence(&observed, &self.prior);

        // EMA update: prior = (1 - alpha) * prior + alpha * observed
        self.update_prior(&observed);

        surprise
    }

    /// Update the running prior via exponential moving average.
    fn update_prior(&mut self, observed: &HashMap<[u8; NGRAM_SIZE], f64>) {
        let retain = 1.0 - EMA_ALPHA;

        // Scale down existing entries.
        for v in self.prior.values_mut() {
            *v *= retain;
        }

        // Blend in observed distribution.
        for (&ngram, &freq) in observed {
            *self.prior.entry(ngram).or_insert(0.0) += EMA_ALPHA * freq;
        }

        // Re-normalize so the distribution sums to 1.
        let sum: f64 = self.prior.values().sum();
        if sum > 0.0 {
            for v in self.prior.values_mut() {
                *v /= sum;
            }
        }

        self.total_mass = 1.0;
    }
}

// ---------------------------------------------------------------------------
// Boundary detection
// ---------------------------------------------------------------------------

/// Scan a sequence of turns and identify episode boundaries where Bayesian
/// surprise exceeds `threshold`.
///
/// Each turn is scored against the running distribution maintained by an
/// internal [`SurpriseCalculator`]. The first turn always has surprise 0.0
/// (no prior to compare against).
///
/// # Complexity
///
/// O(T * N) where T = number of turns, N = average turn length.
#[must_use]
pub fn detect_boundaries(turns: &[&str], threshold: f64) -> Vec<EpisodeBoundary> {
    let mut calc = SurpriseCalculator::new();
    turns
        .iter()
        .enumerate()
        .map(|(i, turn)| {
            let score = calc.compute_surprise(turn);
            EpisodeBoundary {
                position: i,
                surprise_score: score,
                is_boundary: score > threshold,
            }
        })
        .collect()
}

/// Convenience wrapper using [`DEFAULT_THRESHOLD`].
///
/// # Complexity
///
/// O(T * N) — same as [`detect_boundaries`].
#[must_use]
pub fn detect_boundaries_default(turns: &[&str]) -> Vec<EpisodeBoundary> {
    detect_boundaries(turns, DEFAULT_THRESHOLD)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract character bigram frequencies from `text`, normalized to sum to 1.
fn bigram_frequencies(text: &str) -> HashMap<[u8; NGRAM_SIZE], f64> {
    let bytes = text.as_bytes();
    if bytes.len() < NGRAM_SIZE {
        return HashMap::new();
    }

    let mut counts: HashMap<[u8; NGRAM_SIZE], u64> = HashMap::new();
    let mut total = 0u64;

    for window in bytes.windows(NGRAM_SIZE) {
        let mut key = [0u8; NGRAM_SIZE];
        key.copy_from_slice(window);
        *counts.entry(key).or_insert(0) += 1;
        total += 1;
    }

    if total == 0 {
        return HashMap::new();
    }

    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "u64→f64: bigram counts in single text never approach 2^53"
    )]
    let total_f = total as f64;
    counts
        .into_iter()
        .map(|(k, v)| {
            #[expect(
                clippy::cast_precision_loss,
                clippy::as_conversions,
                reason = "u64→f64: individual bigram count <= total, safe"
            )]
            let freq = v as f64 / total_f;
            (k, freq)
        })
        .collect()
}

/// KL divergence `D_KL(P || Q)` with Laplace smoothing.
///
/// Computes `sum_x P(x) * ln(P(x) / Q(x))` over all n-grams present in
/// either distribution, using [`SMOOTHING`] to avoid log(0).
fn kl_divergence(
    p: &HashMap<[u8; NGRAM_SIZE], f64>,
    q: &HashMap<[u8; NGRAM_SIZE], f64>,
) -> f64 {
    // Collect the union of keys.
    let mut all_keys: Vec<&[u8; NGRAM_SIZE]> = p.keys().collect();
    for k in q.keys() {
        if !p.contains_key(k) {
            all_keys.push(k);
        }
    }

    let mut divergence = 0.0f64;
    for &key in &all_keys {
        let p_val = p.get(key).copied().unwrap_or(0.0) + SMOOTHING;
        let q_val = q.get(key).copied().unwrap_or(0.0) + SMOOTHING;
        divergence += p_val * (p_val / q_val).ln();
    }

    divergence.max(0.0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "test assertions on values known to be exactly 0.0"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "test code with known-valid indices"
)]
mod tests {
    use super::*;

    /// Repeated identical text should produce low/zero surprise after the first.
    #[test]
    fn constant_topic_low_surprise() {
        let mut calc = SurpriseCalculator::new();
        let text = "the weather is sunny and warm today";

        let s0 = calc.compute_surprise(text);
        assert_eq!(s0, 0.0, "first observation should be zero");

        // Feed the same text several times; surprise should stay near zero.
        for i in 1..=5 {
            let s = calc.compute_surprise(text);
            assert!(
                s < 0.1,
                "iteration {i}: identical text surprise {s} should be < 0.1"
            );
        }
    }

    /// A drastic topic shift should produce high surprise.
    #[test]
    fn topic_shift_high_surprise() {
        let mut calc = SurpriseCalculator::new();

        // Establish a prior around cooking.
        for _ in 0..5 {
            calc.compute_surprise(
                "chop the onions and garlic then saute in olive oil until golden",
            );
        }

        // Shift to astrophysics.
        let surprise = calc.compute_surprise(
            "the schwarzschild radius of a black hole is proportional to its mass",
        );

        assert!(
            surprise > 0.5,
            "topic shift surprise {surprise} should be > 0.5"
        );
    }

    /// Empty and very short inputs should not panic and should return 0.0.
    #[test]
    fn empty_and_short_input() {
        let mut calc = SurpriseCalculator::new();

        assert_eq!(calc.compute_surprise(""), 0.0);
        assert_eq!(calc.compute_surprise("a"), 0.0); // < NGRAM_SIZE
        assert_eq!(calc.compute_surprise(""), 0.0);

        // After bootstrapping with real text, empty should still be 0.
        calc.compute_surprise("some real text here");
        assert_eq!(calc.compute_surprise(""), 0.0);
        assert_eq!(calc.compute_surprise("x"), 0.0);
    }

    /// `detect_boundaries` should flag turns that exceed the threshold.
    #[test]
    fn threshold_boundary_detection() {
        let turns: Vec<&str> = vec![
            "the cat sat on the mat",
            "the cat sat on the mat",
            "the cat sat on the mat",
            // Sharp shift:
            "quantum entanglement violates bell inequalities",
            // Continue new topic:
            "quantum decoherence destroys superposition states",
        ];

        let boundaries = detect_boundaries(&turns, 0.5);

        assert_eq!(boundaries.len(), 5);
        // First turn: no prior, surprise = 0, not a boundary.
        assert!(!boundaries[0].is_boundary);
        assert_eq!(boundaries[0].position, 0);
        // Repeated same-topic turns should not be boundaries.
        assert!(!boundaries[1].is_boundary);
        assert!(!boundaries[2].is_boundary);
        // The topic shift turn should be flagged.
        assert!(
            boundaries[3].is_boundary,
            "turn 3 surprise {} should exceed 0.5",
            boundaries[3].surprise_score
        );
    }

    /// The EMA should cause the prior to adapt, reducing surprise over time
    /// when a new topic persists.
    #[test]
    fn ema_adaptation() {
        let mut calc = SurpriseCalculator::new();

        // Establish prior.
        for _ in 0..5 {
            calc.compute_surprise("functional programming with immutable data structures");
        }

        // First exposure to new topic: high surprise.
        let first = calc.compute_surprise(
            "the mitochondria is the powerhouse of the cell in biology",
        );

        // Repeated new topic: surprise should decrease as prior adapts.
        let mut previous = first;
        for i in 0..5 {
            let s = calc.compute_surprise(
                "cellular respiration produces adenosine triphosphate in mitochondria",
            );
            assert!(
                s <= previous + 0.01, // small tolerance for floating-point
                "iteration {i}: surprise {s} should be <= previous {previous} (EMA adapting)"
            );
            previous = s;
        }

        // After adaptation, surprise should be much lower than initial shock.
        assert!(
            previous < first * 0.5,
            "adapted surprise {previous} should be < 50% of initial {first}"
        );
    }

    /// Default threshold wrapper should produce the same results.
    #[test]
    fn default_threshold_wrapper() {
        let turns: Vec<&str> = vec!["hello world", "hello world"];
        let boundaries = detect_boundaries_default(&turns);
        assert_eq!(boundaries.len(), 2);
        // Same text, low surprise, no boundaries at default threshold.
        assert!(!boundaries[0].is_boundary);
        assert!(!boundaries[1].is_boundary);
    }

    /// KL divergence of a distribution with itself should be ~0.
    #[test]
    fn kl_self_divergence_near_zero() {
        let freq = bigram_frequencies("hello world this is a test");
        let div = kl_divergence(&freq, &freq);
        assert!(
            div < 1e-6,
            "self-divergence {div} should be near zero"
        );
    }

    /// Bigram frequencies should sum to approximately 1.
    #[test]
    fn bigram_frequencies_normalized() {
        let freq = bigram_frequencies("the quick brown fox jumps over the lazy dog");
        let sum: f64 = freq.values().sum();
        assert!(
            (sum - 1.0).abs() < 1e-9,
            "frequencies should sum to 1.0, got {sum}"
        );
    }
}
