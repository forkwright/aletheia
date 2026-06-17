//! Shared model catalog loaded from the compiled model seed.

use std::fmt;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

const MODEL_SEED_TOML: &str = include_str!("../data/model-seed.toml");

/// Model capability tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ModelTier {
    /// No model call required; a deterministic fast path can handle it.
    #[serde(rename = "no_llm", alias = "no-llm")]
    NoLlm,
    /// Fast, cheap, sufficient for simple queries.
    Haiku,
    /// Balanced capability and cost.
    Sonnet,
    /// Maximum capability for hard problems.
    Opus,
}

impl fmt::Display for ModelTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoLlm => f.write_str("no_llm"),
            Self::Haiku => f.write_str("haiku"),
            Self::Sonnet => f.write_str("sonnet"),
            Self::Opus => f.write_str("opus"),
        }
    }
}

/// Provider family that owns a model catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum ModelProvider {
    /// Anthropic Messages API models.
    Anthropic,
    /// Codex CLI models.
    Codex,
    /// Kimi CLI models.
    Kimi,
}

/// Built-in task role whose default model is a tier reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TaskRole {
    /// Implementation, testing, and debugging.
    Coder,
    /// Investigation, comparison, and documentation.
    Researcher,
    /// Code review and risk assessment.
    Reviewer,
    /// Codebase exploration.
    Explorer,
    /// Command-running task execution.
    Runner,
    /// Background heartbeat checks.
    Prosoche,
    /// Knowledge extraction.
    Extraction,
    /// Dispatch triage prompt generation.
    TriagePrompt,
}

/// Per-model pricing rates for cost estimation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ModelPrice {
    /// Cost per million input tokens in USD.
    pub input_cost_per_mtok: f64,
    /// Cost per million output tokens in USD.
    pub output_cost_per_mtok: f64,
}

/// Select-list entry for setup surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelMenuOption {
    /// Model identifier written into config.
    pub value: &'static str,
    /// Human-readable menu label derived from the model identifier.
    pub label: &'static str,
}

#[derive(Debug, Deserialize)]
struct ModelSeed {
    as_of: String,
    cache: CacheSeed,
    tiers: TierSeed,
    task_roles: TaskRoleSeed,
    models: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
struct CacheSeed {
    read_ratio: f64,
    write_ratio: f64,
}

#[derive(Debug, Deserialize)]
struct TierSeed {
    opus: String,
    sonnet: String,
    haiku: String,
}

#[derive(Debug, Deserialize)]
struct TaskRoleSeed {
    coder: ModelTier,
    researcher: ModelTier,
    reviewer: ModelTier,
    explorer: ModelTier,
    runner: ModelTier,
    prosoche: ModelTier,
    extraction: ModelTier,
    triage_prompt: ModelTier,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
    provider: ModelProvider,
    tier: ModelTier,
    family: String,
    context_tokens: u32,
    input_cost_per_mtok: Option<f64>,
    output_cost_per_mtok: Option<f64>,
    #[serde(default)]
    menu: bool,
    #[serde(default)]
    recommended: bool,
}

// WHY (#5635): `data/model-seed.toml` is parsed at build time in `build.rs`;
// any malformed file fails compilation. The runtime parse is therefore
// guaranteed to succeed, so `expect` documents an unreachable invariant.
#[expect(
    clippy::expect_used,
    reason = "model-seed.toml is validated at build time in build.rs"
)]
static MODEL_SEED: LazyLock<ModelSeed> =
    LazyLock::new(|| toml::from_str(MODEL_SEED_TOML).expect("compiled model seed must parse"));

static ANTHROPIC_MODELS: LazyLock<Box<[&'static str]>> =
    LazyLock::new(|| models_for_provider_boxed(ModelProvider::Anthropic));
static CODEX_MODELS: LazyLock<Box<[&'static str]>> =
    LazyLock::new(|| models_for_provider_boxed(ModelProvider::Codex));
static KIMI_MODELS: LazyLock<Box<[&'static str]>> =
    LazyLock::new(|| models_for_provider_boxed(ModelProvider::Kimi));
static MENU_OPTIONS: LazyLock<Box<[ModelMenuOption]>> = LazyLock::new(model_menu_options_boxed);

/// Namespaced helpers for common model aliases.
pub mod names {
    use super::{ModelTier, model_for_provider, tier_default};

    /// Default Opus-tier model alias.
    #[must_use]
    pub fn opus() -> &'static str {
        tier_default(ModelTier::Opus)
    }

    /// Default Sonnet-tier model alias.
    #[must_use]
    pub fn sonnet() -> &'static str {
        tier_default(ModelTier::Sonnet)
    }

    /// Default Haiku-tier model alias.
    #[must_use]
    pub fn haiku() -> &'static str {
        tier_default(ModelTier::Haiku)
    }

    /// Default Codex model.
    #[must_use]
    pub fn codex() -> &'static str {
        model_for_provider(super::ModelProvider::Codex)
    }

    /// Default Kimi model.
    #[must_use]
    pub fn kimi() -> &'static str {
        model_for_provider(super::ModelProvider::Kimi)
    }
}

/// Date when the compiled seed values were last verified.
#[must_use]
pub fn as_of() -> &'static str {
    &MODEL_SEED.as_of
}

/// Default model identifier for a capability tier.
#[must_use]
pub fn tier_default(tier: ModelTier) -> &'static str {
    match tier {
        ModelTier::NoLlm | ModelTier::Haiku => &MODEL_SEED.tiers.haiku,
        ModelTier::Sonnet => &MODEL_SEED.tiers.sonnet,
        ModelTier::Opus => &MODEL_SEED.tiers.opus,
    }
}

