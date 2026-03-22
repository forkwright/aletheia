//! Cost estimation and retry backoff utilities.

use std::collections::HashMap;
use std::time::Duration;

use rand::Rng as _;

use crate::error;
use crate::models::{BACKOFF_BASE_MS, BACKOFF_FACTOR, BACKOFF_MAX_MS};
use crate::provider::ModelPricing;

/// Derive the model family name by stripping the last dash-separated segment.
///
/// This lets versioned aliases and dated snapshots of the same model family
/// share a single pricing entry.  Examples:
///
/// | Input                        | Output             |
/// |------------------------------|--------------------|
/// | `claude-sonnet-4-20250514`   | `claude-sonnet-4`  |
/// | `claude-sonnet-4-6`          | `claude-sonnet-4`  |
/// | `claude-haiku-4-5-20251001`  | `claude-haiku-4-5` |
/// | `claude-haiku-4-5`           | `claude-haiku-4`   |
pub(crate) fn model_family(model: &str) -> &str {
    model
        .rfind('-')
        .map_or(model, |pos| model.get(..pos).unwrap_or(model))
}

/// Estimate cost using configured pricing.
///
/// Lookup order:
/// 1. Exact model ID match.
/// 2. Family match: any pricing key whose [`model_family`] matches the
///    requested model's family (e.g. `claude-sonnet-4-6` covers
///    `claude-sonnet-4-20250514`).
///
/// Returns `0.0` and logs a warning when neither lookup succeeds.
pub(crate) fn estimate_cost(
    pricing: &HashMap<String, ModelPricing>,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> f64 {
    let p = if let Some(exact) = pricing.get(model) {
        exact
    } else {
        let family = model_family(model);
        if let Some((_, matched)) = pricing.iter().find(|(key, _)| model_family(key) == family) {
            matched
        } else if let Some((_, matched)) = pricing.iter().find(|(key, _)| {
            // WHY: model_family("claude-haiku-4-5") = "claude-haiku-4", which differs from
            // model_family("claude-haiku-4-5-20251001") = "claude-haiku-4-5".  The family
            // check above misses this case.  A prefix check catches dated-snapshot variants
            // whose model ID contains a second numeric component (e.g. haiku-4-5) so that
            // the last-segment strip produces a different family string.
            model.len() > key.len()
                && model.starts_with(key.as_str())
                && model.as_bytes().get(key.len()) == Some(&b'-')
        }) {
            matched
        } else {
            tracing::warn!(model, "no pricing configured for model; cost reported as 0");
            return 0.0;
        }
    };
    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "u64→f64 for token counts: acceptable precision loss for cost estimates"
    )]
    {
        (input_tokens as f64 * p.input_cost_per_mtok // kanon:ignore RUST/as-cast
            + output_tokens as f64 * p.output_cost_per_mtok) // kanon:ignore RUST/as-cast
            / 1_000_000.0
    }
}

pub(crate) fn backoff_delay(attempt: u32, last_error: Option<&error::Error>) -> Duration {
    if let Some(error::Error::RateLimited { retry_after_ms, .. }) = last_error {
        return Duration::from_millis(*retry_after_ms);
    }

    let base = BACKOFF_BASE_MS * BACKOFF_FACTOR.pow(attempt.saturating_sub(1));
    let capped = base.min(BACKOFF_MAX_MS);

    // WHY: ±25% random jitter: prevents thundering herd under concurrent load
    let jitter_range = capped / 4;
    let delay = if jitter_range > 0 {
        let offset = rand::rng().random_range(0..jitter_range * 2);
        capped - jitter_range + offset
    } else {
        capped
    };

    Duration::from_millis(delay.max(100))
}
