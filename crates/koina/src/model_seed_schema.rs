// WHY (#7025): included verbatim by both `src/models.rs` (runtime) and
// `build.rs` (compile-time validation) via
// `include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/model_seed_schema.rs"))`,
// so the two can never accept different languages for `data/model-seed.toml`.
// Every path here is fully qualified (`serde::Deserialize`, `std::fmt::...`)
// rather than relying on a `use` in the includer, because the same `use`
// appearing in both this file and the includer is a hard compile error
// (E0252: name defined multiple times) — this file must compile standing
// alone in either splice site.

/// Model capability tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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

impl std::fmt::Display for ModelTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoLlm => f.write_str("no_llm"),
            Self::Haiku => f.write_str("haiku"),
            Self::Sonnet => f.write_str("sonnet"),
            Self::Opus => f.write_str("opus"),
        }
    }
}

/// Provider family that owns a model catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

// WHY dead_code: fields are read only by serde during deserialization in the
// build-time splice site (`build.rs` never accesses them by name); the
// runtime splice site (`models.rs`) does read them, so the allow is a no-op
// there.
#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
struct ModelSeed {
    as_of: String,
    cache: CacheSeed,
    tiers: TierSeed,
    task_roles: TaskRoleSeed,
    models: Vec<ModelEntry>,
}

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
struct CacheSeed {
    read_ratio: f64,
    write_ratio: f64,
}

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
struct TierSeed {
    opus: String,
    sonnet: String,
    haiku: String,
}

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
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

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize)]
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