/// Default tier for a built-in task role.
#[must_use]
pub fn task_role_tier(role: TaskRole) -> ModelTier {
    let roles = &MODEL_SEED.task_roles;
    match role {
        TaskRole::Coder => roles.coder,
        TaskRole::Researcher => roles.researcher,
        TaskRole::Reviewer => roles.reviewer,
        TaskRole::Explorer => roles.explorer,
        TaskRole::Runner => roles.runner,
        TaskRole::Prosoche => roles.prosoche,
        TaskRole::Extraction => roles.extraction,
        TaskRole::TriagePrompt => roles.triage_prompt,
    }
}

/// Default model identifier for a built-in task role.
#[must_use]
pub fn task_role_default(role: TaskRole) -> &'static str {
    tier_default(task_role_tier(role))
}

/// Prompt-cache read cost multiplier relative to input tokens.
#[must_use]
pub fn cache_read_ratio() -> f64 {
    MODEL_SEED.cache.read_ratio
}

/// Prompt-cache write cost multiplier relative to input tokens.
#[must_use]
pub fn cache_write_ratio() -> f64 {
    MODEL_SEED.cache.write_ratio
}

/// Model identifiers claimed by a built-in provider catalog.
#[must_use]
pub fn provider_models(provider: ModelProvider) -> &'static [&'static str] {
    match provider {
        ModelProvider::Anthropic => &ANTHROPIC_MODELS,
        ModelProvider::Codex => &CODEX_MODELS,
        ModelProvider::Kimi => &KIMI_MODELS,
    }
}

/// First model identifier for a provider catalog.
#[must_use]
pub fn model_for_provider(provider: ModelProvider) -> &'static str {
    provider_models(provider).first().copied().unwrap_or("")
}

/// Model menu options for setup surfaces.
#[must_use]
pub fn model_menu_options() -> &'static [ModelMenuOption] {
    &MENU_OPTIONS
}

/// Pricing rows from the model catalog.
pub fn pricing_entries() -> impl Iterator<Item = (&'static str, ModelPrice)> {
    MODEL_SEED.models.iter().filter_map(|model| {
        let input = model.input_cost_per_mtok?;
        let output = model.output_cost_per_mtok?;
        Some((
            model.id.as_str(),
            ModelPrice {
                input_cost_per_mtok: input,
                output_cost_per_mtok: output,
            },
        ))
    })
}

/// Context window for a model's family, when the model is in the seed.
#[must_use]
pub fn context_tokens(model_id: &str) -> Option<u32> {
    MODEL_SEED
        .models
        .iter()
        .find(|model| model.id == model_id)
        .map(|model| model.context_tokens)
}

/// Family identifier for a model, when the model is in the seed.
#[must_use]
pub fn family(model_id: &str) -> Option<&'static str> {
    MODEL_SEED
        .models
        .iter()
        .find(|model| model.id == model_id)
        .map(|model| model.family.as_str())
}

/// Capability tier for a model, when the model is in the seed.
#[must_use]
pub fn model_tier(model_id: &str) -> Option<ModelTier> {
    MODEL_SEED
        .models
        .iter()
        .find(|model| model.id == model_id)
        .map(|model| model.tier)
}

fn models_for_provider_boxed(provider: ModelProvider) -> Box<[&'static str]> {
    MODEL_SEED
        .models
        .iter()
        .filter(|model| model.provider == provider)
        .map(|model| model.id.as_str())
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

fn model_menu_options_boxed() -> Box<[ModelMenuOption]> {
    let mut options = MODEL_SEED
        .models
        .iter()
        .filter(|model| model.menu)
        .map(|model| {
            let label = if model.recommended {
                Box::leak(format!("{} (recommended)", model.id).into_boxed_str())
            } else {
                model.id.as_str()
            };
            ModelMenuOption {
                value: model.id.as_str(),
                label,
            }
        })
        .collect::<Vec<_>>();
    options.sort_by_key(|option| usize::from(option.value != tier_default(ModelTier::Sonnet)));
    options.into_boxed_slice()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_parses_and_exposes_tier_defaults() {
        assert_eq!(
            tier_default(ModelTier::Sonnet),
            crate::defaults::DEFAULT_MODEL
        );
        assert_eq!(task_role_default(TaskRole::Prosoche), names::haiku());
        assert!(!as_of().is_empty());
    }

    #[test]
    fn provider_catalogs_are_nonempty() {
        assert!(provider_models(ModelProvider::Anthropic).len() >= 3);
        assert_eq!(provider_models(ModelProvider::Codex), [names::codex()]);
        assert_eq!(provider_models(ModelProvider::Kimi), [names::kimi()]);
    }

    #[test]
    fn pricing_and_cache_ratios_come_from_seed() {
        let pricing = pricing_entries().collect::<Vec<_>>();
        assert!(pricing.iter().any(|(id, _)| *id == names::haiku()));
        assert!((cache_read_ratio() - 0.10).abs() < f64::EPSILON);
        assert!((cache_write_ratio() - 1.25).abs() < f64::EPSILON);
    }

    #[test]
    fn menu_labels_derive_from_values() {
        let options = model_menu_options();
        assert!(
            options
                .iter()
                .any(|option| option.value == crate::defaults::DEFAULT_MODEL)
        );
        for option in options {
            assert!(
                option.label.starts_with(option.value),
                "label must derive from value: {option:?}"
            );
        }
    }
}
