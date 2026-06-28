//! Cost estimation utilities.

use std::collections::HashMap;

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

fn lookup_pricing_entry<'a>(
    pricing: &'a HashMap<String, ModelPricing>,
    model: &str,
) -> Option<&'a ModelPricing> {
    if let Some(exact) = pricing.get(model) {
        return Some(exact);
    }

    let family = model_family(model);
    if let Some((_, matched)) = pricing.iter().find(|(key, _)| model_family(key) == family) {
        return Some(matched);
    }

    pricing
        .iter()
        .find(|(key, _)| {
            // WHY: model_family("claude-haiku-4-5") = "claude-haiku-4", which differs from
            // model_family("claude-haiku-4-5-20251001") = "claude-haiku-4-5".  The family
            // check above misses this case.  A prefix check catches dated-snapshot variants
            // whose model ID contains a second numeric component (e.g. haiku-4-5) so that
            // the last-segment strip produces a different family string.
            model.len() > key.len()
                && model.starts_with(key.as_str())
                && model.as_bytes().get(key.len()) == Some(&b'-')
        })
        .map(|(_, matched)| matched)
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
    let Some(p) = lookup_pricing_entry(pricing, model) else {
        tracing::warn!(model, "no pricing configured for model; cost reported as 0");
        return 0.0;
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

/// Estimate cost including prompt cache token pricing.
///
/// Uses Anthropic's standard cache pricing ratios:
/// - cache reads: 10% of input price
/// - cache writes: 125% of input price
pub(crate) fn estimate_cost_with_cache(
    pricing: &HashMap<String, ModelPricing>,
    model: &str,
    usage: &crate::types::Usage,
) -> f64 {
    let base = estimate_cost(pricing, model, usage.input_tokens, usage.output_tokens);
    let Some(p) = lookup_pricing_entry(pricing, model) else {
        return base;
    };
    #[expect(
        clippy::cast_precision_loss,
        clippy::as_conversions,
        reason = "u64->f64 for token counts: acceptable precision loss for cost estimates"
    )]
    {
        base + (usage.cache_read_tokens as f64 * p.input_cost_per_mtok * koina::models::cache_read_ratio() // kanon:ignore RUST/as-cast
            + usage.cache_write_tokens as f64 * p.input_cost_per_mtok * koina::models::cache_write_ratio()) // kanon:ignore RUST/as-cast
            / 1_000_000.0
    }
}
